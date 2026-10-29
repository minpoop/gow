//! "Who writes here": the processor's debug registers (DR0-DR3) set to trap writes to up to four addresses in every
//! thread of Ragnarök for a moment. A vectored exception handler catches each trap (it comes just after the writing
//! instruction) and keeps, per address and code location, the registers, the code bytes around it and 64 bytes
//! around every register that points into readable memory. Nothing is changed: the write has already happened and
//! the thread carries on.
//!
//! `with_callback` (0.5.0) runs the probe's own callback in the trapping thread instead of recording: on writes (just
//! after the write) or on execution (just before the instruction runs; the resume flag is set so it then runs). The
//! callback gets the thread's registers. Writes the callback makes to a watched address trap again; those nested
//! traps are passed over (`IN_CALLBACK`).

use crate::plog;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, EXCEPTION_SINGLE_STEP};
use windows::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, GetThreadContext, ReadProcessMemory, RemoveVectoredExceptionHandler, SetThreadContext, CONTEXT,
    CONTEXT_DEBUG_REGISTERS_AMD64, EXCEPTION_POINTERS,
};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT,
    THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME,
};

const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
const MAX_HITS: usize = 32;
/// The same code location is kept at most this many times per address.
const PER_SITE: usize = 2;
const CODE_BEFORE: usize = 64;
const CODE_AFTER: usize = 32;

#[derive(Clone, Copy)]
struct Hit {
    slot: u32,
    rip: u64,
    thread: u32,
    regs: [u64; 16],
    code: [u8; CODE_BEFORE + CODE_AFTER],
    code_ok: bool,
    around: [[u8; 64]; 16],
    around_ok: [bool; 16],
}

const EMPTY: Hit = Hit {
    slot: 0,
    rip: 0,
    thread: 0,
    regs: [0; 16],
    code: [0; CODE_BEFORE + CODE_AFTER],
    code_ok: false,
    around: [[0; 64]; 16],
    around_ok: [false; 16],
};

struct Hits(std::cell::UnsafeCell<[Hit; MAX_HITS]>);
unsafe impl Sync for Hits {}
static HITS: Hits = Hits(std::cell::UnsafeCell::new([EMPTY; MAX_HITS]));
/// Slots claimed (written once each, by whichever thread claims it).
static CLAIMED: AtomicUsize = AtomicUsize::new(0);
static TRAPS: [AtomicU32; 4] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static MASK: AtomicU32 = AtomicU32::new(0);
/// Slots handled by the callback (a `fn(u32, &mut CONTEXT)` as usize), and which of them are execution traps.
static CB_MASK: AtomicU32 = AtomicU32::new(0);
static EXEC_MASK: AtomicU32 = AtomicU32::new(0);
static CALLBACK: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static IN_CALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const REG_NAMES: [&str; 16] = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];

fn copy(addr: u64, out: &mut [u8]) -> bool {
    let mut got = 0usize;
    unsafe {
        ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, out.as_mut_ptr() as *mut c_void, out.len(), Some(&mut got)).is_ok()
            && got == out.len()
    }
}

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    let info = &*info;
    if (*info.ExceptionRecord).ExceptionCode != EXCEPTION_SINGLE_STEP {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let ctx = &mut *info.ContextRecord;
    let ours = (ctx.Dr6 as u32) & MASK.load(Relaxed);
    if ours == 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    ctx.Dr6 = 0;
    let slot = ours.trailing_zeros();
    TRAPS[slot as usize].fetch_add(1, Relaxed);
    if ours & CB_MASK.load(Relaxed) != 0 {
        let f = CALLBACK.load(Relaxed);
        if f != 0 && !IN_CALLBACK.with(|c| c.replace(true)) {
            let f: fn(u32, &mut CONTEXT) = std::mem::transmute(f);
            f(slot, ctx);
            IN_CALLBACK.with(|c| c.set(false));
        }
        if ours & EXEC_MASK.load(Relaxed) != 0 {
            ctx.EFlags |= 0x10000; // resume flag: run the instruction without trapping again
        }
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    let rip = ctx.Rip;
    // keep a code location only a couple of times per address
    let n = CLAIMED.load(Relaxed).min(MAX_HITS);
    let hits = &*HITS.0.get();
    let seen = hits[..n].iter().filter(|h| h.slot == slot && h.rip == rip).count();
    if seen >= PER_SITE {
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    let k = CLAIMED.fetch_add(1, Relaxed);
    if k >= MAX_HITS {
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    let h = &mut (*HITS.0.get())[k];
    h.slot = slot;
    h.rip = rip;
    h.thread = GetCurrentThreadId();
    h.regs = [
        ctx.Rax, ctx.Rcx, ctx.Rdx, ctx.Rbx, ctx.Rsp, ctx.Rbp, ctx.Rsi, ctx.Rdi, ctx.R8, ctx.R9, ctx.R10, ctx.R11, ctx.R12, ctx.R13, ctx.R14,
        ctx.R15,
    ];
    h.code_ok = copy(rip - CODE_BEFORE as u64, &mut h.code);
    for r in 0..16 {
        let v = h.regs[r];
        h.around_ok[r] = v > 0x10000 && v < 0x7fff_ffff_0000 && copy(v - 32, &mut h.around[r]);
    }
    EXCEPTION_CONTINUE_EXECUTION
}

/// Debug registers of every thread but this one: `addrs` (up to 4, each 4-byte aligned) watched for writes of 4
/// bytes (or, for slots in `exec`, trapped when the instruction there is about to run), or all cleared when empty.
fn set_all(addrs: &[usize], exec: u32) -> (u32, u32) {
    let (mut ok, mut failed) = (0, 0);
    let me = unsafe { GetCurrentThreadId() };
    let pid = unsafe { GetCurrentProcessId() };
    let mut dr7 = 0u64;
    for i in 0..addrs.len() {
        dr7 |= 1 << (2 * i); // local enable
        if exec & (1 << i) == 0 {
            dr7 |= 0b01 << (16 + 4 * i); // break on writes
            dr7 |= 0b11 << (18 + 4 * i); // 4 bytes
        } // execution: RW 00, LEN 00
    }
    // made before any thread is suspended: a suspended thread may hold the heap's lock, and allocating then hangs
    let mut ctx = Box::new(CONTEXT::default());
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return (0, 0) };
        let mut e = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut more = Thread32First(snap, &mut e).is_ok();
        while more {
            if e.th32OwnerProcessID == pid && e.th32ThreadID != me {
                match OpenThread(THREAD_GET_CONTEXT | THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME, false, e.th32ThreadID) {
                    Ok(t) => {
                        SuspendThread(t);
                        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
                        let good = GetThreadContext(t, &mut *ctx).is_ok() && {
                            ctx.Dr0 = addrs.first().copied().unwrap_or(0) as u64;
                            ctx.Dr1 = addrs.get(1).copied().unwrap_or(0) as u64;
                            ctx.Dr2 = addrs.get(2).copied().unwrap_or(0) as u64;
                            ctx.Dr3 = addrs.get(3).copied().unwrap_or(0) as u64;
                            ctx.Dr6 = 0;
                            ctx.Dr7 = dr7;
                            ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
                            SetThreadContext(t, &*ctx).is_ok()
                        };
                        ResumeThread(t);
                        let _ = CloseHandle(t);
                        if good {
                            ok += 1;
                        } else {
                            failed += 1;
                        }
                    }
                    Err(_) => failed += 1,
                }
            }
            more = Thread32Next(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    (ok, failed)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
}

/// Watch `targets` (label, address) for `ms`, then log every writer found.
pub fn watch(n: u32, targets: &[(String, usize)], ms: u64, place: impl Fn(usize) -> String) {
    let targets: Vec<&(String, usize)> = targets.iter().filter(|t| t.1 % 4 == 0).take(4).collect();
    if targets.is_empty() {
        return;
    }
    CLAIMED.store(0, Relaxed);
    for t in &TRAPS {
        t.store(0, Relaxed);
    }
    MASK.store((1u32 << targets.len()) - 1, Relaxed);
    let veh = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    if veh.is_null() {
        plog!("watch", "#{n} could not add the exception handler; no watch");
        return;
    }
    let addrs: Vec<usize> = targets.iter().map(|t| t.1).collect();
    let (ok, failed) = set_all(&addrs, 0);
    plog!("watch", "#{n} watching {} addresses in {ok} threads ({failed} could not be set) for {ms} ms", addrs.len());
    std::thread::sleep(Duration::from_millis(ms));
    let (cleared, _) = set_all(&[], 0);
    // traps already raised finish in the handler; give them a moment before it goes
    std::thread::sleep(Duration::from_millis(50));
    MASK.store(0, Relaxed);
    unsafe {
        RemoveVectoredExceptionHandler(veh);
    }
    plog!("watch", "#{n} cleared in {cleared} threads");

    let count = CLAIMED.load(Relaxed).min(MAX_HITS);
    let hits = unsafe { &*HITS.0.get() };
    for (i, t) in targets.iter().enumerate() {
        plog!("watch", "#{n} [{i}] {} at {:#x} ({}): {} writes", t.0, t.1, place(t.1), TRAPS[i].load(Relaxed));
    }
    for h in &hits[..count] {
        plog!("watch", "#{n} [{}] write by {} (rip {:#x}, thread {})", h.slot, place(h.rip as usize), h.rip, h.thread);
        if h.code_ok {
            plog!("watch", "#{n}     code {:#x}: {} | {}", h.rip - CODE_BEFORE as u64, hex(&h.code[..CODE_BEFORE]), hex(&h.code[CODE_BEFORE..]));
        }
        let regs: Vec<String> = (0..16).map(|r| format!("{}={:#x}", REG_NAMES[r], h.regs[r])).collect();
        plog!("watch", "#{n}     {}", regs.join(" "));
        for r in 0..16 {
            if h.around_ok[r] {
                plog!("watch", "#{n}     [{}-32] ({}): {}", REG_NAMES[r], place(h.regs[r] as usize), hex(&h.around[r]));
            }
        }
    }
}

/// For `ms`, run `f(slot, registers)` in whichever thread hits a trap: `traps` are (address, execution?) pairs, up to
/// 4 (execution traps on an instruction's first byte, write traps on 4 aligned bytes). Returns the traps per slot.
pub fn with_callback(n: u32, traps: &[(usize, bool)], ms: u64, f: fn(u32, &mut CONTEXT)) -> [u32; 4] {
    let traps = &traps[..traps.len().min(4)];
    for t in &TRAPS {
        t.store(0, Relaxed);
    }
    let exec: u32 = traps.iter().enumerate().filter(|(_, t)| t.1).map(|(i, _)| 1 << i).sum();
    let all = (1u32 << traps.len()) - 1;
    CALLBACK.store(f as usize, Relaxed);
    EXEC_MASK.store(exec, Relaxed);
    CB_MASK.store(all, Relaxed);
    MASK.store(all, Relaxed);
    let veh = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    if veh.is_null() {
        plog!("watch", "#{n} could not add the exception handler; no traps");
        return [0; 4];
    }
    let addrs: Vec<usize> = traps.iter().map(|t| t.0).collect();
    let (ok, failed) = set_all(&addrs, exec);
    plog!("watch", "#{n} {} traps set in {ok} threads ({failed} could not be set) for {ms} ms", traps.len());
    std::thread::sleep(Duration::from_millis(ms));
    let (cleared, _) = set_all(&[], 0);
    std::thread::sleep(Duration::from_millis(50));
    MASK.store(0, Relaxed);
    CB_MASK.store(0, Relaxed);
    EXEC_MASK.store(0, Relaxed);
    unsafe {
        RemoveVectoredExceptionHandler(veh);
    }
    CALLBACK.store(0, Relaxed);
    let counts = [0, 1, 2, 3].map(|i| TRAPS[i].load(Relaxed));
    plog!("watch", "#{n} traps cleared in {cleared} threads; per trap: {:?}", &counts[..traps.len()]);
    counts
}

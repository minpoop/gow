//! Stage 2b of live Kratos: move Ragnarök's camera. 0.3.0's run (MODLOG) found what the per-view camera blocks are
//! built from: a camera kept in GoWR.exe's own data at +0x39c7110 (this build, PE time stamp 0x6859ad47): its view
//! matrix (rows from +0x30, translation at +0x70) and, rows of 16 bytes from +0xc0, its x, y and z axes, then its
//! position at +0xf0. 0.4.0 raised it at the start of the block builder (+0x4e7516's function) and the picture did
//! not move; its run showed two writers of the camera there, both on the game's camera thread: `GoWR.exe+0x3cb18e`
//! copies a whole camera record into it (rcx = the camera there, rdx = the record) and `GoWR.exe+0x498b56` writes
//! its rows.
//!
//! 0.5.0's run: A (at the builder, as 0.4.0) again moved nothing; B stopped itself after 2 raises (the second writer
//! rewrites the camera from what it reads there, so the raised value came back and looked like a jump); C was set
//! aside every time (the copy function is a general 256-byte copy, 1,566 calls a second).
//!
//! 0.6.0, Ctrl+F9 with Ragnarök in front; one beep, hold still (about 15 s); three beeps, done:
//! * the camera there and 1 KB around it logged; copies of the position and the view translation in it found;
//!   "who writes here" for a second on the position and the view translation, now with each writer's callers, and
//!   the code of those functions saved (`code_<rva>.bin`, read with tools/live_kratos/disasm.py);
//! * two ways of raising the camera 2 m, 3 s each, a screenshot in each and one before and after:
//!   B. right after each write the game makes to the camera there (it stays raised until the game's next write; a
//!      write of the already raised camera is left alone);
//!   C. at the start of the copy function, in the record it is about to copy (only when it copies a camera into
//!      that place: the record's axes are a rotation and its position within 5 m of the camera's), with why the
//!      others were set aside and the first records seen.
//!   A jump of the real camera over 1.5 m stops an experiment (the game would be reading the raised camera back).
//! Another build of Ragnarök (other time stamp) or an unexpected layout: 0.3.0's search runs instead.

use crate::camsearch::{beep, place};
use crate::plog;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, RtlLookupFunctionEntry, CONTEXT};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

const STAMP: u32 = 0x6859ad47;
const GLOBAL_RVA: usize = 0x39c7110;
const BUILDER_RVA: usize = 0x4e7516;
const COPY_RVA: usize = 0x3cb18e;
const WIN_BEFORE: usize = 0x110;
const WIN_LEN: usize = 0x400;
const RAISE_M: f32 = 2.0;

/// A place in the camera's KB holding the position or a view translation: offset from the camera, stride between
/// its three floats, and which (0 position, 1 -axis·p, 2 -(Σ axis·p) the other order).
#[derive(Clone, Copy, Default)]
struct Match {
    off: isize,
    stride: usize,
    what: u8,
}

const MAX_MATCHES: usize = 16;
struct Shared<T>(std::cell::UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}

static GLOBAL: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static MATCHES: Shared<[Match; MAX_MATCHES]> = Shared(std::cell::UnsafeCell::new([Match { off: 0, stride: 0, what: 0 }; MAX_MATCHES]));
static N_MATCHES: AtomicUsize = AtomicUsize::new(0);
/// The real values of the matches (3 floats each) from the last raise, and the raised position written.
static REAL: Shared<[[f32; 3]; MAX_MATCHES]> = Shared(std::cell::UnsafeCell::new([[0.0; 3]; MAX_MATCHES]));
static WRITTEN: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static HAVE_WRITTEN: AtomicBool = AtomicBool::new(false);
static LAST_REAL: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static HAVE_REAL: AtomicBool = AtomicBool::new(false);
static APPLIED: AtomicU32 = AtomicU32::new(0);
static SKIPPED: AtomicU32 = AtomicU32::new(0);
static RESTORED: AtomicU32 = AtomicU32::new(0);
static JUMPS: AtomicU32 = AtomicU32::new(0);
static IGNORED: AtomicU32 = AtomicU32::new(0);
/// C: calls set aside because the destination was not the camera, the record was unreadable, its axes were not a
/// rotation, its position was not near the camera's; and the first records copied into the camera.
static C_WHY: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static C_SAMPLES_N: AtomicU32 = AtomicU32::new(0);
static C_SAMPLES: Shared<[(usize, [f32; 9]); 3]> = Shared(std::cell::UnsafeCell::new([(0, [0.0; 9]); 3]));

unsafe fn rd(a: usize) -> f32 {
    std::ptr::read_volatile(a as *const f32)
}
unsafe fn wr(a: usize, v: f32) {
    std::ptr::write_volatile(a as *mut f32, v)
}

/// The camera's axes (rows) and position, read from the game.
unsafe fn camera(g: usize) -> ([[f32; 3]; 3], [f32; 3]) {
    let row = |o: usize| [rd(g + o), rd(g + o + 4), rd(g + o + 8)];
    ([row(0xc0), row(0xd0), row(0xe0)], row(0xf0))
}

/// What a match should hold for camera axes `r` and position `p`.
fn value(what: u8, r: &[[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    match what {
        0 => p,
        1 => [-dot(r[0], p), -dot(r[1], p), -dot(r[2], p)],
        _ => [0, 1, 2].map(|j| -(r[0][j] * p[0] + r[1][j] * p[1] + r[2][j] * p[2])),
    }
}

/// Write `v(match)` for every match in the camera record at `base`.
unsafe fn raise_record(base: usize, r: &[[f32; 3]; 3], raised: [f32; 3]) {
    let n = N_MATCHES.load(Relaxed);
    let ms = &*MATCHES.0.get();
    for m in &ms[..n] {
        let a = (base as isize + m.off) as usize;
        let v = value(m.what, r, raised);
        for c in 0..3 {
            wr(a + c * m.stride, v[c]);
        }
    }
}

/// True (and the experiment stopped) when the real camera moved more than 1.5 m since the last raise.
fn jumped(p: [f32; 3]) -> bool {
    if HAVE_REAL.load(Relaxed) {
        let last = [0, 1, 2].map(|i| f32::from_bits(LAST_REAL[i].load(Relaxed)));
        let d = ((p[0] - last[0]).powi(2) + (p[1] - last[1]).powi(2) + (p[2] - last[2]).powi(2)).sqrt();
        if d > 1.5 {
            JUMPS.fetch_add(1, Relaxed);
            ACTIVE.store(false, Relaxed);
            return true;
        }
    }
    for i in 0..3 {
        LAST_REAL[i].store(p[i].to_bits(), Relaxed);
    }
    HAVE_REAL.store(true, Relaxed);
    false
}

/// Which code wrote the camera during B, and how often (rip, count).
static WRITERS: [(AtomicU64, AtomicU32); 8] = [const { (AtomicU64::new(0), AtomicU32::new(0)) }; 8];

fn note_writer(rip: u64) {
    for (r, c) in &WRITERS {
        if r.load(Relaxed) == rip || r.compare_exchange(0, rip, Relaxed, Relaxed).is_ok() {
            c.fetch_add(1, Relaxed);
            return;
        }
    }
}

/// B: the game just wrote the camera there (slot 0: its position; slot 1: its view translation). Raise it again,
/// unless what it wrote is already the raised camera (a writer that read it back during the frame).
fn on_write(slot: u32, ctx: &mut CONTEXT) {
    let g = GLOBAL.load(Relaxed);
    if g == 0 || !ACTIVE.load(Relaxed) {
        return;
    }
    note_writer(ctx.Rip | ((slot as u64) << 63));
    unsafe {
        let (r, p) = camera(g);
        if slot == 0 && HAVE_REAL.load(Relaxed) {
            let last = [0, 1, 2].map(|i| f32::from_bits(LAST_REAL[i].load(Relaxed)));
            let raised = [last[0], last[1] + RAISE_M, last[2]];
            if (0..3).all(|i| (p[i] - raised[i]).abs() < 0.01) {
                SKIPPED.fetch_add(1, Relaxed);
                return;
            }
        }
        let real = if slot == 0 {
            if jumped(p) {
                return;
            }
            p
        } else if HAVE_REAL.load(Relaxed) {
            [0, 1, 2].map(|i| f32::from_bits(LAST_REAL[i].load(Relaxed)))
        } else {
            SKIPPED.fetch_add(1, Relaxed);
            return;
        };
        raise_record(g, &r, [real[0], real[1] + RAISE_M, real[2]]);
        APPLIED.fetch_add(1, Relaxed);
    }
}

fn read_bytes(addr: usize, out: &mut [u8]) -> bool {
    let mut got = 0usize;
    unsafe {
        ReadProcessMemory(GetCurrentProcess(), addr as *const std::ffi::c_void, out.as_mut_ptr() as *mut _, out.len(), Some(&mut got)).is_ok()
            && got == out.len()
    }
}

/// C: the copy function is about to run (rcx = where to, rdx = the record). Raise the record if it is a camera
/// going into the camera's place.
fn on_copy(_slot: u32, ctx: &mut CONTEXT) {
    let g = GLOBAL.load(Relaxed);
    if g == 0 || !ACTIVE.load(Relaxed) {
        return;
    }
    let (dst, src) = (ctx.Rcx as usize, ctx.Rdx as usize);
    let mut b = [0u8; 0x40];
    if dst != g {
        C_WHY[0].fetch_add(1, Relaxed);
        IGNORED.fetch_add(1, Relaxed);
        return;
    }
    if src < 0x10000 || !read_bytes(src + 0xc0, &mut b) {
        C_WHY[1].fetch_add(1, Relaxed);
        IGNORED.fetch_add(1, Relaxed);
        return;
    }
    let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let r = [[f(0), f(4), f(8)], [f(16), f(20), f(24)], [f(32), f(36), f(40)]];
    let p = [f(48), f(52), f(56)];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let rot = (0..3).all(|i| (dot(r[i], r[i]) - 1.0).abs() < 2e-3) && dot(r[0], r[1]).abs() < 2e-3 && dot(r[1], r[2]).abs() < 2e-3;
    let (_, here) = unsafe { camera(g) };
    let near = (0..3).all(|i| (p[i] - here[i]).abs() < 5.0);
    let k = C_SAMPLES_N.fetch_add(1, Relaxed) as usize;
    if k < 3 {
        let samples = unsafe { &mut *C_SAMPLES.0.get() };
        samples[k] = (src, [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], p[0], p[1], p[2]]);
    }
    if !rot || !near {
        C_WHY[if rot { 3 } else { 2 }].fetch_add(1, Relaxed);
        IGNORED.fetch_add(1, Relaxed);
        return;
    }
    if jumped(p) {
        return;
    }
    unsafe { raise_record(src, &r, [p[0], p[1] + RAISE_M, p[2]]) };
    APPLIED.fetch_add(1, Relaxed);
}

/// From the Present hook: put the real camera back if it is still the raised one.
pub fn restore() {
    let g = GLOBAL.load(Relaxed);
    if g == 0 || !HAVE_WRITTEN.load(Relaxed) {
        return;
    }
    unsafe {
        let (_, p) = camera(g);
        let bits = p.map(f32::to_bits);
        if (0..3).all(|i| WRITTEN[i].load(Relaxed) == bits[i]) {
            let n = N_MATCHES.load(Relaxed);
            let ms = &*MATCHES.0.get();
            let real = &*REAL.0.get();
            for (k, m) in ms[..n].iter().enumerate() {
                let a = (g as isize + m.off) as usize;
                for c in 0..3 {
                    wr(a + c * m.stride, real[k][c]);
                }
            }
            RESTORED.fetch_add(1, Relaxed);
        }
        HAVE_WRITTEN.store(false, Relaxed);
    }
}

fn pe_stamp(base: usize) -> u32 {
    unsafe {
        let pe = *((base + 0x3c) as *const u32) as usize;
        *((base + pe + 8) as *const u32)
    }
}

/// A leaf function (no unwind entry): its start guessed as the first byte after the int3 padding (0xcc) or the
/// return (0xc3, then 16-aligned) that ends the function before it, at most 4 KB back.
fn leaf_start(pc: usize) -> Option<(usize, usize)> {
    // up to and including `pc` itself, which may be the start
    let mut b = vec![0u8; 0x1001];
    let from = pc - 0x1000;
    if !read_bytes(from, &mut b) {
        return None;
    }
    for i in (1..b.len()).rev() {
        let at = from + i;
        if b[i - 1] == 0xcc && b[i] != 0xcc || b[i - 1] == 0xc3 && at % 16 == 0 {
            return Some((at, pc));
        }
    }
    None
}

/// The start of the function holding `pc`, from the unwind table (following chained entries); for a leaf function
/// (none there) `leaf_start`'s guess, marked by an end equal to `pc`.
fn function_start(pc: usize) -> Option<(usize, usize)> {
    unsafe {
        let mut image = 0u64;
        let f = RtlLookupFunctionEntry(pc as u64, &mut image, None);
        if f.is_null() {
            return leaf_start(pc);
        }
        let mut e = std::ptr::read_unaligned(f as *const [u32; 3]);
        for _ in 0..16 {
            let info = image as usize + (e[2] & !1) as usize;
            let flags = *(info as *const u8) >> 3;
            if flags & 0x4 == 0 {
                break;
            }
            // chained: the parent entry follows the unwind codes (count rounded up to even, 2 bytes each)
            let count = *((info + 2) as *const u8) as usize;
            e = std::ptr::read_unaligned((info + 4 + ((count + 1) & !1) * 2) as *const [u32; 3]);
        }
        Some((image as usize + e[0] as usize, image as usize + e[1] as usize))
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
}

/// The machine code of the functions that wrote the camera in the last watch, and of their callers in GoWR.exe
/// (up to 8 functions, 16 KB each), saved as `code_<rva>.bin` for disassembly offline.
fn dump_code(n: u32, base: usize) {
    let size = unsafe {
        let pe = *((base + 0x3c) as *const u32) as usize;
        *((base + pe + 0x50) as *const u32) as usize
    };
    let mut funcs: Vec<(usize, usize)> = Vec::new();
    for (_, rip, callers) in crate::watch::sites() {
        for a in std::iter::once(rip).chain(callers).map(|a| a as usize) {
            if a < base || a >= base + size || funcs.len() >= 8 {
                continue;
            }
            if let Some((start, end)) = function_start(a) {
                let end = if end == a { a + 0x80 } else { end };
                if !funcs.iter().any(|f| f.0 == start) {
                    funcs.push((start, end));
                }
            }
        }
    }
    for (start, end) in funcs {
        let len = (end - start).min(0x4000);
        let mut code = vec![0u8; len];
        if !read_bytes(start, &mut code) {
            continue;
        }
        let name = format!("code_{:x}.bin", start - base);
        match std::fs::write(crate::log::dir().join(&name), &code) {
            Ok(()) => plog!("steer", "#{n} code of GoWR.exe+{:#x}..+{:#x} saved as {name} ({len} bytes)", start - base, end - base),
            Err(e) => plog!("steer", "#{n} {name} not saved: {e}"),
        }
    }
}

static RUNS: AtomicU32 = AtomicU32::new(0);

/// Ctrl+F9: on a thread of its own (the camera search's guard is shared).
pub fn start() {
    if crate::camsearch::RUNNING.swap(true, Relaxed) {
        return;
    }
    let n = RUNS.fetch_add(1, Relaxed) + 1;
    std::thread::spawn(move || {
        if std::panic::catch_unwind(|| run(n)).is_err() {
            ACTIVE.store(false, Relaxed);
            restore();
            plog!("steer", "#{n} PANIC; stopped");
            beep(3, 600);
        }
        crate::camsearch::RUNNING.store(false, Relaxed);
    });
}

fn run(n: u32) {
    if n > crate::camsearch::MAX_RUNS {
        plog!("steer", "already tried {} times this session; restart Ragnarök for more", crate::camsearch::MAX_RUNS);
        return;
    }
    let t0 = Instant::now();
    let base = unsafe { GetModuleHandleW(None) }.map(|m| m.0 as usize).unwrap_or(0);
    let stamp = if base != 0 { pe_stamp(base) } else { 0 };
    // testing aid (tools/fakes): GOWR_PROBE_STEER=<stamp>,<camera rva>,<builder rva>,<copy rva> in hex, for the stand-in
    let (want_stamp, global_rva, builder_rva, copy_rva) = std::env::var("GOWR_PROBE_STEER")
        .ok()
        .and_then(|v| {
            let x: Vec<usize> = v.split(',').filter_map(|t| usize::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()).collect();
            (x.len() == 4).then(|| (x[0] as u32, x[1], x[2], x[3]))
        })
        .unwrap_or((STAMP, GLOBAL_RVA, BUILDER_RVA, COPY_RVA));
    let g = base + global_rva;
    let usable = stamp == want_stamp && unsafe {
        let (r, p) = camera(g);
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        (0..3).all(|i| (dot(r[i], r[i]) - 1.0).abs() < 2e-3)
            && dot(r[0], r[1]).abs() < 2e-3
            && dot(r[0], r[2]).abs() < 2e-3
            && dot(r[1], r[2]).abs() < 2e-3
            && p.iter().all(|v| v.is_finite())
    };
    if !usable {
        plog!("steer", "#{n} GoWR.exe time stamp {stamp:#x} (expected {want_stamp:#x}) or no camera at +{global_rva:#x}; running the 0.3.0 search instead");
        crate::camsearch::run(n);
        return;
    }
    beep(1, 250);
    let (r, p) = unsafe { camera(g) };
    plog!("steer", "#{n} hold still. GoWR.exe at {base:#x}; camera at {g:#x}: position ({:.3}, {:.3}, {:.3})", p[0], p[1], p[2]);
    for (k, a) in r.iter().enumerate() {
        plog!("steer", "#{n}   axis {}: ({:.4}, {:.4}, {:.4})", ["x", "y", "z"][k], a[0], a[1], a[2]);
    }
    let start = g - WIN_BEFORE;
    let mut kb = vec![0u8; WIN_LEN];
    unsafe { std::ptr::copy_nonoverlapping(start as *const u8, kb.as_mut_ptr(), WIN_LEN) };
    for (i, line) in kb.chunks(32).enumerate() {
        plog!("steer", "#{n}   +{:#05x}: {}", i * 32, hex(line));
    }

    // copies of the position and of the view translation in the KB
    let f = |o: usize| f32::from_le_bytes(kb[o..o + 4].try_into().unwrap());
    let mut found: Vec<Match> = Vec::new();
    for what in 0..3u8 {
        let want = value(what, &r, p);
        for stride in [4usize, 16] {
            let mut o = 0;
            while o + 2 * stride + 4 <= WIN_LEN {
                if (0..3).all(|c| (f(o + c * stride) - want[c]).abs() < 1e-3 * want[c].abs().max(1.0)) {
                    let off = o as isize - WIN_BEFORE as isize;
                    if !found.iter().any(|m| m.off == off) && found.len() < MAX_MATCHES {
                        found.push(Match { off, stride, what });
                    }
                }
                o += 4;
            }
        }
    }
    for m in &found {
        let name = ["position", "view translation (-axis·p)", "view translation (other order)"][m.what as usize];
        plog!("steer", "#{n}   {name} at camera{:+#x}, floats {} bytes apart", m.off, m.stride);
    }
    if !found.iter().any(|m| m.what == 0 && m.off == 0xf0 && m.stride == 4) {
        plog!("steer", "#{n} the position is not where expected; stopped");
        beep(3, 600);
        return;
    }
    unsafe {
        let ms = &mut *MATCHES.0.get();
        for (k, m) in found.iter().enumerate() {
            ms[k] = *m;
        }
    }
    N_MATCHES.store(found.len(), Relaxed);

    let view_t = found.iter().find(|m| m.what != 0).map(|m| g.wrapping_add_signed(m.off));
    let mut watched = vec![("camera position x".to_string(), g + 0xf0)];
    if let Some(v) = view_t {
        watched.push(("camera view translation x".to_string(), v));
    }
    crate::watch::watch(n, &watched, 1000, place);
    dump_code(n, base);

    GLOBAL.store(g, Relaxed);
    crate::monitor::want_shot("base");
    std::thread::sleep(Duration::from_millis(800));
    let builder = function_start(base + builder_rva);
    let copy = function_start(base + copy_rva);
    let show = |f: Option<(usize, usize)>, pc: usize| match f {
        Some((a, e)) if e == pc => format!("GoWR.exe+{:#x} (a leaf: start guessed from the padding before it)", a - base),
        Some((a, e)) => format!("GoWR.exe+{:#x}..+{:#x}", a - base, e - base),
        None => "not found".into(),
    };
    plog!("steer", "#{n} builder function {}; copy function {}", show(builder, base + builder_rva), show(copy, base + copy_rva));
    let mut experiments: Vec<(&str, Vec<(usize, bool)>, fn(u32, &mut CONTEXT))> = Vec::new();
    let mut w = vec![(g + 0xf0, false)];
    if let Some(v) = view_t {
        w.push((v & !3, false));
    }
    experiments.push(("B-write-through", w, on_write));
    if let Some((a, _)) = copy {
        experiments.push(("C-copy-record", vec![(a, true)], on_copy));
    }
    for (name, traps, f) in experiments {
        for c in [&APPLIED, &SKIPPED, &RESTORED, &JUMPS, &IGNORED] {
            c.store(0, Relaxed);
        }
        HAVE_REAL.store(false, Relaxed);
        HAVE_WRITTEN.store(false, Relaxed);
        ACTIVE.store(true, Relaxed);
        let label = name.to_string();
        let shots = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            crate::monitor::want_shot(&format!("raised-{label}"));
        });
        let counts = crate::watch::with_callback(n, &traps, 3000, f);
        ACTIVE.store(false, Relaxed);
        let _ = shots.join();
        std::thread::sleep(Duration::from_millis(100));
        restore();
        plog!(
            "steer",
            "#{n} {name}: traps {:?}, raised {}, already raised {}, ignored {}, put back {}, stopped by a jump {}",
            &counts[..traps.len()],
            APPLIED.load(Relaxed),
            SKIPPED.load(Relaxed),
            IGNORED.load(Relaxed),
            RESTORED.load(Relaxed),
            JUMPS.load(Relaxed)
        );
        if name.starts_with('B') {
            for (r, c) in &WRITERS {
                let rip = r.load(Relaxed);
                if rip != 0 {
                    let what = if rip >> 63 == 1 { "view translation" } else { "position" };
                    plog!("steer", "#{n}   {what} written by {} {} times", place((rip & !(1 << 63)) as usize), c.load(Relaxed));
                }
            }
        }
        if name.starts_with('C') {
            let w = [0, 1, 2, 3].map(|i| C_WHY[i].load(Relaxed));
            plog!("steer", "#{n}   set aside: not into the camera {}, record unreadable {}, axes not a rotation {}, too far {}", w[0], w[1], w[2], w[3]);
            let samples = unsafe { &*C_SAMPLES.0.get() };
            for (src, v) in samples.iter().take((C_SAMPLES_N.load(Relaxed) as usize).min(3)) {
                plog!(
                    "steer",
                    "#{n}   record {src:#x}: axis x ({:.4}, {:.4}, {:.4}), axis y ({:.4}, {:.4}, {:.4}), position ({:.3}, {:.3}, {:.3})",
                    v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]
                );
            }
        }
        std::thread::sleep(Duration::from_millis(900));
    }
    GLOBAL.store(0, Relaxed);
    crate::monitor::want_shot("after");
    std::thread::sleep(Duration::from_millis(500));
    plog!("steer", "#{n} done in {:.0} s", t0.elapsed().as_secs_f64());
    beep(3, 250);
}

//! Stage 2a of live Kratos: where does Ragnarök keep its camera? Started with Ctrl+F9 while Ragnarök is in front,
//! guided by beeps (the probe cannot draw on screen). 0.3.0, after 0.2.0's run found the per-frame camera blocks
//! (from 0.4.0 run by `steer` only when Ragnarök is another build than the one it knows):
//!
//! 1. one beep: hold still. Ragnarök's writable memory is searched for projection matrices with the screen's shape
//!    (`[sx,0,0,0; 0,sy,0,0; jx,jy,A,±1; 0,0,B,0]` or its transpose). In Ragnarök's per-view blocks the transposed
//!    projection follows the camera-to-world matrix (proj-0x40: 3x4, rows 16 bytes apart, the position in the
//!    fourth column) and the view matrix (proj-0x80); the camera's position is read from there.
//! 2. still holding still: the memory is searched for that position kept three ways: three floats in a row, three
//!    doubles in a row, and three floats 16 bytes apart (a matrix's fourth column), within 1 cm.
//! 3. still holding still: "who writes here" (`watch`) on the block's position for a second, with a second block's
//!    and up to two of step 2's finds that are not in a block: the code that writes them, its registers and the
//!    memory they point at, in the log.
//! 4. two beeps: move the camera (10 s). 512 bytes around each find of step 2 are recorded 5 times a second
//!    (`camsearch_<n>_pos.bin`).
//! 5. three beeps: done.
//!
//! Files (little-endian): "GWCS", u32 version 1, u32 kind (2 position windows), u32 count, u32 window bytes, u32
//! swap width, u32 swap height, u64 window starts[count]; then records of u32 ms since the search began, u64 frame,
//! u8 readable[count], window bytes x count. Read offline (tools/live_kratos/camsearch.py).

use crate::d3d::{FRAMES, SWAP_H, SWAP_W};
use crate::plog;
use std::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Diagnostics::Debug::{Beep, ReadProcessMemory};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_PRIVATE, PAGE_READWRITE, PAGE_WRITECOPY};
use windows::Win32::System::Threading::GetCurrentProcess;

pub static RUNNING: AtomicBool = AtomicBool::new(false);
pub const MAX_RUNS: u32 = 3;

const POS_WINDOW: usize = 512;
const MAX_POS: usize = 64;
const CHUNK: usize = 1 << 20;

// ---- reading our own memory safely ---------------------------------------------------------------------------------

fn read(addr: usize, buf: &mut [u8]) -> bool {
    let mut got = 0usize;
    unsafe {
        ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, buf.as_mut_ptr() as *mut c_void, buf.len(), Some(&mut got)).is_ok()
            && got == buf.len()
    }
}

struct Region {
    base: usize,
    size: usize,
}

/// Committed, plainly read-write memory: the game's heaps (private) and its modules' data (image). Write-combined,
/// uncached and guard pages are left out (GPU upload heaps are write-combined and very slow to read).
fn regions() -> (Vec<Region>, usize) {
    let mut out = Vec::new();
    let mut total = 0;
    let mut a = 0x10000usize;
    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    while a < 0x7FFF_FFFF_0000 {
        if unsafe { VirtualQuery(Some(a as *const c_void), &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            break;
        }
        let base = mbi.BaseAddress as usize;
        let size = mbi.RegionSize;
        let plain = mbi.Protect == PAGE_READWRITE || (mbi.Type == MEM_IMAGE && mbi.Protect == PAGE_WRITECOPY);
        if mbi.State == MEM_COMMIT && plain && (mbi.Type == MEM_PRIVATE || mbi.Type == MEM_IMAGE) {
            out.push(Region { base, size });
            total += size;
        }
        a = base.saturating_add(size.max(0x1000));
    }
    (out, total)
}

/// Every chunk of every region, with 64 bytes of overlap so nothing straddling a chunk edge is missed. The callback
/// also gets the probe's own copies (`own`: start, length, plus the chunk buffer itself): they hold what the game
/// held, so a find inside one is not the game's.
fn scan(regions: &[Region], own: &[(usize, usize)], mut f: impl FnMut(usize, &[u8], &[(usize, usize)])) {
    let mut buf = vec![0u8; CHUNK + 64];
    let mut skip = own.to_vec();
    skip.push((buf.as_ptr() as usize, buf.len()));
    for r in regions {
        let mut off = 0;
        while off < r.size {
            let len = (r.size - off).min(CHUNK + 64);
            if read(r.base + off, &mut buf[..len]) {
                f(r.base + off, &buf[..len], &skip);
            }
            off += CHUNK;
        }
    }
}

fn mine(own: &[(usize, usize)], addr: usize) -> bool {
    own.iter().any(|(s, n)| addr >= *s && addr < s + n)
}

fn f32_at(b: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// "GoWR.exe+0x1234" for module memory, "heap" otherwise.
pub(crate) fn place(addr: usize) -> String {
    unsafe {
        let mut m = HMODULE::default();
        let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
        if GetModuleHandleExW(flags, windows::core::PCWSTR(addr as *const u16), &mut m).is_ok() {
            let mut name = [0u16; 260];
            let n = GetModuleFileNameW(m, &mut name) as usize;
            let full = String::from_utf16_lossy(&name[..n]);
            let short = full.rsplit('\\').next().unwrap_or(&full).to_string();
            return format!("{short}+{:#x}", addr - m.0 as usize);
        }
    }
    "heap".into()
}

// ---- projections -------------------------------------------------------------------------------------------

struct Proj {
    addr: usize,
    transposed: bool,
    sx: f32,
    sy: f32,
    a: f32,
    b: f32,
    w: f32,
    jx: f32,
    jy: f32,
}

fn near0(v: f32) -> bool {
    v.abs() < 1e-5
}

fn projection_at(b: &[u8], i: usize, aspect: f32) -> Option<(bool, f32, f32, f32, f32, f32, f32, f32)> {
    let u = |k: usize| u32::from_le_bytes([b[i + k * 4], b[i + k * 4 + 1], b[i + k * 4 + 2], b[i + k * 4 + 3]]);
    if u(15) & 0x7fff_ffff != 0 {
        return None;
    }
    let one = |k: usize| u(k) & 0x7fff_ffff == 0x3f80_0000;
    let (row, col) = (one(11), one(14));
    if !row && !col {
        return None;
    }
    let p = |k: usize| f32_at(b, i + k * 4);
    let (sx, sy) = (p(0), p(5));
    if !(0.2..20.0).contains(&sx) || !(0.2..20.0).contains(&sy) {
        return None;
    }
    let ratio = sy / sx;
    if (ratio / aspect - 1.0).abs() > 0.03 && (ratio / (16.0 / 9.0) - 1.0).abs() > 0.01 {
        return None;
    }
    if row {
        // [sx,0,0,0; 0,sy,0,0; jx,jy,A,±1; 0,0,B,0]
        if [1, 2, 3, 4, 6, 7, 12, 13].iter().all(|k| near0(p(*k))) && p(8).abs() < 0.05 && p(9).abs() < 0.05 {
            return Some((false, sx, sy, p(10), p(14), p(11), p(8), p(9)));
        }
    }
    if col {
        // the transpose: [sx,0,jx,0; 0,sy,jy,0; 0,0,A,B; 0,0,±1,0]
        if [1, 3, 4, 7, 8, 9, 12, 13].iter().all(|k| near0(p(*k))) && p(2).abs() < 0.05 && p(6).abs() < 0.05 {
            return Some((true, sx, sy, p(10), p(11), p(14), p(2), p(6)));
        }
    }
    None
}

fn find_projections(regions: &[Region], aspect: f32) -> Vec<Proj> {
    let mut out = Vec::new();
    scan(regions, &[], |base, b, own| {
        let end = b.len().saturating_sub(64).min(CHUNK);
        let mut i = 0;
        while i < end {
            if let Some((transposed, sx, sy, a, bb, w, jx, jy)) = projection_at(b, i, aspect) {
                if out.len() < 4096 && !mine(own, base + i) {
                    out.push(Proj { addr: base + i, transposed, sx, sy, a, b: bb, w, jx, jy });
                }
            }
            i += 4;
        }
    });
    out
}

// ---- recording -----------------------------------------------------------------------------------------------------

pub(crate) struct Recorder {
    file: std::io::BufWriter<std::fs::File>,
    starts: Vec<usize>,
    window: usize,
    t0: Instant,
    buf: Vec<u8>,
    pub first: Vec<Vec<u8>>,
    pub last: Vec<Vec<u8>>,
}

impl Recorder {
    pub(crate) fn new(name: &str, kind: u32, starts: Vec<usize>, window: usize, t0: Instant) -> std::io::Result<Self> {
        let f = std::fs::File::create(crate::log::dir().join(name))?;
        let mut file = std::io::BufWriter::new(f);
        file.write_all(b"GWCS")?;
        for v in [1u32, kind, starts.len() as u32, window as u32, SWAP_W.load(Relaxed), SWAP_H.load(Relaxed)] {
            file.write_all(&v.to_le_bytes())?;
        }
        for s in &starts {
            file.write_all(&(*s as u64).to_le_bytes())?;
        }
        let n = starts.len();
        Ok(Recorder { file, starts, window, t0, buf: vec![0; window], first: vec![Vec::new(); n], last: vec![Vec::new(); n] })
    }

    fn snapshot(&mut self) -> std::io::Result<()> {
        let mut ok = Vec::with_capacity(self.starts.len());
        let mut data = Vec::with_capacity(self.starts.len() * self.window);
        for (k, s) in self.starts.iter().enumerate() {
            let good = read(*s, &mut self.buf);
            if !good {
                self.buf.iter_mut().for_each(|x| *x = 0);
            } else {
                if self.first[k].is_empty() {
                    self.first[k] = self.buf.clone();
                }
                self.last[k] = self.buf.clone();
            }
            ok.push(good as u8);
            data.extend_from_slice(&self.buf);
        }
        self.file.write_all(&(self.t0.elapsed().as_millis() as u32).to_le_bytes())?;
        self.file.write_all(&FRAMES.load(Relaxed).to_le_bytes())?;
        self.file.write_all(&ok)?;
        self.file.write_all(&data)
    }

    pub(crate) fn record(&mut self, secs: u64) -> usize {
        let end = Instant::now() + Duration::from_secs(secs);
        let mut n = 0;
        while Instant::now() < end {
            if self.snapshot().is_ok() {
                n += 1;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = self.file.flush();
        n
    }
}

// ---- per-view blocks and the position kept three ways --------------------------------------------------------------

/// A per-view block: its transposed projection at `proj`, camera-to-world at proj-0x40.
struct Block {
    proj: usize,
    fov: f32,
    pos: [f32; 3],
}

fn blocks(projs: &[Proj]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut b = [0u8; 0x40];
    for p in projs.iter().filter(|p| p.transposed) {
        if !read(p.addr - 0x40, &mut b) || rot_at(&b, 0, 16).is_none() {
            continue;
        }
        let pos = [f32_at(&b, 12), f32_at(&b, 28), f32_at(&b, 44)];
        if pos.iter().all(|v| v.is_finite()) && pos.iter().any(|v| v.abs() > 0.5) {
            out.push(Block { proj: p.addr, fov: 2.0 * (1.0 / p.sy).atan().to_degrees(), pos });
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Floats,
    Doubles,
    Column,
}

fn f64_at(b: &[u8], i: usize) -> f64 {
    f64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

/// Every place holding `p` as three floats in a row, three doubles in a row, or three floats 16 bytes apart.
fn find_kept(regions: &[Region], p: [f32; 3], own: &[(usize, usize)]) -> Vec<(usize, Kind)> {
    const EPS: f64 = 0.01;
    let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
    let close = |a: f64, b: f64| (a - b).abs() < EPS;
    let mut out = Vec::new();
    scan(regions, own, |base, b, own| {
        let end = b.len().saturating_sub(48).min(CHUNK);
        let mut i = 0;
        while i < end {
            if out.len() >= 8192 {
                return;
            }
            let at = base + i;
            if close(f32_at(b, i) as f64, x) && !mine(own, at) {
                if close(f32_at(b, i + 4) as f64, y) && close(f32_at(b, i + 8) as f64, z) {
                    out.push((at, Kind::Floats));
                } else if close(f32_at(b, i + 16) as f64, y) && close(f32_at(b, i + 32) as f64, z) {
                    out.push((at, Kind::Column));
                }
            }
            if i % 8 == 0 && close(f64_at(b, i), x) && close(f64_at(b, i + 8), y) && close(f64_at(b, i + 16), z) && !mine(own, at) {
                out.push((at, Kind::Doubles));
            }
            i += 4;
        }
    });
    out
}

// ---- the search ----------------------------------------------------------------------------------------------------

fn rot_at(b: &[u8], o: usize, stride: usize) -> Option<[[f32; 3]; 3]> {
    if o + stride * 2 + 12 > b.len() {
        return None;
    }
    let r = [0, 1, 2].map(|k| [0, 1, 2].map(|c| f32_at(b, o + k * stride + c * 4)));
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    for k in 0..3 {
        if !r[k].iter().all(|v| v.is_finite()) || (dot(r[k], r[k]) - 1.0).abs() > 2e-3 {
            return None;
        }
    }
    if dot(r[0], r[1]).abs() > 2e-3 || dot(r[0], r[2]).abs() > 2e-3 || dot(r[1], r[2]).abs() > 2e-3 {
        return None;
    }
    Some(r)
}

pub(crate) fn beep(n: u32, ms: u32) {
    for _ in 0..n {
        unsafe {
            let _ = Beep(880, ms);
        }
        std::thread::sleep(Duration::from_millis(120));
    }
}

pub(crate) fn run(n: u32) {
    let t0 = Instant::now();
    let (w, h) = (SWAP_W.load(Relaxed).max(1), SWAP_H.load(Relaxed).max(1));
    beep(1, 250);
    plog!("camsearch", "#{n} started (screen {w}x{h}); hold still");
    let (regs, total) = regions();
    plog!("camsearch", "#{n} {} regions, {} MB of read-write memory", regs.len(), total >> 20);

    let projs = find_projections(&regs, w as f32 / h as f32);
    let blocks = blocks(&projs);
    plog!("camsearch", "#{n} {} projections, {} per-view blocks with a camera ({:.1} s)", projs.len(), blocks.len(), t0.elapsed().as_secs_f64());
    for p in projs.iter().take(8) {
        plog!(
            "camsearch",
            "#{n}   projection {:#x} {} sx {:.5} sy {:.5} A {:.6} B {:.6} w {:+} jitter ({:.6}, {:.6})",
            p.addr, if p.transposed { "T" } else { "R" }, p.sx, p.sy, p.a, p.b, p.w, p.jx, p.jy
        );
    }
    for b in blocks.iter().take(24) {
        plog!("camsearch", "#{n}   block: projection {:#x} ({}), fov {:.2}, camera at ({:.3}, {:.3}, {:.3})", b.proj, place(b.proj), b.fov, b.pos[0], b.pos[1], b.pos[2]);
    }
    // the gameplay camera: the position most blocks agree on
    let Some(cam) = blocks
        .iter()
        .max_by_key(|b| blocks.iter().filter(|o| (0..3).all(|i| (o.pos[i] - b.pos[i]).abs() < 0.01)).count())
        .map(|b| b.pos)
    else {
        plog!("camsearch", "#{n} no per-view block with a camera; stopped");
        beep(3, 600);
        return;
    };
    plog!("camsearch", "#{n} camera at ({:.3}, {:.3}, {:.3})", cam[0], cam[1], cam[2]);

    let ts = Instant::now();
    let kept = find_kept(&regs, cam, &[]);
    let near_block = |a: usize| blocks.iter().any(|b| a + 0x100 >= b.proj && a < b.proj + 0x100);
    for kind in [Kind::Doubles, Kind::Column, Kind::Floats] {
        let of: Vec<&(usize, Kind)> = kept.iter().filter(|k| k.1 == kind).collect();
        let list: Vec<String> = of.iter().take(16).map(|k| format!("{:#x} ({}{})", k.0, place(k.0), if near_block(k.0) { ", block" } else { "" })).collect();
        plog!("camsearch", "#{n} kept as {kind:?}: {} places{}{}", of.len(), if of.is_empty() { "" } else { ": " }, list.join(", "));
    }
    plog!("camsearch", "#{n} position search took {:.1} s", ts.elapsed().as_secs_f64());

    // who writes them: the gameplay block's position, another block's, and up to two finds outside the blocks
    let mut targets: Vec<(String, usize)> = Vec::new();
    let agree: Vec<&Block> = blocks.iter().filter(|b| (0..3).all(|i| (b.pos[i] - cam[i]).abs() < 0.01)).collect();
    for b in agree.iter().take(2) {
        targets.push((format!("block camera x (fov {:.1})", b.fov), b.proj - 0x40 + 12));
    }
    for kind in [Kind::Doubles, Kind::Column, Kind::Floats] {
        for k in kept.iter().filter(|k| k.1 == kind && !near_block(k.0)) {
            if targets.len() >= 4 || targets.iter().filter(|t| !t.0.starts_with("block")).count() >= 2 {
                break;
            }
            targets.push((format!("{kind:?} x"), k.0));
        }
    }
    crate::watch::watch(n, &targets, 1000, place);

    let mut starts: Vec<usize> = Vec::new();
    let mut order: Vec<&(usize, Kind)> = kept.iter().filter(|k| k.1 != Kind::Floats).collect();
    order.extend(kept.iter().filter(|k| k.1 == Kind::Floats));
    for (a, _) in order {
        let s = a.saturating_sub(POS_WINDOW / 2) & !0xf;
        if !starts.iter().any(|x| s < x + POS_WINDOW && *x < s + POS_WINDOW) {
            starts.push(s);
        }
        if starts.len() >= MAX_POS {
            break;
        }
    }
    if !starts.is_empty() {
        if let Ok(mut rec) = Recorder::new(&format!("camsearch_{n}_pos.bin"), 2, starts, POS_WINDOW, t0) {
            beep(2, 200);
            plog!("camsearch", "#{n} recording {} windows around the finds; move the camera now", rec.starts.len());
            let snaps = rec.record(10);
            plog!("camsearch", "#{n} {snaps} snapshots recorded");
        }
    }
    plog!("camsearch", "#{n} done in {:.0} s", t0.elapsed().as_secs_f64());
    beep(3, 250);
}

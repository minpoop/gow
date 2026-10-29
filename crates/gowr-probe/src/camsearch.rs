//! Stage 2a of live Kratos: where does Ragnarök keep its camera? Started with Ctrl+F9 while Ragnarök is in front,
//! guided by beeps (the probe cannot draw on screen):
//!
//! 1. one beep: hold still. Ragnarök's writable memory is searched for projection matrices with the screen's shape
//!    (`[sx,0,0,0; 0,sy,0,0; jx,jy,A,±1; 0,0,B,0]` or its transpose, sy/sx = width/height, jitter small): the lens.
//!    The camera's own matrices usually sit right beside it.
//! 2. two beeps: turn the camera around, look up and down, walk a little (20 s). 4 KB around each projection found
//!    is recorded 5 times a second (`camsearch_<n>_proj.bin`).
//! 3. one long beep: let go of everything (5 s). Near the projections, rotation matrices that turned during step 2
//!    give candidate camera positions; the whole memory is searched for those positions (`camsearch` log lines).
//! 4. two beeps: move the camera again (10 s). 512 bytes around each place the position was found are recorded
//!    (`camsearch_<n>_pos.bin`).
//! 5. three beeps: done.
//!
//! Files (little-endian): "GWCS", u32 version 1, u32 kind (1 projection windows, 2 position windows), u32 count,
//! u32 window bytes, u32 swap width, u32 swap height, u64 window starts[count]; then records of u32 ms since the
//! search began, u64 frame, u8 readable[count], window bytes x count. Read offline (tools/live_kratos/camsearch.py).

use crate::d3d::{FRAMES, SWAP_H, SWAP_W};
use crate::plog;
use std::ffi::c_void;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::Diagnostics::Debug::{Beep, ReadProcessMemory};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_PRIVATE, PAGE_READWRITE, PAGE_WRITECOPY};
use windows::Win32::System::Threading::GetCurrentProcess;

pub static RUNNING: AtomicBool = AtomicBool::new(false);
static RUNS: AtomicU32 = AtomicU32::new(0);
pub const MAX_RUNS: u32 = 3;

const PROJ_WINDOW: usize = 4096;
const POS_WINDOW: usize = 512;
const MAX_PROJ: usize = 24;
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
fn place(addr: usize) -> String {
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

// ---- step 1: projections -------------------------------------------------------------------------------------------

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

struct Recorder {
    file: std::io::BufWriter<std::fs::File>,
    starts: Vec<usize>,
    window: usize,
    t0: Instant,
    buf: Vec<u8>,
    pub first: Vec<Vec<u8>>,
    pub last: Vec<Vec<u8>>,
}

impl Recorder {
    fn new(name: &str, kind: u32, starts: Vec<usize>, window: usize, t0: Instant) -> std::io::Result<Self> {
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

    fn record(&mut self, secs: u64) -> usize {
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

// ---- step 3: rotations that turned, and the positions they give ----------------------------------------------------

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

fn angle_deg(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> f32 {
    let mut tr = 0.0;
    for i in 0..3 {
        for k in 0..3 {
            tr += a[i][k] * b[i][k];
        }
    }
    ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees()
}

struct Turned {
    window: usize,
    offset: usize,
    stride: usize,
    degrees: f32,
}

/// Rotation blocks (rows 16 or 12 bytes apart) in the windows that are rotations both at the start and the end of
/// the recording and turned more than 15 degrees in between.
fn turned(rec: &Recorder) -> Vec<Turned> {
    let mut out = Vec::new();
    for (w, (a, b)) in rec.first.iter().zip(&rec.last).enumerate() {
        if a.is_empty() || b.is_empty() {
            continue;
        }
        for stride in [16usize, 12] {
            let mut o = 0;
            while o + stride * 2 + 12 <= a.len() {
                if let (Some(ra), Some(rb)) = (rot_at(a, o, stride), rot_at(b, o, stride)) {
                    let d = angle_deg(&ra, &rb);
                    if d > 15.0 {
                        out.push(Turned { window: w, offset: o, stride, degrees: d });
                    }
                }
                o += 4;
            }
        }
    }
    out
}

/// Candidate camera positions from a turned block read now: its translation (as stored after the rows, or in each
/// row's fourth float) and that translation taken back through the rotation both ways (a view matrix stores
/// -R·position).
fn positions(b: &[u8], t: &Turned, out: &mut Vec<[f32; 3]>) {
    let Some(r) = rot_at(b, t.offset, t.stride) else { return };
    let mut ts = Vec::new();
    if t.stride == 16 {
        if t.offset + 60 <= b.len() {
            ts.push([0, 1, 2].map(|c| f32_at(b, t.offset + 48 + c * 4)));
        }
        ts.push([0, 1, 2].map(|k| f32_at(b, t.offset + k * 16 + 12)));
    } else if t.offset + 48 <= b.len() {
        ts.push([0, 1, 2].map(|c| f32_at(b, t.offset + 36 + c * 4)));
    }
    for tr in ts {
        let rt = [0, 1, 2].map(|i| -(r[0][i] * tr[0] + r[1][i] * tr[1] + r[2][i] * tr[2]));
        let r_ = [0, 1, 2].map(|i| -(r[i][0] * tr[0] + r[i][1] * tr[1] + r[i][2] * tr[2]));
        for p in [tr, rt, r_] {
            let mag = p.iter().map(|v| v.abs()).fold(0.0, f32::max);
            if p.iter().all(|v| v.is_finite()) && mag > 0.5 && mag < 1e6 && !out.iter().any(|q| (0..3).all(|i| (q[i] - p[i]).abs() < 1e-3)) {
                out.push(p);
            }
        }
    }
}

/// Every place in memory holding one of the positions as three floats in a row (within 1 cm).
fn find_positions(regions: &[Region], cands: &[[f32; 3]], own: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let lo = cands.iter().map(|p| p[0]).fold(f32::MAX, f32::min) - 0.01;
    let hi = cands.iter().map(|p| p[0]).fold(f32::MIN, f32::max) + 0.01;
    let mut out = Vec::new();
    let mut own = own.to_vec();
    own.push((cands.as_ptr() as usize, std::mem::size_of_val(cands)));
    scan(regions, &own, |base, b, own| {
        let end = b.len().saturating_sub(12).min(CHUNK);
        let mut i = 0;
        while i < end {
            let x = f32_at(b, i);
            if x >= lo && x <= hi {
                for (k, p) in cands.iter().enumerate() {
                    if (x - p[0]).abs() < 0.01 && (f32_at(b, i + 4) - p[1]).abs() < 0.01 && (f32_at(b, i + 8) - p[2]).abs() < 0.01 {
                        if out.len() < 4096 && !mine(own, base + i) {
                            out.push((base + i, k));
                        }
                        break;
                    }
                }
            }
            i += 4;
        }
    });
    out
}

// ---- the search ----------------------------------------------------------------------------------------------------

fn beep(n: u32, ms: u32) {
    for _ in 0..n {
        unsafe {
            let _ = Beep(880, ms);
        }
        std::thread::sleep(Duration::from_millis(120));
    }
}

/// Ctrl+F9: one search, on a thread of its own.
pub fn start() {
    if RUNNING.swap(true, Relaxed) {
        return;
    }
    let n = RUNS.fetch_add(1, Relaxed) + 1;
    if n > MAX_RUNS {
        plog!("camsearch", "already searched {MAX_RUNS} times this session; restart Ragnarök for more");
        return;
    }
    std::thread::spawn(move || {
        if std::panic::catch_unwind(|| run(n)).is_err() {
            plog!("camsearch", "#{n} PANIC; stopped");
            beep(3, 600);
        }
        RUNNING.store(false, Relaxed);
    });
}

fn run(n: u32) {
    let t0 = Instant::now();
    let (w, h) = (SWAP_W.load(Relaxed).max(1), SWAP_H.load(Relaxed).max(1));
    let aspect = w as f32 / h as f32;
    beep(1, 250);
    plog!("camsearch", "#{n} started (screen {w}x{h}); hold still");
    let (regs, total) = regions();
    plog!("camsearch", "#{n} {} regions, {} MB of read-write memory", regs.len(), total >> 20);

    let projs = find_projections(&regs, aspect);
    plog!("camsearch", "#{n} {} projections with the screen's shape ({:.1} s)", projs.len(), t0.elapsed().as_secs_f64());
    let mut groups: Vec<(f32, f32, f32, f32, bool, usize)> = Vec::new();
    for p in &projs {
        let key = (p.sx, p.sy, p.a, p.b, p.transposed);
        match groups.iter_mut().find(|g| (g.0, g.1, g.2, g.3, g.4) == key) {
            Some(g) => g.5 += 1,
            None => groups.push((p.sx, p.sy, p.a, p.b, p.transposed, 1)),
        }
    }
    for g in groups.iter().take(16) {
        let fov = 2.0 * (1.0 / g.1).atan().to_degrees();
        plog!("camsearch", "#{n}   x{}: fov {fov:.2} deg, sx {:.5} sy {:.5}, A {:.6} B {:.6}, {}", g.5, g.0, g.1, g.2, g.3, if g.4 { "transposed" } else { "row" });
    }
    for p in projs.iter().take(40) {
        plog!(
            "camsearch",
            "#{n}   {:#x} ({}) {} sx {:.5} sy {:.5} A {:.6} B {:.6} w {:+} jitter ({:.6}, {:.6})",
            p.addr, place(p.addr), if p.transposed { "T" } else { "R" }, p.sx, p.sy, p.a, p.b, p.w, p.jx, p.jy
        );
    }
    if projs.is_empty() {
        plog!("camsearch", "#{n} nothing found; stopped");
        beep(3, 600);
        return;
    }

    // windows around the projections, merged where they overlap
    let mut starts: Vec<usize> = Vec::new();
    for p in &projs {
        let s = (p.addr.saturating_sub(PROJ_WINDOW / 2)) & !0xf;
        if !starts.iter().any(|x| s < x + PROJ_WINDOW && *x < s + PROJ_WINDOW) {
            starts.push(s);
        }
        if starts.len() >= MAX_PROJ {
            break;
        }
    }
    let mut rec = match Recorder::new(&format!("camsearch_{n}_proj.bin"), 1, starts, PROJ_WINDOW, t0) {
        Ok(r) => r,
        Err(e) => return plog!("camsearch", "#{n} cannot write the recording: {e}"),
    };
    beep(2, 200);
    plog!("camsearch", "#{n} recording {} windows; move the camera now", rec.starts.len());
    let snaps = rec.record(20);
    plog!("camsearch", "#{n} {snaps} snapshots recorded");

    beep(1, 900);
    plog!("camsearch", "#{n} hold still");
    std::thread::sleep(Duration::from_millis(1500));
    let turns = turned(&rec);
    plog!("camsearch", "#{n} {} rotation blocks turned during the recording", turns.len());
    let mut cands = Vec::new();
    let mut buf = vec![0u8; PROJ_WINDOW];
    for t in &turns {
        let at = rec.starts[t.window] + t.offset;
        if read(rec.starts[t.window], &mut buf) {
            let before = cands.len();
            positions(&buf, t, &mut cands);
            plog!(
                "camsearch",
                "#{n}   rotation at {at:#x} (rows {} bytes apart) turned {:.0} deg; positions: {}",
                t.stride,
                t.degrees,
                cands[before..].iter().map(|p| format!("({:.3}, {:.3}, {:.3})", p[0], p[1], p[2])).collect::<Vec<_>>().join(" ")
            );
        }
        if cands.len() >= 24 {
            break;
        }
    }
    if cands.is_empty() {
        plog!("camsearch", "#{n} no turning rotation near the projections; the recording still says a lot. Done");
        beep(3, 250);
        return;
    }
    let ts = Instant::now();
    let mut own: Vec<(usize, usize)> = vec![(buf.as_ptr() as usize, buf.len()), (rec.buf.as_ptr() as usize, rec.buf.len())];
    for v in rec.first.iter().chain(&rec.last) {
        own.push((v.as_ptr() as usize, v.len()));
    }
    let hits = find_positions(&regs, &cands, &own);
    plog!("camsearch", "#{n} {} places hold one of the {} positions ({:.1} s)", hits.len(), cands.len(), ts.elapsed().as_secs_f64());
    for (k, p) in cands.iter().enumerate() {
        let at: Vec<String> = hits.iter().filter(|h| h.1 == k).take(12).map(|h| format!("{:#x} ({})", h.0, place(h.0))).collect();
        let count = hits.iter().filter(|h| h.1 == k).count();
        plog!("camsearch", "#{n}   ({:.3}, {:.3}, {:.3}): {count} places{}{}", p[0], p[1], p[2], if count > 0 { ": " } else { "" }, at.join(", "));
    }

    let mut pos_starts: Vec<usize> = Vec::new();
    for (a, _) in &hits {
        let s = a.saturating_sub(POS_WINDOW / 2) & !0xf;
        if !pos_starts.iter().any(|x| s < x + POS_WINDOW && *x < s + POS_WINDOW) {
            pos_starts.push(s);
        }
        if pos_starts.len() >= MAX_POS {
            break;
        }
    }
    if !pos_starts.is_empty() {
        if let Ok(mut rec2) = Recorder::new(&format!("camsearch_{n}_pos.bin"), 2, pos_starts, POS_WINDOW, t0) {
            beep(2, 200);
            plog!("camsearch", "#{n} recording {} windows around the positions; move the camera again", rec2.starts.len());
            let snaps = rec2.record(10);
            plog!("camsearch", "#{n} {snaps} snapshots recorded");
        }
    }
    plog!("camsearch", "#{n} done in {:.0} s", t0.elapsed().as_secs_f64());
    beep(3, 250);
}

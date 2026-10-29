//! Stage 2b of live Kratos: move Ragnarök's camera. 0.3.0's run (MODLOG) found what the per-view camera blocks are
//! built from: a camera kept in GoWR.exe's own data at +0x39c7110 (this build, PE time stamp 0x6859ad47), rows of
//! 16 bytes from +0xc0: the camera's x, y and z axes, then its position at +0xf0. The builder that reads it runs
//! at +0x4e7516 (inside a function whose start Windows' unwind table gives).
//!
//! Ctrl+F9 with Ragnarök in front, guided by beeps:
//! 1. one beep, hold still: the camera kept there is logged (and 1 KB around it, in hex), with every copy of its
//!    position or of the view translation (-axis·position, both orders) in that KB; for a second "who writes here"
//!    on its position (`watch`); then for 6 s an execution trap at the builder function's start raises the camera
//!    2 m (position and every matching view translation) each time the builder is about to run. The real values go
//!    back at every Present (`restore`), so the game's own camera logic never sees the raised one; if the real
//!    camera jumps more than 1.5 m between two runs, the test stops. Screenshots at 2 s and 4 s, and 1 s after.
//! 2. two beeps: move the camera (10 s); the KB is recorded 5 times a second (`camsearch_<n>_global.bin`).
//! 3. three beeps: done.
//! Another build of Ragnarök (other time stamp) or an unexpected layout: 0.3.0's search runs instead.

use crate::camsearch::{beep, place, Recorder};
use crate::plog;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};
use windows::Win32::System::Diagnostics::Debug::RtlLookupFunctionEntry;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

const STAMP: u32 = 0x6859ad47;
const GLOBAL_RVA: usize = 0x39c7110;
const BUILDER_RVA: usize = 0x4e7516;
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

/// The builder is about to run: raise the camera (unless it is still raised from this frame).
fn on_builder() {
    let g = GLOBAL.load(Relaxed);
    if g == 0 || !ACTIVE.load(Relaxed) {
        return;
    }
    unsafe {
        let (r, p) = camera(g);
        let bits = p.map(f32::to_bits);
        if HAVE_WRITTEN.load(Relaxed) && (0..3).all(|i| WRITTEN[i].load(Relaxed) == bits[i]) {
            SKIPPED.fetch_add(1, Relaxed);
            return;
        }
        if HAVE_REAL.load(Relaxed) {
            let last = [0, 1, 2].map(|i| f32::from_bits(LAST_REAL[i].load(Relaxed)));
            let d = ((p[0] - last[0]).powi(2) + (p[1] - last[1]).powi(2) + (p[2] - last[2]).powi(2)).sqrt();
            if d > 1.5 {
                JUMPS.fetch_add(1, Relaxed);
                ACTIVE.store(false, Relaxed);
                return;
            }
        }
        for i in 0..3 {
            LAST_REAL[i].store(bits[i], Relaxed);
        }
        HAVE_REAL.store(true, Relaxed);
        let raised = [p[0], p[1] + RAISE_M, p[2]];
        let n = N_MATCHES.load(Relaxed);
        let ms = &*MATCHES.0.get();
        let real = &mut *REAL.0.get();
        for (k, m) in ms[..n].iter().enumerate() {
            let a = (g as isize + m.off) as usize;
            real[k] = [rd(a), rd(a + m.stride), rd(a + 2 * m.stride)];
            let v = value(m.what, &r, raised);
            for c in 0..3 {
                wr(a + c * m.stride, v[c]);
            }
        }
        for i in 0..3 {
            WRITTEN[i].store(raised[i].to_bits(), Relaxed);
        }
        HAVE_WRITTEN.store(true, Relaxed);
        APPLIED.fetch_add(1, Relaxed);
    }
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

/// The start of the function holding `pc`, from the unwind table (following chained entries).
fn function_start(pc: usize) -> Option<(usize, usize)> {
    unsafe {
        let mut image = 0u64;
        let f = RtlLookupFunctionEntry(pc as u64, &mut image, None);
        if f.is_null() {
            return None;
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
    // testing aid (tools/fakes): GOWR_PROBE_STEER=<stamp>,<camera rva>,<builder rva> in hex, for the stand-in
    let (want_stamp, global_rva, builder_rva) = std::env::var("GOWR_PROBE_STEER")
        .ok()
        .and_then(|v| {
            let x: Vec<usize> = v.split(',').filter_map(|t| usize::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok()).collect();
            (x.len() == 3).then(|| (x[0] as u32, x[1], x[2]))
        })
        .unwrap_or((STAMP, GLOBAL_RVA, BUILDER_RVA));
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
    if !found.iter().any(|m| m.what == 0 && m.off == 0xf0) {
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

    crate::watch::watch(n, &[("camera position x".into(), g + 0xf0), ("camera position y".into(), g + 0xf4)], 1000, place);

    match function_start(base + builder_rva) {
        None => plog!("steer", "#{n} no unwind entry for the builder at +{builder_rva:#x}; not raising the camera"),
        Some((fstart, fend)) => {
            plog!(
                "steer",
                "#{n} builder function GoWR.exe+{:#x}..+{:#x}; raising the camera {RAISE_M} m for 6 s",
                fstart - base,
                fend - base
            );
            for c in [&APPLIED, &SKIPPED, &RESTORED, &JUMPS] {
                c.store(0, Relaxed);
            }
            HAVE_REAL.store(false, Relaxed);
            HAVE_WRITTEN.store(false, Relaxed);
            GLOBAL.store(g, Relaxed);
            ACTIVE.store(true, Relaxed);
            let shots = std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(2));
                crate::monitor::want_shot("raised-2m-a");
                std::thread::sleep(Duration::from_secs(2));
                crate::monitor::want_shot("raised-2m-b");
            });
            let runs = crate::watch::on_entry(n, fstart, 6000, on_builder);
            ACTIVE.store(false, Relaxed);
            let _ = shots.join();
            std::thread::sleep(Duration::from_millis(100));
            restore();
            GLOBAL.store(0, Relaxed);
            plog!(
                "steer",
                "#{n} builder ran {runs} times: camera raised {}, already raised {}, put back {} times, stopped by a jump {} times",
                APPLIED.load(Relaxed),
                SKIPPED.load(Relaxed),
                RESTORED.load(Relaxed),
                JUMPS.load(Relaxed)
            );
            std::thread::sleep(Duration::from_secs(1));
            crate::monitor::want_shot("after-raise");
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    if let Ok(mut rec) = Recorder::new(&format!("camsearch_{n}_global.bin"), 2, vec![start], WIN_LEN, t0) {
        beep(2, 200);
        plog!("steer", "#{n} recording the camera's KB; move the camera now");
        let snaps = rec.record(10);
        plog!("steer", "#{n} {snaps} snapshots recorded");
    }
    plog!("steer", "#{n} done in {:.0} s", t0.elapsed().as_secs_f64());
    beep(3, 250);
}

//! Where Elden Ring keeps the player's animated pose: step 2 of giving Kratos his own movement. The mod can already
//! put a per-bone offset on the game's skinning (the offset arrays the cape writes: final = game skinning x offset,
//! in rest space, per bone, see `bonepose`), so to show Kratos's decoded Ragnarök motion it has to know the game's
//! skinning first: offset = game skinning^-1 x Kratos's skinning. 0.1.68's probe looked only at the start of each
//! object it reached (an array within 64 bytes of where a pointer led) and found the pose nowhere in the skeleton's
//! order: the arrays that moved matched none ("likely another bone order").
//!
//! This probe scans the whole of each object it reaches (8 KB, every 16 bytes) for runs of at least 100 bone
//! transforms (4x4, 3x4, 4x3 matrices, hkQsTransforms), four pointers deep from more roots (the character, its model
//! and model item, the bone-matrix entity, the matrices exporter, the behaviour module, its hkbCharacter and setup,
//! the pose importer and the anim-skeleton-to-model modifier). Then it reads every array found 6 times, 200 ms
//! apart, while the player moves: what changes is animated. The log gives each array's size, how much moved and how
//! it reads against the Havok skeleton in its order (local, model space, skinning) and in any order (translations
//! that equal a bone's rest offset); the snapshots go to `%LOCALAPPDATA%\GowRing\er_pose_find_<n>.bin` (all six
//! for arrays that moved, the first for the rest) for working out the bone order offline. Read-only (`ReadProcessMemory` on our own process), on a thread of its own.
//! It runs by itself 30, 90 and 150 s into the world, and on the debug pose key.

use std::collections::HashSet;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::pose::Layout;
use crate::skeleton::{read_vec, reads, Bone};

/// Bytes of each object scanned for arrays, and read for pointers to follow.
const WINDOW: usize = 0x2000;
const POINTERS: usize = 0x800;
const DEPTH: usize = 4;
const BUDGET: u64 = 300_000;
const MAX_ARRAYS: usize = 64;
/// Entries read per array at most (the player skeleton has 150 bones; the model's bone list may be longer).
const MAX_ENTRIES: usize = 512;
const MIN_RUN: usize = 100;
const SNAPSHOTS: usize = 6;
const SNAPSHOT_GAP: Duration = Duration::from_millis(200);
/// Seconds into the world at which a round runs by itself.
pub const AT_S: [f32; 3] = [30.0, 90.0, 150.0];

const LAYOUTS: [Layout; 4] = [Layout::M44, Layout::M34, Layout::M43, Layout::Qs];

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Rounds started this session (the next round's number), and how many of `AT_S` have been passed.
static ROUNDS: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);

struct Array {
    path: String,
    address: usize,
    layout: Layout,
    count: usize,
}

/// Called every frame with the time in the world: starts a round when one of `AT_S` is passed (`roots` is only
/// called then).
pub fn tick(in_world_s: f32, roots: impl FnOnce() -> Vec<(&'static str, usize)>) {
    let passed = AT_S.iter().filter(|t| in_world_s >= **t).count();
    if passed > PASSED.load(Ordering::Relaxed) && !RUNNING.load(Ordering::SeqCst) {
        PASSED.store(passed, Ordering::Relaxed);
        start(roots(), "timed");
    }
}

/// Start a round now from `roots` (name, address), unless one is running.
pub fn start(roots: Vec<(&'static str, usize)>, why: &str) {
    if roots.is_empty() {
        crate::glog!("posefind", "pose search ({why}): no roots (no player model)");
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        crate::overlay::toast("Pose search already running (debug)");
        return;
    }
    let round = ROUNDS.fetch_add(1, Ordering::SeqCst) + 1;
    let why = why.to_string();
    let spawned = std::thread::Builder::new().name("gowring-pose-find".into()).spawn(move || {
        if std::panic::catch_unwind(|| run(round, &roots, &why)).is_err() {
            crate::glog!("posefind", "pose search {round} panicked");
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::SeqCst);
        crate::glog!("posefind", "could not start the pose search thread: {e}");
    }
}

/// The longest run of valid transforms of any layout starting at float `i` of `f`: (layout, entries).
fn run_at(f: &[f32], i: usize) -> Option<(Layout, usize)> {
    LAYOUTS
        .iter()
        .map(|l| (*l, f[i..].chunks_exact(l.stride()).take_while(|e| l.valid(e)).count()))
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 0)
}

/// Runs of at least `MIN_RUN` transforms anywhere in the object at `at` (its first `WINDOW` bytes in `window`).
fn scan(at: usize, window: &[f32], path: &str, seen: &mut HashSet<usize>, out: &mut Vec<Array>) {
    let mut i = 0;
    while i + 16 <= window.len() && out.len() < MAX_ARRAYS {
        let Some((layout, n)) = run_at(window, i) else {
            i += 4;
            continue;
        };
        // a long run, or one that may go on past the window: read it whole from where it starts
        let start = at + i * 4;
        let to_the_end = i + (n + 1) * layout.stride() > window.len();
        if !(n >= MIN_RUN || to_the_end && n >= 4) || seen.contains(&start) {
            i += 4;
            continue;
        }
        let Some(full) = read_vec::<f32>(start, MAX_ENTRIES * 16) else {
            i += 4;
            continue;
        };
        match run_at(&full, 0) {
            Some((layout, count)) if count >= MIN_RUN => {
                seen.insert(start);
                out.push(Array { path: format!("{path}@{:#x}", i * 4), address: start, layout, count: count.min(MAX_ENTRIES) });
                i += count * layout.stride();
            }
            _ => i += 4,
        }
    }
}

fn search(roots: &[(&'static str, usize)]) -> (Vec<Array>, usize) {
    let r0 = reads();
    let mut visited: HashSet<usize> = HashSet::new();
    let mut seen_arrays: HashSet<usize> = HashSet::new();
    let mut found: Vec<Array> = Vec::new();
    let pointer = |w: usize| (0x10000..0x7fff_ffff_0000).contains(&w) && w % 8 == 0;
    let mut level: Vec<(String, usize)> = roots.iter().map(|(n, a)| (n.to_string(), *a)).collect();
    for depth in 0..=DEPTH {
        let mut next = Vec::new();
        for (path, at) in level {
            if reads() - r0 > BUDGET || found.len() >= MAX_ARRAYS {
                break;
            }
            if !visited.insert(at) {
                continue;
            }
            let Some(window) = read_vec::<f32>(at, WINDOW / 4) else { continue };
            scan(at, &window, &path, &mut seen_arrays, &mut found);
            if depth == DEPTH {
                continue;
            }
            for (k, pair) in window[..POINTERS / 4].chunks_exact(2).enumerate() {
                let w = (pair[0].to_bits() as usize) | ((pair[1].to_bits() as usize) << 32);
                if pointer(w) && !visited.contains(&w) {
                    next.push((format!("{path}+{:#x}", k * 8), w));
                }
            }
        }
        level = next;
    }
    (found, visited.len())
}

fn translation(layout: Layout, e: &[f32]) -> [f32; 3] {
    match layout {
        Layout::M34 => [e[3], e[7], e[11]],
        Layout::M43 => [e[9], e[10], e[11]],
        Layout::M44 => [e[12], e[13], e[14]],
        Layout::Qs => [e[0], e[1], e[2]],
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// How a snapshot reads against the skeleton: links at rest as local transforms in its order, as model-space ones in
/// its order, entries (any order) whose translation is some bone's rest offset (bones offset 2 cm or more), and the
/// skinning match in its order (3x4 only).
fn scores(a: &Array, f: &[f32], bones: &[Bone]) -> (usize, usize, usize, Option<(usize, usize)>) {
    let stride = a.layout.stride();
    let entry = |i: usize| &f[i * stride..i * stride + stride];
    let n = a.count.min(f.len() / stride);
    let (mut local, mut model) = (0, 0);
    for (i, b) in bones.iter().enumerate().take(n) {
        let Some(p) = usize::try_from(b.parent).ok().filter(|p| *p < i) else { continue };
        let t = translation(a.layout, entry(i));
        if dist(t, b.t) < 0.01 {
            local += 1;
        }
        let rest = dist(b.t, [0.0; 3]);
        if rest > 0.02 && (dist(t, translation(a.layout, entry(p))) - rest).abs() < 0.01 {
            model += 1;
        }
    }
    let offsets: Vec<[f32; 3]> = bones.iter().map(|b| b.t).filter(|t| dist(*t, [0.0; 3]) >= 0.02).collect();
    let any = (0..n).filter(|i| offsets.iter().any(|t| dist(translation(a.layout, entry(*i)), *t) < 0.002)).count();
    let skin = (a.layout == Layout::M34).then(|| crate::bonepose::skin_match(&f[..n * stride], bones)).flatten();
    (local, model, any, skin)
}

fn run(round: usize, roots: &[(&'static str, usize)], why: &str) {
    let t0 = Instant::now();
    crate::overlay::toast(&format!("Pose search {round}: walk or run for a few seconds (debug)"));
    let (found, looked) = search(roots);
    let searched_ms = t0.elapsed().as_millis();
    // the snapshots, while he moves
    let mut snaps: Vec<Vec<Vec<f32>>> = found.iter().map(|_| Vec::new()).collect();
    for k in 0..SNAPSHOTS {
        if k > 0 {
            std::thread::sleep(SNAPSHOT_GAP);
        }
        for (a, s) in found.iter().zip(snaps.iter_mut()) {
            if let Some(f) = read_vec::<f32>(a.address, a.count * a.layout.stride()) {
                s.push(f);
            }
        }
    }
    let bones = crate::skeleton::bones();
    let mut lines = Vec::new();
    let mut moving_total = 0;
    let mut moved_flags = Vec::new();
    for (a, s) in found.iter().zip(&snaps) {
        let stride = a.layout.stride();
        let moved = (0..a.count)
            .filter(|i| s.iter().skip(1).any(|f| f[i * stride..i * stride + stride].iter().zip(&s[0][i * stride..i * stride + stride]).any(|(x, y)| (x - y).abs() > 1e-4)))
            .count();
        if moved > 0 {
            moving_total += 1;
        }
        moved_flags.push(moved > 0);
        let sc = match (bones, s.first()) {
            (Some(b), Some(f)) => {
                let (local, model, any, skin) = scores(a, f, b);
                format!("in skeleton order: local {local}, model {model}, skinning {skin:?}; any order: {any} rest offsets")
            }
            _ => "skeleton not read yet".to_string(),
        };
        lines.push(format!("{} at {:#x}: {} x {}, {} moved over {} snapshots; {sc}", a.path, a.address, a.count, a.layout.name(), moved, s.len()));
    }
    let file = write_dump(round, &found, &snaps, &moved_flags);
    crate::glog!(
        "posefind",
        "pose search {round} ({why}): {} array(s), {moving_total} moving, {looked} objects, {} reads, search {searched_ms} ms, all {} ms; roots {:?}; {}\n    {}",
        found.len(),
        reads(),
        t0.elapsed().as_millis(),
        roots.iter().map(|(n, a)| format!("{n} {a:#x}")).collect::<Vec<_>>(),
        file,
        lines.join("\n    ")
    );
    crate::overlay::toast(&format!("Pose search {round} done: {} arrays, {moving_total} moving (debug)", found.len()));
}

/// The snapshots, for offline work: "GRPF", u32 version 1, u32 arrays, u32 snapshots; per array u32 path length,
/// the path, u64 address, u32 layout (0 4x4, 1 3x4, 2 4x3, 3 hkQsTransform), u32 entries, u32 floats per entry, u32
/// snapshots kept (all for an array that moved, else the first), then the floats.
fn write_dump(round: usize, found: &[Array], snaps: &[Vec<Vec<f32>>], moved: &[bool]) -> String {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"GRPF");
    for v in [1u32, found.len() as u32, SNAPSHOTS as u32] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for ((a, s), m) in found.iter().zip(snaps).zip(moved) {
        let s = &s[..if *m { s.len() } else { s.len().min(1) }];
        b.extend_from_slice(&(a.path.len() as u32).to_le_bytes());
        b.extend_from_slice(a.path.as_bytes());
        b.extend_from_slice(&(a.address as u64).to_le_bytes());
        let layout = LAYOUTS.iter().position(|l| *l == a.layout).unwrap_or(0) as u32;
        for v in [layout, a.count as u32, a.layout.stride() as u32, s.len() as u32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for f in s {
            for x in f {
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    let dir = &crate::paths::get().local;
    let path = dir.join(format!("er_pose_find_{round}.bin"));
    let wrote = std::fs::create_dir_all(dir).and_then(|_| std::fs::File::create(&path)).and_then(|mut f| f.write_all(&b));
    match wrote {
        Ok(()) => format!("snapshots written to {} ({} KB)", path.display(), b.len() / 1024),
        Err(e) => format!("could not write the snapshots: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_found_anywhere_in_a_window_and_read_whole() {
        // 3x4 identities from float 20 (80 bytes in), 120 of them
        let mut f = vec![0.5f32; 20];
        for _ in 0..120 {
            f.extend_from_slice(&[1.0, 0.0, 0.0, 0.1, 0.0, 1.0, 0.0, 0.2, 0.0, 0.0, 1.0, 0.3]);
        }
        f.extend(vec![7.0; 64]);
        let (layout, n) = run_at(&f, 20).unwrap();
        assert_eq!(n, 120);
        assert!(matches!(layout, Layout::M34 | Layout::M44 | Layout::M43));
        assert!(run_at(&f, 0).is_none());
    }

    #[test]
    fn translations_by_layout() {
        let e: Vec<f32> = (0..16).map(|x| x as f32).collect();
        assert_eq!(translation(Layout::M34, &e), [3.0, 7.0, 11.0]);
        assert_eq!(translation(Layout::M43, &e), [9.0, 10.0, 11.0]);
        assert_eq!(translation(Layout::M44, &e), [12.0, 13.0, 14.0]);
        assert_eq!(translation(Layout::Qs, &e), [0.0, 1.0, 2.0]);
    }
}

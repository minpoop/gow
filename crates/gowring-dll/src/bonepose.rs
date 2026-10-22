//! Moving the player's bones from the mod, the way to a cape that flows: Elden Ring's own capes run on Havok cloth data
//! inside each armour file, which the mod cannot make, so a cape of Kratos's would hang from spare bones (the skeleton's
//! two chains of ten `Xtra_Multipurpose_Bone`s) that the mod moves itself.
//!
//! The player's pose captures (0.1.54) held two arrays, reached from the model item's `mtx43_array_entity` (+0x30, then
//! +0x508 or +0x510), whose first 150 entries are row-major 3x4 skinning matrices (a bone's model-space transform times
//! the inverse of its rest one) in the Havok skeleton's order: read that way, each of the 149 children sits at its rest
//! offset from its parent. Whether the game draws from them, and when in the frame a write survives, is what the probe
//! here finds out. On the debug pose key it finds the arrays again and checks them the same way, then for 3 s per step
//! lifts the head's matrix 30 cm through each array that checks out, once after the physics step and once just before
//! drawing, naming each step on screen and in the log. A write goes through `WriteProcessMemory` (a bad address fails
//! cleanly), only to an entry that still reads as a rotation, and never adds up: a value still at our last write is
//! left as it is, and put back as it was when its step ends.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::pose::{Found, Layout};
use crate::skeleton::{read, Bone};

/// Where in the frame a step writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// `ChrIns_PostPhysicsSafe`: after the characters' physics and cloth.
    PostPhysics,
    /// `Draw_Pre`: the last step before the frame is drawn.
    DrawPre,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Stage::PostPhysics => "after the physics step",
            Stage::DrawPre => "just before drawing",
        }
    }
}

/// An array that reads as skinning matrices in the skeleton's order.
#[derive(Clone)]
pub struct Checked {
    pub path: String,
    pub address: usize,
    /// Children found at their rest offset from their parent, of all parent-child links.
    pub matched: usize,
    pub links: usize,
}

/// How far a step lifts the head (m, up in model space).
const LIFT: f32 = 0.3;
/// How long a step lifts, and the pause after it (s).
const STEP_S: f32 = 3.0;
const GAP_S: f32 = 1.0;
/// The first step starts this long after the arrays are found (s), for the toast to be read.
const LEAD_S: f32 = 2.0;
/// At most this many arrays are tried.
const MAX_ARRAYS: usize = 4;
/// The probe's search reaches further than the pose capture's (4,000 reads, 8 arrays): it runs on its own thread when
/// asked, and 0.1.66's captures no longer found the arrays under the bone-matrix entity.
const SEARCH_READS: u64 = 40_000;
const SEARCH_ARRAYS: usize = 32;
/// The share of parent-child links that must sit at their rest offset for an array to count.
const MIN_MATCH: f32 = 0.9;
/// A 3x4 entry is a bone's when each row of its 3x3 part is this close to unit length (rotation, scale about 1).
const ROW_SLACK: f32 = 0.2;

#[derive(Default, Clone, Copy)]
struct Counts {
    /// Frames the head was lifted, writes that failed, frames the value was still ours when the frame came round again
    /// (the game had not written it since), and writes after the physics step found before drawing as written / not.
    written: u32,
    failed: u32,
    stale: u32,
    kept: u32,
    lost: u32,
}

struct Probe {
    started: Instant,
    arrays: Vec<Checked>,
    head: usize,
    shown: Option<usize>,
    counts: Vec<Counts>,
    /// Our last write: the entry's address, the lifted y and the y it had before.
    last: Option<(usize, f32, f32)>,
    /// A write after the physics step not yet looked at again before drawing.
    unchecked: bool,
}

/// Put back the y our last write replaced, if the entry still holds what we wrote (a lift must not outlast its step).
fn undo(p: &mut Probe) {
    if let Some((at, lifted, before)) = p.last.take() {
        if read::<f32>(at + 7 * 4) == Some(lifted) {
            write_f32(at + 7 * 4, before);
        }
    }
    p.unchecked = false;
}

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
static SEARCHING: AtomicBool = AtomicBool::new(false);

fn steps(p: &Probe) -> usize {
    p.arrays.len() * 2
}

fn step_of(k: usize) -> (usize, Stage) {
    (k / 2, if k % 2 == 0 { Stage::PostPhysics } else { Stage::DrawPre })
}

/// Rest (bind) transforms in model space, 4x4 row-major, from the skeleton's parent-relative ones.
fn bind_pose(bones: &[Bone]) -> Vec<[[f64; 4]; 4]> {
    let mut out: Vec<[[f64; 4]; 4]> = Vec::with_capacity(bones.len());
    for b in bones {
        let [x, y, z, w] = b.r.map(|v| v as f64);
        let s = b.s.map(|v| v as f64);
        let r = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        let mut local = [[0.0; 4]; 4];
        for i in 0..3 {
            for j in 0..3 {
                local[i][j] = r[i][j] * s[j];
            }
            local[i][3] = b.t[i] as f64;
        }
        local[3][3] = 1.0;
        let m = match usize::try_from(b.parent).ok().and_then(|p| out.get(p)) {
            Some(parent) => mul(parent, &local),
            None => local,
        };
        out.push(m);
    }
    out
}

fn mul(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// The inverse of an affine 4x4 (3x3 by its adjugate), if its 3x3 part is not singular.
fn inverse(m: &[[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let a = |i: usize, j: usize| m[i][j];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0)) + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-9 {
        return None;
    }
    let mut inv = [[0.0; 4]; 4];
    inv[0][0] = (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) / det;
    inv[0][1] = (a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2)) / det;
    inv[0][2] = (a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1)) / det;
    inv[1][0] = (a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2)) / det;
    inv[1][1] = (a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0)) / det;
    inv[1][2] = (a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2)) / det;
    inv[2][0] = (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0)) / det;
    inv[2][1] = (a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1)) / det;
    inv[2][2] = (a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)) / det;
    for i in 0..3 {
        inv[i][3] = -(0..3).map(|k| inv[i][k] * m[k][3]).sum::<f64>();
    }
    inv[3][3] = 1.0;
    Some(inv)
}

/// How many of the skeleton's parent-child links sit at their rest offset when `floats` (row-major 3x4) are read as
/// skinning matrices in the skeleton's order: (matched, links). `None` when the array is shorter than the skeleton.
pub fn skin_match(floats: &[f32], bones: &[Bone]) -> Option<(usize, usize)> {
    if floats.len() < bones.len() * 12 || bones.is_empty() {
        return None;
    }
    let bind = bind_pose(bones);
    let model: Vec<[[f64; 4]; 4]> = (0..bones.len())
        .map(|i| {
            let f = &floats[i * 12..i * 12 + 12];
            let mut s = [[0.0; 4]; 4];
            for r in 0..3 {
                for c in 0..4 {
                    s[r][c] = f[r * 4 + c] as f64;
                }
            }
            s[3][3] = 1.0;
            mul(&s, &bind[i])
        })
        .collect();
    let (mut matched, mut links) = (0, 0);
    for (i, b) in bones.iter().enumerate() {
        let Some(p) = usize::try_from(b.parent).ok().filter(|p| *p < i) else { continue };
        let Some(inv) = inverse(&model[p]) else { continue };
        links += 1;
        let local = mul(&inv, &model[i]);
        let d: f64 = (0..3).map(|k| (local[k][3] - b.t[k] as f64).powi(2)).sum::<f64>().sqrt();
        if d < 0.01 {
            matched += 1;
        }
    }
    Some((matched, links))
}

/// The found arrays that read as the skeleton's skinning matrices, best first, at most [`MAX_ARRAYS`].
pub fn check(found: &[Found], bones: &[Bone]) -> (Vec<Checked>, Vec<String>) {
    let mut lines = Vec::new();
    let mut ok = Vec::new();
    for f in found {
        let res = (f.layout == Layout::M34).then(|| skin_match(&f.floats, bones)).flatten();
        match res {
            Some((m, l)) => {
                lines.push(format!("{} at {:#x}: {m}/{l} links at rest offset", f.path, f.address));
                if l > 0 && m as f32 >= MIN_MATCH * l as f32 {
                    ok.push(Checked { path: f.path.clone(), address: f.address, matched: m, links: l });
                }
            }
            None => lines.push(format!("{} at {:#x}: not 3x4 or shorter than the skeleton", f.path, f.address)),
        }
    }
    ok.sort_by(|a, b| b.matched.cmp(&a.matched));
    ok.truncate(MAX_ARRAYS);
    (ok, lines)
}

/// The debug pose key: find and check the arrays on a thread of our own (from `roots`, as the pose capture does), then
/// run the steps from the game's tasks ([`on_stage`]).
pub fn start(roots: Vec<(&'static str, usize)>) {
    if PROBE.lock().map(|p| p.is_some()).unwrap_or(true) || SEARCHING.swap(true, Ordering::SeqCst) {
        crate::overlay::toast("Pose probe already running (debug)");
        return;
    }
    let spawned = std::thread::Builder::new().name("gowring-pose-probe".into()).spawn(move || {
        let r = std::panic::catch_unwind(|| {
            let Some(bones) = crate::skeleton::bones() else {
                crate::glog!("bonepose", "pose probe: the skeleton has not been read yet (5 s into the world)");
                crate::overlay::toast("Pose probe: skeleton not read yet, try again in a few seconds (debug)");
                return;
            };
            let (found, looked) = crate::pose::find(&roots, SEARCH_READS, SEARCH_ARRAYS);
            let (arrays, lines) = check(&found, bones);
            crate::glog!("bonepose", "pose probe: {} array(s) found ({looked} objects looked at): {}", found.len(), lines.join("; "));
            let head = bones.iter().position(|b| b.name == "Head");
            match (arrays.is_empty(), head) {
                (false, Some(head)) => {
                    let tried: Vec<String> = arrays.iter().enumerate().map(|(i, a)| format!("array {} = {} ({}/{})", i + 1, a.path, a.matched, a.links)).collect();
                    crate::glog!("bonepose", "pose probe: {} array(s) to try ({}), the head is bone {head}; steps of {STEP_S} s", arrays.len(), tried.join(", "));
                    crate::overlay::toast(&format!("Pose probe: {} steps, watch Kratos's head (debug)", arrays.len() * 2));
                    if let Ok(mut p) = PROBE.lock() {
                        let n = arrays.len() * 2;
                        *p = Some(Probe { started: Instant::now(), arrays, head, shown: None, counts: vec![Counts::default(); n], last: None, unchecked: false });
                    }
                }
                _ => {
                    crate::glog!("bonepose", "pose probe: nothing to try (head bone {head:?})");
                    crate::overlay::toast("Pose probe: no skinning matrices found (debug)");
                }
            }
        });
        if r.is_err() {
            crate::glog!("bonepose", "pose probe search panicked");
        }
        SEARCHING.store(false, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        SEARCHING.store(false, Ordering::SeqCst);
        crate::glog!("bonepose", "could not start the pose probe thread: {e}");
    }
}

fn write_f32(addr: usize, v: f32) -> bool {
    use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut done = 0usize;
    let ok = unsafe { WriteProcessMemory(GetCurrentProcess(), addr as *const _, &v as *const f32 as *const _, 4, Some(&mut done)) };
    ok.is_ok() && done == 4
}

/// Does this 3x4 read as a bone's (rows of its 3x3 near unit length, all finite)?
fn bone_like(m: &[f32; 12]) -> bool {
    m.iter().all(|x| x.is_finite()) && (0..3).all(|r| ((m[r * 4] * m[r * 4] + m[r * 4 + 1] * m[r * 4 + 1] + m[r * 4 + 2] * m[r * 4 + 2]).sqrt() - 1.0).abs() < ROW_SLACK)
}

/// Called from the game's tasks at each stage, every frame: runs the probe's step for this stage, if one is running.
pub fn on_stage(stage: Stage) {
    let Ok(mut guard) = PROBE.try_lock() else { return };
    let Some(p) = guard.as_mut() else { return };
    let t = p.started.elapsed().as_secs_f32() - LEAD_S;
    if t < 0.0 {
        return;
    }
    let k = (t / (STEP_S + GAP_S)) as usize;
    if k >= steps(p) {
        undo(p);
        let lines: Vec<String> = (0..steps(p))
            .map(|k| {
                let (a, s) = step_of(k);
                let c = p.counts[k];
                format!("step {} ({} {}): lifted {} frames, {} failed, {} already ours, {} kept and {} lost by drawing", k + 1, p.arrays[a].path, s.name(), c.written, c.failed, c.stale, c.kept, c.lost)
            })
            .collect();
        crate::glog!("bonepose", "pose probe done: {}", lines.join("; "));
        crate::overlay::toast("Pose probe done: which step lifted his head? (debug)");
        *guard = None;
        return;
    }
    let (a, step_stage) = step_of(k);
    let within = t - k as f32 * (STEP_S + GAP_S);
    let addr = p.arrays[a].address + p.head * 48;
    if p.shown != Some(k) {
        undo(p);
        p.shown = Some(k);
        let line = format!("Pose probe step {}/{}: head lifted through array {} {}", k + 1, steps(p), a + 1, step_stage.name());
        crate::glog!("bonepose", "{line} ({} at {:#x})", p.arrays[a].path, p.arrays[a].address);
        crate::overlay::toast(&format!("{line} (debug)"));
    }
    // a write after the physics step, looked at again just before drawing
    if stage == Stage::DrawPre && step_stage == Stage::PostPhysics && p.unchecked {
        p.unchecked = false;
        if let Some((at, y, _)) = p.last {
            match read::<f32>(at + 7 * 4) {
                Some(now) if now == y => p.counts[k].kept += 1,
                _ => p.counts[k].lost += 1,
            }
        }
    }
    if stage != step_stage {
        return;
    }
    if within > STEP_S {
        undo(p);
        return;
    }
    let Some(m) = read::<[f32; 12]>(addr).filter(bone_like) else {
        p.counts[k].failed += 1;
        return;
    };
    // the game has not written it since our last write: it is lifted already
    if p.last.is_some_and(|(at, y, _)| at == addr && m[7] == y) {
        p.counts[k].stale += 1;
        return;
    }
    let y = m[7] + LIFT;
    if write_f32(addr + 7 * 4, y) {
        p.counts[k].written += 1;
        p.last = Some((addr, y, m[7]));
        p.unchecked = stage == Stage::PostPhysics;
    } else {
        p.counts[k].failed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(name: &str, parent: i16, t: [f32; 3], r: [f32; 4]) -> Bone {
        Bone { name: name.into(), parent, t, r, s: [1.0; 3] }
    }

    #[test]
    fn skinning_matrices_in_the_skeletons_order_put_every_child_at_its_rest_offset() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let bones = vec![bone("Master", -1, [0.0; 3], [0.0, h, 0.0, h]), bone("Pelvis", 0, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]), bone("Spine", 1, [0.2, 0.0, 0.0], [0.0, 0.0, h, h]), bone("Head", 2, [0.5, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])];
        // at rest every skinning matrix is the identity
        let identity: Vec<f32> = (0..bones.len()).flat_map(|_| [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]).collect();
        assert_eq!(skin_match(&identity, &bones), Some((3, 3)));
        // the whole body moved (one matrix for all) still matches; the head alone lifted does not
        let moved: Vec<f32> = (0..bones.len()).flat_map(|_| [0.0, 0.0, 1.0, 2.0, 0.0, 1.0, 0.0, 0.5, -1.0, 0.0, 0.0, 3.0]).collect();
        assert_eq!(skin_match(&moved, &bones), Some((3, 3)));
        let mut lifted = identity.clone();
        lifted[3 * 12 + 7] += LIFT;
        assert_eq!(skin_match(&lifted, &bones), Some((2, 3)));
        // model-space transforms read as skinning ones do not match, nor does a short array
        assert!(skin_match(&identity[..24], &bones).is_none());
        let bind = bind_pose(&bones);
        let model: Vec<f32> = bind.iter().flat_map(|m| (0..3).flat_map(move |r| (0..4).map(move |c| m[r][c] as f32))).collect();
        assert!(skin_match(&model, &bones).unwrap().0 < 3);
    }

    #[test]
    fn the_steps_try_each_array_after_the_physics_step_then_before_drawing() {
        assert_eq!(step_of(0), (0, Stage::PostPhysics));
        assert_eq!(step_of(1), (0, Stage::DrawPre));
        assert_eq!(step_of(2), (1, Stage::PostPhysics));
        assert!(bone_like(&[1.0, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, -2.0]));
        assert!(!bone_like(&[0.0; 12]));
    }
}

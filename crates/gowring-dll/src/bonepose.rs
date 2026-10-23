//! Moving the player's bones from the mod, the way to a cape that flows: Elden Ring's own capes run on Havok cloth data
//! inside each armour file, which the mod cannot make, so a cape of Kratos's would hang from bones the mod moves itself.
//!
//! What 0.1.67's probe found: two arrays reached from the model item's `mtx43_array_entity` (+0x30, then +0x508 and
//! +0x510; [`OFFSETS`]) hold one row-major 3x4 per bone of the Havok skeleton, in its order, and the game fills them on
//! alternate frames; a 30 cm lift written into the head's entry, after the physics step or just before drawing, lifted
//! his head on screen. But in play they are all the identity (0.1.54's captures): they are offsets the game puts on top
//! of the animated pose, not the pose (the 149/149 "skinning matrix" match was the identity's), so the pose itself and
//! how the offsets combine with it are still to be found.
//!
//! This probe (the debug pose key) finds out. First a search from the model item, its bone-matrix entity, the
//! behaviour module, its hkbCharacter, and the character's pose importer and anim-skeleton-to-model modifier (ChrIns
//! +0x398, +0x3a8), four pointers deep: every array of bone transforms found is scored against the skeleton read as
//! local (parent-relative), model-space and skinning transforms in its order, and written to `er_pose_probe.json`.
//! Then three steps on screen, through the offset arrays just before drawing: the right hand turned 90 degrees about
//! the vertical, first with no shift (a turn about the origin of whatever space the offset is applied in), then about
//! the vertical through its rest position; then the left upper arm turned 35 degrees about its rest position. If the
//! first turns the hand in place, the offset is applied in the bone's own local frame (before the hierarchy); if the
//! second does, in rest (bind) space under the animation; if neither, in model space over it. Whether the fingers and
//! weapon (and the forearm) go with it or tear away says whether it carries to the bone's children. That is what the
//! cape needs.
//!
//! A write goes through `WriteProcessMemory` (a bad address fails cleanly), only to an entry that still reads as a
//! rotation, and the identity is put back when a step ends.

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

/// From the model item to the offset arrays: the bone-matrix entity, then +0x30, then each of these.
pub const OFFSETS: [usize; 2] = [0x508, 0x510];
const TO_LIST: usize = 0x30;
/// A 3x4 entry is a bone's when each row of its 3x3 part is this close to unit length (rotation, scale about 1).
const ROW_SLACK: f32 = 0.2;
/// The probe's search: further than the pose capture's (4,000 reads, 8 arrays, 2 pointers), on its own thread.
const SEARCH_READS: u64 = 200_000;
const SEARCH_ARRAYS: usize = 48;
const SEARCH_DEPTH: usize = 4;
/// The steps: bone, axis, angle (degrees), about its rest position (else about the origin); each held this long, with
/// a pause after it (s).
const STEPS: [(&str, [f32; 3], f32, bool); 3] = [("R_Hand", [0.0, 1.0, 0.0], 90.0, false), ("R_Hand", [0.0, 1.0, 0.0], 90.0, true), ("L_UpperArm", [1.0, 0.0, 0.0], 35.0, true)];
const STEP_S: f32 = 4.0;
const GAP_S: f32 = 2.0;
/// The first step starts this long after the search (s), for the toast to be read.
const LEAD_S: f32 = 2.0;

/// Rest (bind) transforms in model space, 4x4 row-major, from the skeleton's parent-relative ones.
pub(crate) fn bind_pose(bones: &[Bone]) -> Vec<[[f64; 4]; 4]> {
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
    skin_match_except(floats, bones, &[])
}

/// [`skin_match`] leaving out the links of the bones in `skip` (bones the mod writes itself, see `cape`).
pub fn skin_match_except(floats: &[f32], bones: &[Bone], skip: &[usize]) -> Option<(usize, usize)> {
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
        if skip.contains(&i) || skip.contains(&p) {
            continue;
        }
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

/// Write `v` at `addr` through `WriteProcessMemory` on our own process (fails cleanly on memory that is not there).
pub(crate) fn write_floats(addr: usize, v: &[f32]) -> bool {
    use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut done = 0usize;
    let ok = unsafe { WriteProcessMemory(GetCurrentProcess(), addr as *const _, v.as_ptr() as *const _, v.len() * 4, Some(&mut done)) };
    ok.is_ok() && done == v.len() * 4
}

/// Does this 3x4 read as a bone's (rows of its 3x3 near unit length, all finite)?
pub(crate) fn bone_like(m: &[f32; 12]) -> bool {
    m.iter().all(|x| x.is_finite()) && (0..3).all(|r| ((m[r * 4] * m[r * 4] + m[r * 4 + 1] * m[r * 4 + 1] + m[r * 4 + 2] * m[r * 4 + 2]).sqrt() - 1.0).abs() < ROW_SLACK)
}


/// The offset arrays' addresses from the model item (0 where a pointer does not read).
pub fn offset_arrays(model_item: usize) -> [usize; 2] {
    let list = read::<usize>(model_item + crate::pose::MTX43_ARRAY_ENTITY).and_then(|e| read::<usize>(e + TO_LIST));
    OFFSETS.map(|off| list.and_then(|l| read::<usize>(l + off)).unwrap_or(0))
}

/// Where an entry of `layout` keeps its translation, and whether it is the identity.
fn translation(layout: Layout, e: &[f32]) -> [f32; 3] {
    match layout {
        Layout::M34 => [e[3], e[7], e[11]],
        Layout::M43 => [e[9], e[10], e[11]],
        Layout::M44 => [e[12], e[13], e[14]],
        Layout::Qs => [e[0], e[1], e[2]],
    }
}

/// How an array reads against the skeleton in its order: (entries not the identity, links at their rest length as
/// local transforms, as model-space transforms, as skinning matrices).
fn scores(f: &Found, bones: &[Bone]) -> (usize, usize, usize, Option<(usize, usize)>) {
    let stride = match f.layout {
        Layout::M44 => 16,
        _ => 12,
    };
    let n = f.floats.len() / stride;
    let entry = |i: usize| &f.floats[i * stride..i * stride + stride];
    let identity = |e: &[f32]| match f.layout {
        Layout::Qs => e[..3].iter().all(|x| x.abs() < 1e-4) && e[4..7].iter().all(|x| x.abs() < 1e-4),
        _ => translation(f.layout, e).iter().all(|x| x.abs() < 1e-4) && (e[0] - 1.0).abs() < 1e-4 && (e[5] - 1.0).abs() < 1e-4,
    };
    let moving = (0..n).filter(|i| !identity(entry(*i))).count();
    let (mut local, mut model) = (0, 0);
    for (i, b) in bones.iter().enumerate().take(n) {
        let Some(p) = usize::try_from(b.parent).ok().filter(|p| *p < i) else { continue };
        let t = translation(f.layout, entry(i));
        let rest = (b.t[0] * b.t[0] + b.t[1] * b.t[1] + b.t[2] * b.t[2]).sqrt();
        if (0..3).all(|k| (t[k] - b.t[k]).abs() < 0.01) {
            local += 1;
        }
        let tp = translation(f.layout, entry(p));
        let d = ((t[0] - tp[0]).powi(2) + (t[1] - tp[1]).powi(2) + (t[2] - tp[2]).powi(2)).sqrt();
        if rest > 0.02 && (d - rest).abs() < 0.01 {
            model += 1;
        }
    }
    let skin = (f.layout == Layout::M34).then(|| skin_match(&f.floats, bones)).flatten();
    (moving, local, model, skin)
}

struct Probe {
    started: Instant,
    /// Per step: the bone and its offset (row-major 3x4).
    steps: Vec<(usize, [f32; 12])>,
    shown: Option<usize>,
    /// The step whose offset was last written (to put the identity back).
    written: Option<usize>,
    writes: Vec<u32>,
    failed: Vec<u32>,
}

static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
static SEARCHING: AtomicBool = AtomicBool::new(false);
static MODEL_ITEM: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// A turn by `deg` about `axis` through `pivot`, row-major 3x4.
fn turn_about(axis: [f32; 3], deg: f32, pivot: [f32; 3]) -> [f32; 12] {
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let [x, y, z] = axis.map(|v| v / l);
    let (s, c) = deg.to_radians().sin_cos();
    let t = 1.0 - c;
    let r = [[t * x * x + c, t * x * y - s * z, t * x * z + s * y], [t * x * y + s * z, t * y * y + c, t * y * z - s * x], [t * x * z - s * y, t * y * z + s * x, t * z * z + c]];
    let mut m = [0.0f32; 12];
    for i in 0..3 {
        for j in 0..3 {
            m[i * 4 + j] = r[i][j];
        }
        m[i * 4 + 3] = pivot[i] - (0..3).map(|k| r[i][k] * pivot[k]).sum::<f32>();
    }
    m
}

/// The debug pose key: search and score on a thread of our own (from `roots`), then run the steps from the game's
/// draw task ([`on_stage`]) through the offset arrays of `model_item`.
pub fn start(roots: Vec<(&'static str, usize)>, model_item: usize) {
    if PROBE.lock().map(|p| p.is_some()).unwrap_or(true) || SEARCHING.swap(true, Ordering::SeqCst) {
        crate::overlay::toast("Pose probe already running (debug)");
        return;
    }
    MODEL_ITEM.store(model_item, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("gowring-pose-probe".into()).spawn(move || {
        let r = std::panic::catch_unwind(|| {
            let Some(bones) = crate::skeleton::bones() else {
                crate::glog!("bonepose", "pose probe: the skeleton has not been read yet (5 s into the world)");
                crate::overlay::toast("Pose probe: skeleton not read yet, try again in a few seconds (debug)");
                return;
            };
            let t0 = Instant::now();
            let (found, looked) = crate::pose::find(&roots, SEARCH_READS, SEARCH_ARRAYS, SEARCH_DEPTH);
            let lines: Vec<String> = found
                .iter()
                .map(|f| {
                    let (moving, local, model, skin) = scores(f, bones);
                    let stride = if f.layout == Layout::M44 { 16 } else { 12 };
                    format!("{} at {:#x} ({} x {}): {moving} not identity; links at rest as local {local}, as model space {model}, as skinning {skin:?}", f.path, f.address, f.floats.len() / stride, f.layout.name())
                })
                .collect();
            crate::glog!("bonepose", "pose probe search: {} array(s) in {} ms ({looked} objects looked at, roots {:?}):\n    {}", found.len(), t0.elapsed().as_millis(), roots.iter().map(|(n, a)| format!("{n} {a:#x}")).collect::<Vec<_>>(), lines.join("\n    "));
            // all of it for offline work, as the pose capture writes it
            let mut j = String::from("{\n  \"capture\": \"probe\",\n  \"arrays\": [\n");
            for (i, f) in found.iter().enumerate() {
                let floats: Vec<String> = f.floats.iter().map(|x| x.to_string()).collect();
                j.push_str(&format!("    {{\"path\": {:?}, \"address\": \"{:#x}\", \"layout\": {:?}, \"floats\": [{}]}}{}\n", f.path, f.address, f.layout.name(), floats.join(","), if i + 1 < found.len() { "," } else { "" }));
            }
            j.push_str("  ]\n}\n");
            let path = crate::paths::get().local.join("er_pose_probe.json");
            let _ = std::fs::write(&path, j);
            // the steps
            let bind = bind_pose(bones);
            let steps: Vec<(usize, [f32; 12])> = STEPS
                .iter()
                .filter_map(|(name, axis, deg, pivoted)| {
                    let i = bones.iter().position(|b| b.name == *name)?;
                    let pivot = if *pivoted { [bind[i][0][3] as f32, bind[i][1][3] as f32, bind[i][2][3] as f32] } else { [0.0; 3] };
                    Some((i, turn_about(*axis, *deg, pivot)))
                })
                .collect();
            let arrays = offset_arrays(MODEL_ITEM.load(Ordering::SeqCst));
            crate::glog!("bonepose", "pose probe: offset arrays {:#x}, {:#x}; {} steps of {STEP_S} s (search written to {})", arrays[0], arrays[1], steps.len(), path.display());
            if steps.is_empty() || arrays == [0, 0] {
                crate::overlay::toast("Pose probe: no offset arrays or bones found (debug)");
                return;
            }
            crate::overlay::toast("Pose probe: watch his right hand (twice), then his left arm (debug)");
            if let Ok(mut p) = PROBE.lock() {
                let n = steps.len();
                *p = Some(Probe { started: Instant::now(), steps, shown: None, written: None, writes: vec![0; n], failed: vec![0; n] });
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

/// Write `m` into the entry for `bone` of each offset array (only where the entry reads as a bone's).
fn write_bone(bone: usize, m: &[f32; 12]) -> (u32, u32) {
    let (mut ok, mut bad) = (0, 0);
    for a in offset_arrays(MODEL_ITEM.load(Ordering::SeqCst)) {
        if a == 0 {
            continue;
        }
        let at = a + bone * 48;
        if read::<[f32; 12]>(at).is_some_and(|e| bone_like(&e)) && write_floats(at, m) {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

/// Called from the game's tasks at each stage, every frame: runs the probe's step, if one is running (just before
/// drawing).
pub fn on_stage(stage: Stage) {
    if stage != Stage::DrawPre {
        return;
    }
    let Ok(mut guard) = PROBE.try_lock() else { return };
    let Some(p) = guard.as_mut() else { return };
    let t = p.started.elapsed().as_secs_f32() - LEAD_S;
    if t < 0.0 {
        return;
    }
    let k = (t / (STEP_S + GAP_S)) as usize;
    let within = t - k as f32 * (STEP_S + GAP_S);
    let active = k < p.steps.len() && within <= STEP_S;
    if !active {
        if let Some(w) = p.written.take() {
            write_bone(p.steps[w].0, &IDENTITY);
        }
    }
    if k >= p.steps.len() {
        let names: Vec<String> = STEPS.iter().zip(p.writes.iter().zip(&p.failed)).map(|((n, _, d, at), (w, f))| format!("{n} turned {d} degrees{}: {w} writes, {f} failed", if *at { " about its rest position" } else { "" })).collect();
        crate::glog!("bonepose", "pose probe done: {}", names.join("; "));
        crate::overlay::toast("Pose probe done: what did the hand and the arm do? (debug)");
        *guard = None;
        return;
    }
    if p.shown != Some(k) {
        p.shown = Some(k);
        let (name, _, deg, _) = STEPS[k];
        let line = match k {
            0 | 1 => format!("Pose probe {}/3: right hand turned {deg} degrees. Turning in place, or jumping away? Fingers and weapon with it?", k + 1),
            _ => format!("Pose probe 3/3: left upper arm turned {deg} degrees. Whole arm with it, or torn at the elbow?"),
        };
        crate::glog!("bonepose", "{line} ({name})");
        crate::overlay::toast(&format!("{line} (debug)"));
    }
    if active {
        let (bone, m) = p.steps[k];
        let (ok, bad) = write_bone(bone, &m);
        p.writes[k] += ok;
        p.failed[k] += bad;
        p.written = Some(k);
    }
}

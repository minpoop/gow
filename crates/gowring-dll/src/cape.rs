//! Kratos's cloak, swung by the mod. The launcher skins the back of the cloak below his shoulder blades to a chain of
//! helper bones down his back, Elden Ring's Mantle bones on his spine and pelvis (`gowring_gow::cape`, the rig in
//! `mod/parts/kratos_cape.json`).
//! The game keeps a per-bone offset that it applies in rest space under the animation, per bone, not to its children
//! (the pose probes, 0.1.67-0.1.69: a hand turned about its rest position turned in place, one turned about the origin
//! stretched away, an upper arm turned without its forearm). So every frame, just before drawing, this simulates the
//! chains and writes each chain bone's offset: the cloak bends in rest space, and his spine and hips carry it (the bone's own
//! skinning matrix times our offset). The offsets are not at a fixed address (0.1.67, 0.1.68 and 0.1.69 each found
//! them elsewhere), so they are searched for as the probe does, on a thread of our own, whenever the player's model
//! changes.
//!
//! The simulation, in his own frame (model space: it turns and moves with him; his spine's own movement is not known
//! to the mod, it only carries the result): each chain is a string of points, the first pinned where the rig puts it,
//! the others under gravity, air drag (a gusting wind along his back, and part of the air he moves through), a
//! softened push from his speeding up and slowing down and a softened, capped swing from his turning, pulled back toward
//! where the cloak hangs at rest (firmly near the top, barely at the hem), kept at their rest spacing and out of his
//! torso and legs at rest (capsules, never tighter than the cloak sits). His limbs' movement is not known either, so
//! no point goes in front of where it hangs at rest (0.1.72 let the wind from behind and his stopping swing it into
//! him: "it's phasing through myself"), it keeps more room behind his legs the faster he moves, and it goes only so far
//! to either side. Each chain bone's offset then turns the rest segment below it onto the simulated one about its rest
//! point.
//!
//! Safety: an array counts only when it reads as the offsets (3x4, the identity for nearly every bone); before each
//! frame's writes its root bones (Master, RootPos, Pelvis) must still read as the identity, else it is dropped and the
//! search runs again; every write goes through `WriteProcessMemory` and only to an entry that still reads as a bone's.
//! If nothing is written, the chain bones follow his spine and hips and the cloak looks as it did before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::bonepose::{bind_pose, bone_like, offsets_like, write_floats, IDENTITY};
use crate::skeleton::{read, Bone};

type V3 = [f32; 3];

const GRAVITY: f32 = 9.8;
/// Velocity kept per 1/60 s step.
const DAMP: f32 = 0.985;
/// Air drag (1/s): how quickly the cloak takes the air's speed.
const DRAG: f32 = 2.0;
/// The wind's mean speed (m/s) at `cape_wind` 1.
const WIND: f32 = 4.0;
/// The pull back toward the cloak's rest shape per step, at the top of a chain and at the hem. Tuned offline on the
/// rig's own points (scratchpad `capesim2.py` / `shoulder72/capesim3.py`, the same loop). 0.1.70's 0.04 / 0.002 on
/// chains from mid-back: the hem 6 cm from rest standing still, 12 cm on average in the gusts, 17 walking, 43 running,
/// the upper points barely moving ("only the bottom of the cloak is moving"). Now, on chains from his shoulder blades
/// (bear cloak points), per point top to hem: standing 1 / 5 / 8 cm, in the gusts 6 / 10 / 19 (hem up to 46), walking
/// at 2 m/s 7 / 13 / 26, running at 6 m/s 21 / 39 / 68 (about 40 degrees). Those were 0.015 / 0.0015 with the cape in
/// the world (0.1.73-0.1.78); in his own frame (0.1.79 on) see `HEADWIND`.
const STIFF_TOP: f32 = 0.012;
const STIFF_HEM: f32 = 0.001;
const ITERATIONS: usize = 3;
const STEP_S: f32 = 1.0 / 60.0;
/// A frame longer than this, or the player moving further than `JUMP_M`, starts the cloak again at rest.
const RESET_S: f32 = 0.25;
const JUMP_M: f32 = 3.0;
/// Capsules the cloak stays out of (at rest): two bones and a radius (m).
const BODY: [(&str, &str, f32); 5] = [("Pelvis", "Spine2", 0.15), ("L_Thigh", "L_Calf", 0.10), ("L_Calf", "L_Foot", 0.08), ("R_Thigh", "R_Calf", 0.10), ("R_Calf", "R_Foot", 0.08)];
/// How far each chain point may go to either side of where it hangs at rest, at the hem (m; less toward the top): the
/// cloak's sides wrap round his hips and lie by his arms.
const SIDEWAYS: f32 = 0.12;
/// Room kept behind his legs while he moves, at the hem (m; less toward the top), full from `LEG_ROOM_SPEED` (m/s):
/// his legs swing back further than the cloak hangs behind them at rest.
const LEG_ROOM: f32 = 0.15;
const LEG_ROOM_SPEED: f32 = 4.0;
/// How quickly his velocity and turn rate are followed, and their change (s): the step pulses of a walk are smoothed
/// out of the push he gives the cape.
const VEL_SMOOTH_S: f32 = 0.15;
const ACC_SMOOTH_S: f32 = 0.25;
/// The share of the air he moves through that blows on the cape (his own speed against it), of his speeding up and
/// slowing down it feels, and of the swing of his turning (capped, m/s^2). The cape lives in his own frame: 0.1.70-0.1.78
/// simulated it in the world, so a start whipped it up like a yanked pendulum and a quick turn flung it round through
/// the air ("when i walk it bounces way higher than i should be. and when i turn 180 my cape also freaks out"; offline
/// the hem rose 0.43 m walking and 1.24 m over a half turn, then 0.02 m and 0.04 m without wind).
/// 0.1.79's 0.5 / 0.4 / 0.15 (cap 6) with the pull to rest at 0.015 / 0.0015 were "a bit stiff": offline, without
/// wind, the hem trailed 0.12 m walking and 0.27 m running; now 0.15 and 0.47 (0.1.73's world-space cape, "perfect" in
/// the player's words, trailed about 0.26 and 0.68), 0.16 m of rise over a half turn walking, jumps under 3.5 cm a frame.
const HEADWIND: f32 = 0.8;
const INERTIA: f32 = 0.6;
const TURN: f32 = 0.25;
const TURN_MAX: f32 = 8.0;
/// Bones whose offsets must read as the identity for an array to be written: the root and the feet's IK targets,
/// which nothing offsets (Kratos's own moves, `moves`, offset RootPos and Pelvis, so they no longer can be checked).
const ROOTS: [&str; 3] = ["Master", "L_Foot_Target", "R_Foot_Target"];
/// The least time between searches (s).
const SEARCH_EVERY_S: f32 = 10.0;
/// Seconds between status lines in the log.
const LOG_EVERY_S: f32 = 20.0;

struct Chain {
    bones: Vec<usize>,
    rest: Vec<V3>,
    len: Vec<f32>,
    /// Per point, per capsule: the least distance the point may come to the capsule's line.
    clear: Vec<Vec<f32>>,
    p: Vec<V3>,
    q: Vec<V3>,
}

struct Cape {
    chains: Vec<Chain>,
    roots: Vec<usize>,
    /// The body's capsules at rest (model space).
    caps: Vec<(V3, V3)>,
    /// His back and his left (model space, level).
    back: V3,
    left: V3,
    /// His velocity (world) and its change, and his turn about y with its rate and the rate's change, smoothed.
    vel: V3,
    last_vel: V3,
    acc: V3,
    yaw: f32,
    w: f32,
    last_w: f32,
    dw: f32,
    /// His fastest speed and quickest turn since the last status line.
    peak_speed: f32,
    peak_turn: f32,
    /// Times a point was held back from going into him or too far to a side.
    held: u64,
    started: bool,
    last: Option<Instant>,
    t: f32,
    last_pos: V3,
    frames: u64,
    writes: u64,
    failed: u64,
    next_log: f32,
}

static CAPE: Mutex<Option<Cape>> = Mutex::new(None);
/// The rig was looked for (once a session, once the skeleton is read).
static LOADED: AtomicBool = AtomicBool::new(false);
/// The offset arrays to write, the model item they were found from, and when the last search started.
static ARRAYS: Mutex<(Vec<usize>, usize, Option<Instant>)> = Mutex::new((Vec::new(), 0, None));
static SEARCHING: AtomicBool = AtomicBool::new(false);
/// Switched off and on from the debug key (`debug_cape_key`): off, the chain bones go back to rest and the cloak hangs
/// as it was built, carried by his bones.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Switch the swing off or on; true when it is now on.
pub fn toggle() -> bool {
    !ENABLED.fetch_xor(true, Ordering::Relaxed)
}

/// The offset arrays found for the player's model (none until the search has run, or with no cape rig).
pub(crate) fn offset_arrays() -> Vec<usize> {
    ARRAYS.lock().map(|a| a.0.clone()).unwrap_or_default()
}

/// This frame's chain bone offsets (bone, row-major 3x4), for `moves` to put on top of Kratos's pose; none while the
/// swing is off.
static CURRENT: Mutex<Vec<(usize, [f32; 12])>> = Mutex::new(Vec::new());

pub(crate) fn current_offsets() -> Vec<(usize, [f32; 12])> {
    CURRENT.lock().map(|c| c.clone()).unwrap_or_default()
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

type M3 = [[f32; 3]; 3];

fn mul3(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}
fn mv(a: &M3, v: V3) -> V3 {
    [dot(a[0], v), dot(a[1], v), dot(a[2], v)]
}

/// The rotation turning direction `a` onto `b` by the shortest way (both non-zero).
fn align(a: V3, b: V3) -> M3 {
    let (la, lb) = (length(a), length(b));
    if la < 1e-6 || lb < 1e-6 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let (a, b) = (scale(a, 1.0 / la), scale(b, 1.0 / lb));
    let v = cross(a, b);
    let c = dot(a, b);
    if c < -0.9999 {
        // opposite: half a turn about any axis across a
        let axis = if a[0].abs() < 0.9 { cross(a, [1.0, 0.0, 0.0]) } else { cross(a, [0.0, 1.0, 0.0]) };
        let n = scale(axis, 1.0 / length(axis));
        return std::array::from_fn(|i| std::array::from_fn(|j| 2.0 * n[i] * n[j] - if i == j { 1.0 } else { 0.0 }));
    }
    let k: M3 = [[0.0, -v[2], v[1]], [v[2], 0.0, -v[0]], [-v[1], v[0], 0.0]];
    let kk = mul3(&k, &k);
    let f = 1.0 / (1.0 + c);
    std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 } + k[i][j] + kk[i][j] * f))
}

/// The quaternion (x, y, z, w) as a rotation matrix.
fn quat(q: [f32; 4]) -> M3 {
    let [x, y, z, w] = q;
    [[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)], [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)], [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)]]
}

/// Push `p` by `by` along `dir` (unit) and take away its speed against `dir` (Verlet: `q` is the last position).
fn hold(p: &mut V3, q: &mut V3, dir: V3, by: f32) {
    *p = add(*p, scale(dir, by));
    let against = dot(sub(*p, *q), dir);
    if against < 0.0 {
        *q = add(*q, scale(dir, against));
    }
}

/// The closest point to `p` on the segment `a`-`b`.
fn closest_on(p: V3, a: V3, b: V3) -> V3 {
    let ab = sub(b, a);
    let l = dot(ab, ab);
    let t = if l < 1e-9 { 0.0 } else { (dot(sub(p, a), ab) / l).clamp(0.0, 1.0) };
    add(a, scale(ab, t))
}

/// The rig file (`gowring_gow::cape::Rig::json`) read against the skeleton.
fn load(bones: &'static [Bone]) -> Result<Option<Cape>, String> {
    let path = crate::paths::get().home.join("mod").join("parts").join("kratos_cape.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let index = |name: &str| bones.iter().position(|b| b.name == name);
    let chains_json = v["chains"].as_array().cloned().unwrap_or_default();
    if chains_json.is_empty() {
        return Ok(None);
    }
    let bind = bind_pose(bones);
    let bind_pos: Vec<V3> = bind.iter().map(|m| [m[0][3] as f32, m[1][3] as f32, m[2][3] as f32]).collect();
    let capsules: Vec<(V3, V3, f32)> = BODY.iter().filter_map(|(a, b, r)| Some((bind_pos[index(a)?], bind_pos[index(b)?], *r))).collect();
    let roots: Vec<usize> = ROOTS.iter().filter_map(|n| index(n)).collect();
    // his back, away from his toes; his left, by his thighs (as the rig takes them)
    let at = |n: &str| index(n).map(|i| bind_pos[i]);
    let level = |v: V3| -> Option<V3> {
        let l = (v[0] * v[0] + v[2] * v[2]).sqrt();
        (l > 1e-4).then(|| [v[0] / l, 0.0, v[2] / l])
    };
    let back = (|| level(scale(add(sub(at("L_Toe0")?, at("L_Foot")?), sub(at("R_Toe0")?, at("R_Foot")?)), -1.0)))().ok_or("no feet in the skeleton")?;
    let left = (|| level(sub(at("L_Thigh")?, at("R_Thigh")?)))().ok_or("no thighs in the skeleton")?;
    let mut chains = Vec::new();
    for c in &chains_json {
        let names: Vec<&str> = c["bones"].as_array().map(|a| a.iter().filter_map(|n| n.as_str()).collect()).unwrap_or_default();
        let rest: Vec<V3> = c["points"].as_array().map(|a| a.iter().filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32])).collect()).unwrap_or_default();
        let bone_ids: Vec<usize> = names.iter().map(|n| index(n)).collect::<Option<Vec<_>>>().ok_or("a chain bone is not in the skeleton")?;
        if bone_ids.len() != rest.len() || rest.len() < 2 || bone_ids.iter().any(|b| *b >= bones.len().min(150) || roots.contains(b)) {
            return Err("a chain does not match its points (or names a bone the offsets do not reach)".into());
        }
        let len: Vec<f32> = (0..rest.len()).map(|k| if k == 0 { 0.0 } else { length(sub(rest[k], rest[k - 1])) }).collect();
        // never pushed further out than the cloak sits at rest
        let clear: Vec<Vec<f32>> = rest.iter().map(|p| capsules.iter().map(|(a, b, r)| r.min(length(sub(*p, closest_on(*p, *a, *b))) - 0.015).max(0.0)).collect()).collect();
        chains.push(Chain { bones: bone_ids, p: rest.clone(), q: rest.clone(), rest, len, clear });
    }
    crate::glog!("cape", "rig: {} chain(s) of {} points from {} ({} cloak vertices), bones {:?}, {} body capsules", chains.len(), chains[0].rest.len(), path.display(), v["vertices"].as_u64().unwrap_or(0), chains.iter().map(|c| c.bones.iter().map(|b| bones[*b].name.as_str()).collect::<Vec<_>>()).collect::<Vec<_>>(), capsules.len());
    Ok(Some(Cape { chains, roots, caps: capsules.iter().map(|(a, b, _)| (*a, *b)).collect(), back, left, vel: [0.0; 3], last_vel: [0.0; 3], acc: [0.0; 3], yaw: 0.0, w: 0.0, last_w: 0.0, dw: 0.0, peak_speed: 0.0, peak_turn: 0.0, held: 0, started: false, last: None, t: 0.0, last_pos: [0.0; 3], frames: 0, writes: 0, failed: 0, next_log: 5.0 }))
}

/// Search for the offset arrays from the player's model item on a thread of our own (as the pose probe does).
fn search(model_item: usize, bones: &'static [Bone]) {
    if SEARCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(mut a) = ARRAYS.lock() {
        a.2 = Some(Instant::now());
    }
    let spawned = std::thread::Builder::new().name("gowring-cape-search".into()).spawn(move || {
        let r = std::panic::catch_unwind(|| {
            let mut roots: Vec<(&'static str, usize)> = Vec::new();
            if let Some(e) = read::<usize>(model_item + crate::pose::MTX43_ARRAY_ENTITY).filter(|p| *p > 0x10000) {
                roots.push(("mtx43 array entity", e));
            }
            roots.push(("model item", model_item));
            let t0 = Instant::now();
            let (found, looked) = crate::pose::find(&roots, crate::bonepose::SEARCH_READS, crate::bonepose::SEARCH_ARRAYS, crate::bonepose::SEARCH_DEPTH);
            let offsets: Vec<(String, usize)> = found.iter().filter(|f| offsets_like(f, bones)).map(|f| (f.path.clone(), f.address)).collect();
            crate::glog!("cape", "offset arrays: {} of {} arrays found ({looked} objects, {} ms): {:?}", offsets.len(), found.len(), t0.elapsed().as_millis(), offsets.iter().map(|(p, a)| format!("{p} at {a:#x}")).collect::<Vec<_>>());
            if let Ok(mut a) = ARRAYS.lock() {
                a.0 = offsets.into_iter().map(|(_, a)| a).collect();
                a.1 = model_item;
            }
        });
        if r.is_err() {
            crate::glog!("cape", "the offset search panicked");
        }
        SEARCHING.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        SEARCHING.store(false, Ordering::SeqCst);
    }
}

/// Called just before drawing, every frame.
pub fn step() {
    if !LOADED.load(Ordering::Relaxed) {
        let Some(bones) = crate::skeleton::bones() else { return };
        LOADED.store(true, Ordering::Relaxed);
        let cape = match load(bones) {
            Ok(Some(c)) => Some(c),
            Ok(None) => {
                crate::glog!("cape", "no cape rig (the cape is off, or the outfit has no cloak behind him); the cloak stays as it is");
                None
            }
            Err(e) => {
                crate::glog!("cape", "the cape rig did not load: {e}");
                None
            }
        };
        if let Ok(mut g) = CAPE.lock() {
            *g = cape;
        }
    }
    let Some(bones) = crate::skeleton::bones() else { return };
    let Ok(mut guard) = CAPE.try_lock() else { return };
    let Some(c) = guard.as_mut() else { return };
    let now = Instant::now();
    let dt = c.last.map_or(1.0, |l| now.duration_since(l).as_secs_f32());
    c.last = Some(now);
    // the player: model item, position and turn
    let player = (|| {
        use eldenring::cs::WorldChrMan;
        use eldenring::position::HavokPosition;
        use fromsoftware_shared::FromStatic;
        let wcm = unsafe { WorldChrMan::instance() }.ok()?;
        let p = wcm.main_player.as_ref()?;
        let model_item = crate::host::model_item_of(&p.chr_ins)?;
        let HavokPosition(x, y, z, _) = p.chr_ins.modules.physics.position;
        let o = p.chr_ins.modules.physics.orientation;
        Some((model_item, [x, y, z], [o.0, o.1, o.2, o.3]))
    })();
    let Some((model_item, pos, turn)) = player else { return };
    // the offset arrays: searched for when the model changes or none is left (at most every few seconds)
    let arrays = {
        let Ok(mut a) = ARRAYS.lock() else { return };
        let stale = a.1 != model_item || a.0.is_empty();
        let due = a.2.map_or(true, |t| t.elapsed().as_secs_f32() >= SEARCH_EVERY_S);
        if stale && due {
            if a.1 != model_item {
                a.0.clear();
            }
            drop(a);
            search(model_item, bones);
            return;
        }
        // an array whose root bones no longer read as the identity is not the offsets any more
        let roots = &c.roots;
        let before = a.0.len();
        a.0.retain(|addr| roots.iter().all(|r| read::<[f32; 12]>(addr + r * 48).is_some_and(|e| e == IDENTITY)));
        if a.0.len() < before {
            crate::glog!("cape", "{} offset array(s) dropped (their root bones changed); {} left", before - a.0.len(), a.0.len());
        }
        a.0.clone()
    };
    if arrays.is_empty() {
        return;
    }
    // switched off: the chain bones back at rest once, then nothing written until it is on again (and starts at rest)
    if !ENABLED.load(Ordering::Relaxed) {
        if let Ok(mut cur) = CURRENT.lock() {
            cur.clear();
        }
        if c.started {
            for addr in &arrays {
                for bone in c.chains.iter().flat_map(|ch| ch.bones.iter()) {
                    let at = addr + bone * 48;
                    if read::<[f32; 12]>(at).is_some_and(|e| bone_like(&e)) && write_floats(at, &IDENTITY) {
                        c.writes += 1;
                    }
                }
            }
            c.started = false;
        }
        return;
    }
    let turn_m = quat(turn);
    let turn_t: M3 = std::array::from_fn(|i| std::array::from_fn(|j| turn_m[j][i]));
    let reset = !c.started || dt > RESET_S || length(sub(pos, c.last_pos)) > JUMP_M;
    // his motion, smoothed: velocity, acceleration, turn rate and its change (world, and his turn about y)
    let h_all = dt.min(RESET_S);
    let ease = |tau: f32| 1.0 - (-h_all / tau).exp();
    let yaw = turn_m[0][2].atan2(turn_m[0][0]);
    if reset {
        (c.vel, c.last_vel, c.acc, c.w, c.last_w, c.dw) = ([0.0; 3], [0.0; 3], [0.0; 3], 0.0, 0.0, 0.0);
    } else {
        let v_now = scale(sub(pos, c.last_pos), 1.0 / dt.max(1e-3));
        c.vel = add(c.vel, scale(sub(v_now, c.vel), ease(VEL_SMOOTH_S)));
        let a_now = scale(sub(c.vel, c.last_vel), 1.0 / dt.max(1e-3));
        c.acc = add(c.acc, scale(sub(a_now, c.acc), ease(ACC_SMOOTH_S)));
        let mut turned = yaw - c.yaw;
        if turned > std::f32::consts::PI {
            turned -= 2.0 * std::f32::consts::PI;
        } else if turned < -std::f32::consts::PI {
            turned += 2.0 * std::f32::consts::PI;
        }
        c.w += (turned / dt.max(1e-3) - c.w) * ease(VEL_SMOOTH_S);
        c.dw += ((c.w - c.last_w) / dt.max(1e-3) - c.dw) * ease(ACC_SMOOTH_S);
    }
    c.last_vel = c.vel;
    c.last_w = c.w;
    c.yaw = yaw;
    let speed = (c.vel[0] * c.vel[0] + c.vel[2] * c.vel[2]).sqrt();
    c.peak_speed = c.peak_speed.max(speed);
    c.peak_turn = c.peak_turn.max(c.w.abs());
    let room = (speed / LEG_ROOM_SPEED).min(1.0);
    let (back, left) = (c.back, c.left);
    let (vel, acc, w, dw) = (c.vel, c.acc, c.w, c.dw);
    let mut held = 0;
    c.started = true;
    c.last_pos = pos;
    let st = crate::settings::get();
    let wind_scale = st.cape_wind.clamp(0.0, 3.0);
    let steps = ((h_all / STEP_S).ceil() as usize).clamp(1, 4);
    let h = h_all / steps as f32;
    let damp = DAMP.powf(h / STEP_S);
    let t0 = c.t;
    c.t += h * steps as f32;
    // the air past him and his speeding up and slowing down, in his own frame (where the chains live)
    let push = scale(mv(&turn_t, acc), -INERTIA);
    let caps = &c.caps;
    for chain in c.chains.iter_mut() {
        if reset {
            chain.p = chain.rest.clone();
            chain.q = chain.rest.clone();
        }
        let last = chain.rest.len() - 1;
        for s in 0..steps {
            let t = t0 + h * (s + 1) as f32;
            let gust = (0.6 + 0.4 * (0.7 * t).sin() + 0.25 * (1.9 * t + 1.1).sin() + 0.15 * (4.3 * t + 0.3).sin()).max(0.0);
            let angle = 2.0 + 0.5 * (0.03 * t).sin();
            let wind = [angle.cos() * WIND * gust * wind_scale, 0.0, angle.sin() * WIND * gust * wind_scale];
            // the wind along his back only: from a side it held the cloak against its sideways limit for as long as
            // he faced that way (0.1.82: "its only moving to the side like that after i move for the first time";
            // offline, standing side-on, the hem sat 8 to 11 cm to one side, and 2 cm without the wind's side part)
            let breeze = mv(&turn_t, wind);
            let air = sub(sub(breeze, scale(left, dot(breeze, left))), mv(&turn_t, scale(vel, HEADWIND)));
            for k in 1..=last {
                let v = scale(sub(chain.p[k], chain.q[k]), damp);
                let velp = scale(v, 1.0 / h);
                // his turn: the cape lags its start (Euler) and flares out (centrifugal), softened and capped
                let r = [chain.p[k][0], 0.0, chain.p[k][2]];
                let lag = [-dw * r[2], 0.0, dw * r[0]];
                let flare = scale(r, w * w);
                let mut swing = scale(add(lag, flare), TURN);
                let sl = length(swing);
                if sl > TURN_MAX {
                    swing = scale(swing, TURN_MAX / sl);
                }
                let a = add(add(add([0.0, -GRAVITY, 0.0], scale(sub(air, velp), DRAG)), push), swing);
                chain.q[k] = chain.p[k];
                chain.p[k] = add(add(chain.p[k], v), scale(a, h * h));
                // back toward the rest shape, firmly at the top
                let s = STIFF_TOP + (STIFF_HEM - STIFF_TOP) * k as f32 / last as f32;
                chain.p[k] = add(chain.p[k], scale(sub(chain.rest[k], chain.p[k]), s));
            }
            chain.p[0] = chain.rest[0];
            chain.q[0] = chain.rest[0];
            for _ in 0..ITERATIONS {
                for k in 1..=last {
                    let d = sub(chain.p[k], chain.p[k - 1]);
                    let l = length(d);
                    if l > 1e-6 {
                        let to = add(chain.p[k - 1], scale(d, chain.len[k] / l));
                        let moved = sub(to, chain.p[k]);
                        chain.p[k] = to;
                        chain.q[k] = add(chain.q[k], scale(moved, 0.5));
                    }
                    for (ci, (a, b)) in caps.iter().enumerate() {
                        let r = chain.clear[k][ci];
                        if r <= 0.0 {
                            continue;
                        }
                        let on = closest_on(chain.p[k], *a, *b);
                        let out = sub(chain.p[k], on);
                        let d = length(out);
                        if d < r && d > 1e-6 {
                            chain.p[k] = add(on, scale(out, r / d));
                        }
                    }
                    // never in front of where it hangs at rest (into his back and legs), with room behind his legs
                    // while he moves, and only so far to either side (into his hips and arms)
                    let f = k as f32 / last as f32;
                    let least = LEG_ROOM * f * room;
                    let along = dot(sub(chain.p[k], chain.rest[k]), back);
                    if along < least {
                        hold(&mut chain.p[k], &mut chain.q[k], back, least - along);
                        held += 1;
                    }
                    let across = dot(sub(chain.p[k], chain.rest[k]), left);
                    let most = SIDEWAYS * f;
                    if across.abs() > most {
                        hold(&mut chain.p[k], &mut chain.q[k], scale(left, -across.signum()), across.abs() - most);
                        held += 1;
                    }
                }
            }
        }
    }
    c.held += held;
    // each chain bone's offset (rest space): its rest segment turned onto the simulated one about its rest point
    let mut out: Vec<(usize, [f32; 12])> = Vec::new();
    for chain in &c.chains {
        let pm = &chain.p;
        let last = chain.rest.len() - 1;
        for k in 0..=last {
            let (a, b) = if k < last { (k, k + 1) } else { (k - 1, k) };
            let r = align(sub(chain.rest[b], chain.rest[a]), sub(pm[b], pm[a]));
            let t = sub(pm[k], mv(&r, chain.rest[k]));
            out.push((chain.bones[k], [r[0][0], r[0][1], r[0][2], t[0], r[1][0], r[1][1], r[1][2], t[1], r[2][0], r[2][1], r[2][2], t[2]]));
        }
    }
    if let Ok(mut cur) = CURRENT.lock() {
        cur.clone_from(&out);
    }
    for addr in &arrays {
        for (bone, mat) in &out {
            let at = addr + bone * 48;
            if read::<[f32; 12]>(at).is_some_and(|e| bone_like(&e)) && write_floats(at, mat) {
                c.writes += 1;
            } else {
                c.failed += 1;
            }
        }
    }
    c.frames += 1;
    if c.t >= c.next_log {
        c.next_log = c.t + LOG_EVERY_S;
        // and which way: behind, to his left (minus: his right) and up
        let hem: Vec<String> = c
            .chains
            .iter()
            .map(|ch| {
                let d = sub(ch.p[ch.p.len() - 1], ch.rest[ch.rest.len() - 1]);
                format!("{:.2} (back {:+.2}, left {:+.2}, up {:+.2})", length(d), dot(d, c.back), dot(d, c.left), d[1])
            })
            .collect();
        crate::glog!("cape", "{} frames, {} writes, {} failed, into {} offset arrays; hems {} m from rest; {} holds (kept out of him or from going too far sideways); fastest {:.1} m/s and quickest turn {:.1} rad/s since the last line; wind x{wind_scale}", c.frames, c.writes, c.failed, arrays.len(), hem.join(", "), c.held, c.peak_speed, c.peak_turn);
        c.peak_speed = 0.0;
        c.peak_turn = 0.0;
    }
}

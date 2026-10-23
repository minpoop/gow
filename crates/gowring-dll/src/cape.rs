//! Kratos's cloak, swung by the mod. The launcher skins the back of the cloak below mid-back to two chains of helper
//! bones that hang off his spine and pelvis (`gowring_gow::cape`, the rig in `mod/parts/kratos_cape.json`). The game
//! keeps a per-bone offset that it applies in rest space under the animation, per bone, not to its children (the pose
//! probes, 0.1.67-0.1.69: a hand turned about its rest position turned in place, one turned about the origin stretched
//! away, an upper arm turned without its forearm). So every frame, just before drawing, this simulates the chains and
//! writes each chain bone's offset: the cloak bends in rest space, and his spine and hips carry it (the bone's own
//! skinning matrix times our offset). The offsets are not at a fixed address (0.1.67, 0.1.68 and 0.1.69 each found
//! them elsewhere), so they are searched for as the probe does, on a thread of our own, whenever the player's model
//! changes.
//!
//! The simulation, in world space following his body as a whole (his position and turn; his spine's own movement is
//! not known to the mod, it only carries the result): each chain is a string of points, the first pinned where the rig
//! puts it, the others under gravity and air drag (a gusting wind, and the air he runs through), pulled back toward
//! where the cloak hangs at rest (firmly near the top, barely at the hem), kept at their rest spacing and out of his
//! torso and legs at rest (capsules, never tighter than the cloak sits). Each chain bone's offset then turns the rest
//! segment below it onto the simulated one about its rest point.
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
/// rig's own points (scratchpad `capesim2.py`, the same loop): the hem 4 cm from rest standing still, 10 cm on average
/// (up to 24) in the gusts, 15 cm walking at 2 m/s, 38 cm (about 30 degrees) running at 6 m/s; 0.20 / 0.02 kept it
/// within 3 cm in the wind and 14 cm running.
const STIFF_TOP: f32 = 0.04;
const STIFF_HEM: f32 = 0.002;
const ITERATIONS: usize = 3;
const STEP_S: f32 = 1.0 / 60.0;
/// A frame longer than this, or the player moving further than `JUMP_M`, starts the cloak again at rest.
const RESET_S: f32 = 0.25;
const JUMP_M: f32 = 3.0;
/// Capsules the cloak stays out of (at rest): two bones and a radius (m).
const BODY: [(&str, &str, f32); 5] = [("Pelvis", "Spine2", 0.15), ("L_Thigh", "L_Calf", 0.10), ("L_Calf", "L_Foot", 0.08), ("R_Thigh", "R_Calf", 0.10), ("R_Calf", "R_Foot", 0.08)];
/// Bones whose offsets must read as the identity for an array to be written (nothing offsets the root).
const ROOTS: [&str; 3] = ["Master", "RootPos", "Pelvis"];
/// The least time between searches (s).
const SEARCH_EVERY_S: f32 = 10.0;
/// Seconds between status lines in the log.
const LOG_EVERY_S: f32 = 60.0;

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
    Ok(Some(Cape { chains, roots, caps: capsules.iter().map(|(a, b, _)| (*a, *b)).collect(), started: false, last: None, t: 0.0, last_pos: [0.0; 3], frames: 0, writes: 0, failed: 0, next_log: 5.0 }))
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
        let model_item = &*p.chr_ins.chr_model_ins.model_ins.model_item as *const _ as usize;
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
    let turn_m = quat(turn);
    let turn_t: M3 = std::array::from_fn(|i| std::array::from_fn(|j| turn_m[j][i]));
    let to_world = |p: V3| add(mv(&turn_m, p), pos);
    let to_model = |p: V3| mv(&turn_t, sub(p, pos));
    let caps: Vec<(V3, V3)> = c.caps.iter().map(|(a, b)| (to_world(*a), to_world(*b))).collect();
    let reset = !c.started || dt > RESET_S || length(sub(pos, c.last_pos)) > JUMP_M;
    c.started = true;
    c.last_pos = pos;
    let st = crate::settings::get();
    let wind_scale = st.cape_wind.clamp(0.0, 3.0);
    let steps = ((dt.min(RESET_S) / STEP_S).ceil() as usize).clamp(1, 4);
    let h = dt.min(RESET_S) / steps as f32;
    let damp = DAMP.powf(h / STEP_S);
    let t0 = c.t;
    c.t += h * steps as f32;
    for chain in c.chains.iter_mut() {
        let targets: Vec<V3> = chain.rest.iter().map(|r| to_world(*r)).collect();
        if reset {
            chain.p = targets.clone();
            chain.q = targets.clone();
        }
        let last = chain.rest.len() - 1;
        for s in 0..steps {
            let t = t0 + h * (s + 1) as f32;
            let gust = (0.6 + 0.4 * (0.7 * t).sin() + 0.25 * (1.9 * t + 1.1).sin() + 0.15 * (4.3 * t + 0.3).sin()).max(0.0);
            let angle = 2.0 + 0.5 * (0.03 * t).sin();
            let wind = [angle.cos() * WIND * gust * wind_scale, 0.0, angle.sin() * WIND * gust * wind_scale];
            for k in 1..=last {
                let v = scale(sub(chain.p[k], chain.q[k]), damp);
                let vel = scale(v, 1.0 / h);
                let a = add([0.0, -GRAVITY, 0.0], scale(sub(wind, vel), DRAG));
                chain.q[k] = chain.p[k];
                chain.p[k] = add(add(chain.p[k], v), scale(a, h * h));
                // back toward the rest shape, firmly at the top
                let s = STIFF_TOP + (STIFF_HEM - STIFF_TOP) * k as f32 / last as f32;
                chain.p[k] = add(chain.p[k], scale(sub(targets[k], chain.p[k]), s));
            }
            chain.p[0] = targets[0];
            chain.q[0] = targets[0];
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
                }
            }
        }
    }
    // each chain bone's offset (rest space): its rest segment turned onto the simulated one about its rest point
    let mut out: Vec<(usize, [f32; 12])> = Vec::new();
    for chain in &c.chains {
        let pm: Vec<V3> = chain.p.iter().map(|p| to_model(*p)).collect();
        let last = chain.rest.len() - 1;
        for k in 0..=last {
            let (a, b) = if k < last { (k, k + 1) } else { (k - 1, k) };
            let r = align(sub(chain.rest[b], chain.rest[a]), sub(pm[b], pm[a]));
            let t = sub(pm[k], mv(&r, chain.rest[k]));
            out.push((chain.bones[k], [r[0][0], r[0][1], r[0][2], t[0], r[1][0], r[1][1], r[1][2], t[1], r[2][0], r[2][1], r[2][2], t[2]]));
        }
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
        let hem: Vec<String> = c.chains.iter().map(|ch| format!("{:.2}", length(sub(ch.p[ch.p.len() - 1], to_world(ch.rest[ch.rest.len() - 1]))))).collect();
        crate::glog!("cape", "{} frames, {} writes, {} failed, into {} offset arrays; hems {} m from rest; wind x{wind_scale}", c.frames, c.writes, c.failed, arrays.len(), hem.join(", "));
    }
}

//! Kratos's cloak, swung by the mod: the launcher skins the back of the cloak below mid-back to two chains of the
//! skeleton's spare bones (`gowring_gow::cape`, the rig in `mod/parts/kratos_cape.json`), and every frame, just before
//! drawing, this simulates the chains and writes their skinning matrices where the game keeps the player's
//! (0.1.67's pose probe: two arrays the game fills on alternate frames, from the model item's bone-matrix entity +0x30,
//! then +0x508 and +0x510, row-major 3x4, the Havok skeleton's order; a write there before drawing is what shows).
//!
//! The simulation, in world space: each chain is a string of points, the first pinned where the rig puts it on the
//! anchor bone, the others under gravity and air drag (a gusting wind, and the air he runs through), pulled back toward
//! where the cloak hangs at rest on the anchor (firmly near the top, barely at the hem), kept at their rest spacing, and
//! kept out of his torso and legs (capsules between their bones, never tighter than the cloak sits at rest). Each chain
//! bone then turns the rest segment below it onto the simulated one, following the anchor's own turn.
//!
//! Safety as in the probe: an array counts only when its entries read as the skeleton's skinning matrices (the chain
//! bones left out), every write goes through `WriteProcessMemory`, and an entry is written only while it still reads
//! as a bone's.

use std::sync::Mutex;
use std::time::Instant;

use crate::bonepose::{bind_pose, bone_like, skin_match_except, write_floats};
use crate::skeleton::{read, read_vec, Bone};

type V3 = [f32; 3];

/// Offsets from the model item to the two arrays (see the module doc).
const ENTITY: usize = crate::pose::MTX43_ARRAY_ENTITY;
const TO_LIST: usize = 0x30;
const ARRAYS: [usize; 2] = [0x508, 0x510];
/// An array counts when this share of the skeleton's links (the chain bones' left out) sit at rest offset.
const MIN_MATCH: f32 = 0.9;

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
/// Capsules the cloak stays out of: two bones and a radius (m).
const BODY: [(&str, &str, f32); 5] = [("Pelvis", "Spine2", 0.15), ("L_Thigh", "L_Calf", 0.10), ("L_Calf", "L_Foot", 0.08), ("R_Thigh", "R_Calf", 0.10), ("R_Calf", "R_Foot", 0.08)];
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
    anchor: usize,
    bones: &'static [Bone],
    bind_pos: Vec<V3>,
    capsules: Vec<(usize, usize)>,
    skip: Vec<usize>,
    /// The arrays' pointers as last read, and whether each checked out (its address, or 0).
    seen: [usize; 2],
    good: [usize; 2],
    /// What we last wrote into each array's first chain bone (to tell which one the game filled this frame).
    wrote: [[f32; 12]; 2],
    started: bool,
    last: Option<Instant>,
    t: f32,
    last_pos: V3,
    frames: u64,
    writes: u64,
    failed: u64,
    fresh: [u64; 2],
    next_log: f32,
}

static CAPE: Mutex<Option<Cape>> = Mutex::new(None);
/// The rig was looked for (once a session, once the skeleton is read).
static LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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

/// A row-major 3x4 applied to a point.
fn apply(m: &[f32; 12], p: V3) -> V3 {
    [m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3], m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7], m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11]]
}

type M3 = [[f32; 3]; 3];

fn rot_of(m: &[f32; 12]) -> M3 {
    [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]]
}
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
    let anchor = v["anchor"].as_str().and_then(index).ok_or("the rig's anchor is not in the skeleton")?;
    let bind = bind_pose(bones);
    let bind_pos: Vec<V3> = bind.iter().map(|m| [m[0][3] as f32, m[1][3] as f32, m[2][3] as f32]).collect();
    let capsules: Vec<(usize, usize, f32)> = BODY.iter().filter_map(|(a, b, r)| Some((index(a)?, index(b)?, *r))).collect();
    let mut chains = Vec::new();
    let mut skip = Vec::new();
    for c in &chains_json {
        let names: Vec<&str> = c["bones"].as_array().map(|a| a.iter().filter_map(|n| n.as_str()).collect()).unwrap_or_default();
        let rest: Vec<V3> = c["points"].as_array().map(|a| a.iter().filter_map(|p| Some([p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32])).collect()).unwrap_or_default();
        let bone_ids: Vec<usize> = names.iter().map(|n| index(n)).collect::<Option<Vec<_>>>().ok_or("a chain bone is not in the skeleton")?;
        if bone_ids.len() != rest.len() || rest.len() < 2 || bone_ids.iter().any(|b| *b >= 150 || *b == anchor) {
            return Err("a chain does not match its points (or names a bone the arrays do not reach)".into());
        }
        let len: Vec<f32> = (0..rest.len()).map(|k| if k == 0 { 0.0 } else { length(sub(rest[k], rest[k - 1])) }).collect();
        // never pushed further out than the cloak sits at rest
        let clear: Vec<Vec<f32>> = rest
            .iter()
            .map(|p| capsules.iter().map(|(a, b, r)| r.min(length(sub(*p, closest_on(*p, bind_pos[*a], bind_pos[*b]))) - 0.015).max(0.0)).collect())
            .collect();
        skip.extend(&bone_ids);
        chains.push(Chain { bones: bone_ids, p: rest.clone(), q: rest.clone(), rest, len, clear });
    }
    crate::glog!("cape", "rig: {} chain(s) of {} points from {} ({} cloak vertices), anchor {}, {} body capsules", chains.len(), chains[0].rest.len(), path.display(), v["vertices"].as_u64().unwrap_or(0), bones[anchor].name, capsules.len());
    Ok(Some(Cape {
        chains,
        anchor,
        bones,
        bind_pos,
        capsules: capsules.iter().map(|(a, b, _)| (*a, *b)).collect(),
        skip,
        seen: [0; 2],
        good: [0; 2],
        wrote: [[0.0; 12]; 2],
        started: false,
        last: None,
        t: 0.0,
        last_pos: [0.0; 3],
        frames: 0,
        writes: 0,
        failed: 0,
        fresh: [0; 2],
        next_log: 5.0,
    }))
}

/// The two arrays' addresses this frame, checked when they change (0 for one that is not there or does not check out).
fn arrays(c: &mut Cape, model_item: usize) -> [usize; 2] {
    let list = read::<usize>(model_item + ENTITY).and_then(|e| read::<usize>(e + TO_LIST));
    for (k, off) in ARRAYS.iter().enumerate() {
        let ptr = list.and_then(|l| read::<usize>(l + off)).unwrap_or(0);
        if ptr != c.seen[k] {
            c.seen[k] = ptr;
            c.good[k] = 0;
            let n = c.bones.len().min(150);
            let checked = (ptr != 0).then(|| read_vec::<f32>(ptr, n * 12)).flatten().and_then(|f| skin_match_except(&f, &c.bones[..n], &c.skip));
            match checked {
                Some((m, l)) if l > 0 && m as f32 >= MIN_MATCH * l as f32 => {
                    c.good[k] = ptr;
                    crate::glog!("cape", "bone array {} at {ptr:#x}: {m}/{l} links at rest offset", k + 1);
                }
                other => crate::glog!("cape", "bone array {} at {ptr:#x} does not check out ({other:?}); not written", k + 1),
            }
        }
    }
    c.good
}

/// Called just before drawing, every frame.
pub fn step() {
    if !LOADED.load(std::sync::atomic::Ordering::Relaxed) {
        let Some(bones) = crate::skeleton::bones() else { return };
        LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
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
    let good = arrays(c, model_item);
    if good == [0, 0] {
        crate::log::once("cape-no-arrays", "cape", "no bone array checks out; the cloak is not swung");
        return;
    }
    // this frame's pose: from the array the game filled (its first chain bone no longer what we wrote there)
    let n = c.bones.len().min(150);
    let reads: Vec<Option<Vec<f32>>> = good.iter().map(|a| (*a != 0).then(|| read_vec::<f32>(*a, n * 12)).flatten()).collect();
    let first = c.chains[0].bones[0];
    let filled = (0..2).filter(|k| reads[*k].as_ref().is_some_and(|f| f[first * 12..first * 12 + 12] != c.wrote[*k][..])).collect::<Vec<_>>();
    let src = filled.first().copied().or_else(|| (0..2).find(|k| reads[*k].is_some()));
    let Some(src) = src else { return };
    c.fresh[src] += 1;
    let f = reads[src].as_ref().unwrap();
    let m = |b: usize| -> [f32; 12] { std::array::from_fn(|i| f[b * 12 + i]) };
    let anchor = m(c.anchor);
    if !bone_like(&anchor) {
        c.failed += 1;
        return;
    }
    let turn_m = quat(turn);
    let turn_t: M3 = std::array::from_fn(|i| std::array::from_fn(|j| turn_m[j][i]));
    let to_world = |p: V3| add(mv(&turn_m, p), pos);
    let to_model = |p: V3| mv(&turn_t, sub(p, pos));
    // the body's capsules, in world space
    let caps: Vec<(V3, V3)> = c.capsules.iter().map(|(a, b)| (to_world(apply(&m(*a), c.bind_pos[*a])), to_world(apply(&m(*b), c.bind_pos[*b])))).collect();
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
        let targets: Vec<V3> = chain.rest.iter().map(|r| to_world(apply(&anchor, *r))).collect();
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
    // the chain bones' matrices: each rest segment onto its simulated one, after the anchor's own turn
    let ra = rot_of(&anchor);
    let mut out: Vec<(usize, [f32; 12])> = Vec::new();
    for chain in &c.chains {
        let pm: Vec<V3> = chain.p.iter().map(|p| to_model(*p)).collect();
        let last = chain.rest.len() - 1;
        for k in 0..=last {
            let (a, b) = if k < last { (k, k + 1) } else { (k - 1, k) };
            let r = mul3(&align(mv(&ra, sub(chain.rest[b], chain.rest[a])), sub(pm[b], pm[a])), &ra);
            let t = sub(pm[k], mv(&r, chain.rest[k]));
            out.push((chain.bones[k], [r[0][0], r[0][1], r[0][2], t[0], r[1][0], r[1][1], r[1][2], t[1], r[2][0], r[2][1], r[2][2], t[2]]));
        }
    }
    for (k, addr) in good.iter().enumerate() {
        if *addr == 0 {
            continue;
        }
        for (bone, mat) in &out {
            let at = addr + bone * 48;
            if read::<[f32; 12]>(at).is_some_and(|e| bone_like(&e)) && write_floats(at, mat) {
                c.writes += 1;
                if *bone == first {
                    c.wrote[k] = *mat;
                }
            } else {
                c.failed += 1;
            }
        }
    }
    c.frames += 1;
    if c.t >= c.next_log {
        c.next_log = c.t + LOG_EVERY_S;
        let hem: Vec<String> = c.chains.iter().map(|ch| format!("{:.2}", length(sub(ch.p[ch.p.len() - 1], to_world(apply(&anchor, ch.rest[ch.rest.len() - 1])))))).collect();
        crate::glog!("cape", "{} frames, {} writes, {} failed; pose from array 1/2 {:?}; hems {} m from rest; wind x{wind_scale}", c.frames, c.writes, c.failed, c.fresh, hem.join(", "));
    }
}

//! Kratos's own moves in play (step 3 of giving him his movement): his Ragnarök idle, decoded and retargeted onto the
//! player skeleton by the launcher (`gowring_gow::kratos_anim`, `kratos_moves.bin` beside the parts), shown while the
//! player stands still.
//!
//! Each frame, just before drawing (after the cape), for each of the game's two buffers: the game's skinning `Y`
//! (`livepose`, just before the offsets) and the clip's skinning in model space `K`; the character is where the
//! root's skinning puts him (`C = Y[Master]`), so the skinning wanted is `C x K` and the offset that shows it, in rest
//! space under the game's pose, is `Y^-1 x C x K` (for the cape's bones, times the cape's own offset on top). The
//! bones from RootPos up are written in one write per buffer; Master and the feet's IK targets are left alone (the
//! cape checks them before writing).
//!
//! When: the player stands still in the game's own pose (his root moved under 0.3 m/s and his hands under 0.6 m/s in
//! his own frame, both over the last 0.4 s): the idle blends in over 0.3 s, and out over 0.12 s as soon as he moves,
//! back to the game's pose.
//!
//! 0.1.91 in play: the idle showed and the body was right, but both hands came out as flat stretched slabs and the
//! axe floated (the game places weapons from its own hand). Offline, his real mesh under the same pose closes both
//! hands into fists, so something in the game changes the hands after this step reads them (the hand IK that keeps
//! two hands on a weapon is one candidate). So the debug pose key now cycles: Kratos's idle; his idle with the game's
//! own arms (the hands and the weapon where the game puts them); Elden Ring's rest pose (every bone that takes the
//! offsets snaps to it, any that does not shows); off. And the first time each is shown, a trace reads the game's
//! skinning and offsets at six points of the frame for 40 frames and logs, per bone group, at which points they
//! changed.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::bonepose::{bind_pose, inverse, mul, write_floats, IDENTITY};
use crate::livepose::{locate, matrix, Buffer};
use crate::skeleton::{read, read_vec, Bone};

/// The first bone written (RootPos): Master and the IK targets before it are left at the identity.
const FIRST: usize = 7;
const CALM_SPEED: f64 = 0.3;
const CALM_HANDS: f64 = 0.6;
const CALM_S: f32 = 0.4;
/// How often the motion is sampled (the game fills each buffer every other frame).
const SAMPLE_S: f32 = 0.2;
const BLEND_IN_S: f32 = 0.3;
const BLEND_OUT_S: f32 = 0.12;
const LOG_EVERY_S: f32 = 20.0;

/// What is shown while he stands still (the debug pose key cycles them).
const MODES: [&str; 4] = ["Kratos's idle", "Kratos's idle with the game's arms", "Elden Ring's rest pose (a check)", "off: the game's pose"];
const KRATOS: u8 = 0;
const GAME_ARMS: u8 = 1;
const REST: u8 = 2;
const OFF: u8 = 3;
static MODE: AtomicU8 = AtomicU8::new(KRATOS);
static STATE: Mutex<Option<State>> = Mutex::new(None);
static LOADED: AtomicBool = AtomicBool::new(false);

/// Go to the next mode; its name.
pub fn cycle() -> &'static str {
    let m = (MODE.load(Ordering::Relaxed) + 1) % MODES.len() as u8;
    MODE.store(m, Ordering::Relaxed);
    MODES[m as usize]
}

struct Clip {
    name: String,
    fps: f32,
    frames: usize,
    /// frames x bones x 12 (row-major 3x4 skinning in model space).
    data: Vec<f32>,
}

struct State {
    clip: Clip,
    bones: usize,
    bind: Vec<[[f64; 4]; 4]>,
    hands: [usize; 2],
    /// The bones of both arms (the clavicles and all below them).
    arms: Vec<bool>,
    /// The modes already traced, the mode last frame and how long it has been shown in full since.
    traced: u8,
    last_mode: u8,
    full_s: f32,
    /// Located buffers per offset array (None: not found there).
    buffers: Vec<(usize, Option<Buffer>)>,
    last: Option<Instant>,
    /// The last motion sample: when, the root's world position and the hands in his own frame.
    sample: Option<(Instant, [f64; 3], [[f64; 3]; 2])>,
    calm_for: f32,
    weight: f32,
    time: f32,
    /// Whether the last frame wrote the pose (so stopping restores the offsets once).
    wrote: bool,
    // for the log
    shown_s: f32,
    starts: u32,
    writes: u64,
    skipped: u64,
    t: f32,
    next_log: f32,
}

/// The moves file (`gowring_gow::kratos_anim::moves_file`): its first clip, if it is for this skeleton.
fn load(bones: &[Bone]) -> Result<Clip, String> {
    let path = crate::paths::get().home.join("mod").join("parts").join("kratos_moves.bin");
    let b = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
    if b.get(..4) != Some(b"GRMV") || u32_at(4) != Some(1) {
        return Err("not a version 1 moves file".into());
    }
    let (n, clips) = (u32_at(8).unwrap_or(0) as usize, u32_at(12).unwrap_or(0) as usize);
    if n != bones.len() {
        return Err(format!("made for {n} bones, the skeleton has {}", bones.len()));
    }
    if clips == 0 {
        return Err("no clips in it (the launcher could not build them)".into());
    }
    let len = u32_at(16).ok_or("cut short")? as usize;
    let name = String::from_utf8_lossy(b.get(20..20 + len).ok_or("cut short")?).into_owned();
    let o = 20 + len;
    let fps = f32::from_le_bytes(b.get(o..o + 4).ok_or("cut short")?.try_into().unwrap());
    let frames = u32_at(o + 4).ok_or("cut short")? as usize;
    let floats = frames * n * 12;
    let raw = b.get(o + 8..o + 8 + 4 * floats).ok_or("cut short")?;
    let data: Vec<f32> = raw.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    if frames == 0 || !(1.0..=240.0).contains(&fps) || data.iter().any(|x| !x.is_finite()) {
        return Err(format!("a clip of {frames} frames at {fps} fps, or with bad values"));
    }
    Ok(Clip { name, fps, frames, data })
}

/// Bone `b`'s joint in his own frame (the root's skinning undone) in skinning `y`.
fn own_joint(y: &[f32], bind: &[[[f64; 4]; 4]], b: usize) -> Option<[f64; 3]> {
    let w = mul(&inverse(&matrix(&y[..12]))?, &mul(&matrix(&y[b * 12..b * 12 + 12]), &bind[b]));
    Some([w[0][3], w[1][3], w[2][3]])
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Called just before drawing, every frame, after the cape.
pub fn step() {
    let Some(bones) = crate::skeleton::bones() else { return };
    if !LOADED.swap(true, Ordering::Relaxed) {
        match load(bones) {
            Ok(clip) => {
                crate::glog!("moves", "Kratos's {} ({} frames at {} fps) for {} bones; shown while he stands still", clip.name, clip.frames, clip.fps, bones.len());
                let index = |n: &str| bones.iter().position(|b| b.name == n);
                let (Some(l), Some(r)) = (index("L_Hand"), index("R_Hand")) else {
                    crate::glog!("moves", "no hands in the skeleton; his moves stay off");
                    return;
                };
                let mut arms = vec![false; bones.len()];
                for (i, b) in bones.iter().enumerate() {
                    let p = usize::try_from(b.parent).ok().filter(|p| *p < i);
                    arms[i] = b.name == "L_Clavicle" || b.name == "R_Clavicle" || p.is_some_and(|p| arms[p]);
                }
                if let Ok(mut s) = STATE.lock() {
                    *s = Some(State { clip, bones: bones.len(), bind: bind_pose(bones), hands: [l, r], arms, traced: 0, last_mode: KRATOS, full_s: 0.0, buffers: Vec::new(), last: None, sample: None, calm_for: 0.0, weight: 0.0, time: 0.0, wrote: false, shown_s: 0.0, starts: 0, writes: 0, skipped: 0, t: 0.0, next_log: LOG_EVERY_S });
                }
            }
            Err(e) => crate::glog!("moves", "Kratos's moves are off: {e}"),
        }
    }
    let Ok(mut guard) = STATE.try_lock() else { return };
    let Some(s) = guard.as_mut() else { return };
    let now = Instant::now();
    let dt = s.last.map_or(0.0, |l| now.duration_since(l).as_secs_f32()).min(0.1);
    s.last = Some(now);
    s.t += dt;
    // the game's buffers, found once per offset array
    let arrays = crate::cape::offset_arrays();
    if arrays.is_empty() {
        crate::log::once("moves-no-offsets", "moves", "no offset arrays yet (the cape finds them); his moves wait");
        return;
    }
    if s.buffers.iter().map(|b| b.0).ne(arrays.iter().copied()) {
        s.buffers = arrays.iter().map(|a| (*a, locate(*a, bones, &s.bind))).collect();
        let found: Vec<String> = s.buffers.iter().map(|(a, b)| b.map_or(format!("{a:#x}: none"), |b| format!("{a:#x}: skinning at {:#x}, {} bones", b.skinning, b.bones))).collect();
        crate::glog!("moves", "the game's pose by its offsets: {}", found.join("; "));
        s.wrote = false;
    }
    let n = s.bones;
    // only the model's own buffers (the most bones: the skeleton's and the model's extras). 0.1.92 found three more
    // by 150-bone blocks that the game updated once or never in 40 frames (stale, or another model's), and wrote 143
    // matrices a frame into them too until the game closed by itself at 3:05
    let most = s.buffers.iter().filter_map(|b| b.1).map(|b| b.bones).max().unwrap_or(0);
    let buffers: Vec<Buffer> = s.buffers.iter().filter_map(|b| b.1).filter(|b| b.bones == most && most > n).collect();
    let Some(first) = buffers.first().copied() else { return };
    // standing still? sampled every SAMPLE_S from the first buffer
    if let Some(y) = read_vec::<f32>(first.skinning, n * 12).filter(|y| y.iter().all(|x| x.is_finite())) {
        let root = [y[3] as f64, y[7] as f64, y[11] as f64];
        let hands = [own_joint(&y, &s.bind, s.hands[0]), own_joint(&y, &s.bind, s.hands[1])];
        if let [Some(l), Some(r)] = hands {
            match s.sample {
                Some((at, p, h)) if now.duration_since(at).as_secs_f32() >= SAMPLE_S => {
                    let span = now.duration_since(at).as_secs_f64();
                    let calm = dist(root, p) / span < CALM_SPEED && dist(l, h[0]) / span < CALM_HANDS && dist(r, h[1]) / span < CALM_HANDS;
                    s.calm_for = if calm { s.calm_for + span as f32 } else { 0.0 };
                    s.sample = Some((now, root, [l, r]));
                }
                None => s.sample = Some((now, root, [l, r])),
                _ => {}
            }
        }
    }
    let mode = MODE.load(Ordering::Relaxed);
    let on = mode != OFF && s.calm_for >= CALM_S;
    let before = s.weight;
    s.weight = if on { (s.weight + dt / BLEND_IN_S).min(1.0) } else { (s.weight - dt / BLEND_OUT_S).max(0.0) };
    if before == 0.0 && s.weight > 0.0 {
        s.time = 0.0;
        s.starts += 1;
    }
    if s.weight > 0.0 {
        s.shown_s += dt;
        s.time = (s.time + dt) % (s.clip.frames as f32 / s.clip.fps);
    }
    let cape: Vec<(usize, [f32; 12])> = crate::cape::current_offsets();
    if s.weight == 0.0 {
        // back to the game's pose once: the identity, or the cape's offset on its bones
        if s.wrote {
            for b in &buffers {
                let mut v = Vec::with_capacity((n - FIRST) * 12);
                for bone in FIRST..n {
                    v.extend_from_slice(cape.iter().find(|c| c.0 == bone).map_or(&IDENTITY, |c| &c.1));
                }
                if write_floats(b.offsets + FIRST * 48, &v) {
                    s.writes += 1;
                }
            }
            s.wrote = false;
        }
    } else {
        // the clip's frame (between two, looped) and the offsets that show it
        let f = s.time * s.clip.fps;
        let (a, frac) = (f.floor() as usize % s.clip.frames, f.fract());
        let b2 = (a + 1) % s.clip.frames;
        let data = &s.clip.data;
        let k: Vec<[f32; 12]> = (0..n)
            .map(|bone| {
                if mode == REST {
                    return IDENTITY;
                }
                let (x, y) = (&data[(a * n + bone) * 12..(a * n + bone) * 12 + 12], &data[(b2 * n + bone) * 12..(b2 * n + bone) * 12 + 12]);
                std::array::from_fn(|i| x[i] * (1.0 - frac) + y[i] * frac)
            })
            .collect();
        let w = s.weight as f64;
        for b in &buffers {
            let Some(y) = read_vec::<f32>(b.skinning, n * 12).filter(|y| y.iter().all(|x| x.is_finite())) else {
                s.skipped += 1;
                continue;
            };
            // the array must still read as the offsets (the cape checks the same before its writes)
            if !read::<[f32; 12]>(b.offsets).is_some_and(|e| e == IDENTITY) {
                s.skipped += 1;
                continue;
            }
            let c = matrix(&y[..12]);
            let mut v = Vec::with_capacity((n - FIRST) * 12);
            let mut ok = true;
            for bone in FIRST..n {
                if mode == GAME_ARMS && s.arms[bone] {
                    v.extend_from_slice(&IDENTITY);
                    continue;
                }
                let Some(yi) = inverse(&matrix(&y[bone * 12..bone * 12 + 12])) else {
                    ok = false;
                    break;
                };
                let mut o = mul(&yi, &mul(&c, &matrix(&k[bone])));
                if let Some(cp) = cape.iter().find(|c| c.0 == bone) {
                    o = mul(&o, &matrix(&cp.1));
                }
                for r in 0..3 {
                    for col in 0..4 {
                        let id = if r == col { 1.0 } else { 0.0 };
                        v.push((id + w * (o[r][col] - id)) as f32);
                    }
                }
            }
            if ok && v.iter().all(|x| x.is_finite()) && write_floats(b.offsets + FIRST * 48, &v) {
                s.writes += 1;
                s.wrote = true;
            } else {
                s.skipped += 1;
            }
        }
    }
    // the trace, once per mode, after it has been shown in full for a second
    if mode != s.last_mode {
        s.last_mode = mode;
        s.full_s = 0.0;
    }
    s.full_s = if s.weight >= 1.0 { s.full_s + dt } else { 0.0 };
    if s.full_s >= 1.0 && s.traced & (1 << mode) == 0 && !TRACING.load(Ordering::Relaxed) {
        s.traced |= 1 << mode;
        start_trace(&buffers, bones, MODES[mode as usize]);
    }
    if s.t >= s.next_log {
        s.next_log = s.t + LOG_EVERY_S;
        crate::glog!(
            "moves",
            "{}: shown {:.1} s ({} times) since the start, weight now {:.2}, {} still; {} writes, {} skipped; {}",
            s.clip.name,
            s.shown_s,
            s.starts,
            s.weight,
            if s.calm_for >= CALM_S { "standing" } else { "not standing" },
            s.writes,
            s.skipped,
            MODES[mode as usize]
        );
    }
}

/// The points of the frame the trace reads at (see `boot`).
const POINTS: [&str; 6] = ["FrameBegin", "PreBehavior", "PostPhysics", "Draw_Pre", "Draw_Post", "FrameEnd"];
const GROUPS: [(&str, &[&str]); 5] = [
    ("body", &["Pelvis", "Spine2", "Head"]),
    ("arms", &["L_UpperArm", "L_Forearm", "R_UpperArm", "R_Forearm"]),
    ("hands", &["L_Hand", "L_Finger1", "L_Finger11", "R_Hand", "R_Finger1", "R_Finger11"]),
    ("weapons", &["L_Weapon", "R_Weapon"]),
    ("IK targets", &["L_Hand_Target", "R_Hand_Target"]),
];
const TRACE_FRAMES: u32 = 40;

static TRACING: AtomicBool = AtomicBool::new(false);
static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

struct Trace {
    label: String,
    buffers: Vec<Buffer>,
    n: usize,
    groups: Vec<Vec<usize>>,
    /// Per buffer: the skinning and offsets as last read.
    last: Vec<Option<(Vec<f32>, Vec<f32>)>>,
    /// Per buffer, per point, per group: the reads in which the skinning (and the offsets) had changed since the read
    /// before.
    skin: Vec<[[u32; 5]; 6]>,
    offs: Vec<[[u32; 5]; 6]>,
    frames: u32,
}

fn start_trace(buffers: &[Buffer], bones: &[Bone], label: &str) {
    let groups = GROUPS.iter().map(|(_, names)| names.iter().filter_map(|n| bones.iter().position(|b| b.name == *n)).collect()).collect();
    if let Ok(mut t) = TRACE.lock() {
        *t = Some(Trace { label: label.into(), buffers: buffers.to_vec(), n: bones.len(), groups, last: vec![None; buffers.len()], skin: vec![[[0; 5]; 6]; buffers.len()], offs: vec![[[0; 5]; 6]; buffers.len()], frames: 0 });
        TRACING.store(true, Ordering::Relaxed);
    }
}

/// Called at each of [`POINTS`] (its index) every frame; reads only while a trace runs.
pub fn trace(point: usize) {
    if !TRACING.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut guard) = TRACE.try_lock() else { return };
    let Some(t) = guard.as_mut() else { return };
    let n = t.n;
    for (i, b) in t.buffers.iter().enumerate() {
        let (Some(y), Some(o)) = (read_vec::<f32>(b.skinning, n * 12), read_vec::<f32>(b.offsets, n * 12)) else { continue };
        if let Some((py, po)) = &t.last[i] {
            for (g, bones) in t.groups.iter().enumerate() {
                let changed = |a: &[f32], c: &[f32]| bones.iter().any(|k| a[k * 12..k * 12 + 12].iter().zip(&c[k * 12..k * 12 + 12]).any(|(x, z)| (x - z).abs() > 1e-6));
                if changed(&y, py) {
                    t.skin[i][point][g] += 1;
                }
                if changed(&o, po) {
                    t.offs[i][point][g] += 1;
                }
            }
        }
        t.last[i] = Some((y, o));
    }
    if point == POINTS.len() - 1 {
        t.frames += 1;
        if t.frames >= TRACE_FRAMES {
            let mut lines = Vec::new();
            for (i, b) in t.buffers.iter().enumerate() {
                let fmt = |c: &[[u32; 5]; 6]| GROUPS.iter().enumerate().map(|(g, (name, _))| format!("{name} {}", (0..6).map(|p| c[p][g].to_string()).collect::<Vec<_>>().join("/"))).collect::<Vec<_>>().join(", ");
                lines.push(format!("buffer {} ({} bones, skinning {:#x}): skinning changed {}; offsets changed {}", i + 1, b.bones, b.skinning, fmt(&t.skin[i]), fmt(&t.offs[i])));
            }
            crate::glog!("moves", "trace while showing {} ({} frames; counts at {}):\n    {}", t.label, t.frames, POINTS.join("/"), lines.join("\n    "));
            TRACING.store(false, Ordering::Relaxed);
            *guard = None;
        }
    }
}

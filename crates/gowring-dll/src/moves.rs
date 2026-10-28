//! Kratos's own moves in play (step 3 of giving him his movement): his Ragnarök idle, walk, run and sprint, decoded and
//! retargeted onto the player skeleton by the launcher (`gowring_gow::kratos_anim`, `kratos_moves.bin` beside the
//! parts), shown over the game's pose.
//!
//! Each frame, just before drawing (after the cape), for each of the model's two buffers: the game's skinning `Y`
//! (`livepose`, just before the offsets) and the clip's skinning in model space `K`; the character is where the
//! root's skinning puts him (`C = Y[Master]`), so the skinning wanted is `C x K` and the offset that shows it, in rest
//! space under the game's pose, is `Y^-1 x C x K` (for the cape's bones, times the cape's own offset on top). The
//! bones from RootPos up are written in one write per buffer; Master and the feet's IK targets are left alone (the
//! cape checks them before writing).
//!
//! When (`choose`): his walk, run or sprint (the one nearest the player's speed, played at that speed over its own)
//! while the game plays a forward walk, run or dash loop; else his idle while the player stands still (his root under
//! 0.3 m/s and his hands under 0.6 m/s in his own frame, over the last 0.4 s); else the game's pose (rolls, attacks,
//! jumps, anything else). In over 0.3 s, out over 0.12 s, a 0.25 s fade between clips (the stride carried over
//! between walk, run and sprint).
//!
//! 0.1.91-0.1.92 in play: the hands came out stretched flat; that was the retargeting's fingers (fixed in 0.1.93,
//! `kratos_anim`), and the axe floats (the game places weapons from its own hand). The debug pose key cycles: Kratos's
//! moves; them with the game's own arms (the hands and the weapon where the game puts them); Elden Ring's rest pose (a
//! check that every bone takes the offsets); off. The first time each is shown, a trace reads the game's skinning
//! and offsets at six points of the frame for 40 frames and logs, per bone group, at which points they changed
//! (0.1.92: the game writes both at ChrIns_PostPhysics, nothing after the mod).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::bonepose::{bind_pose, inverse, mul, write_floats, IDENTITY};
use crate::livepose::{bone_count, locate, matrix, Buffer};
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
/// His walk, run and sprint show while the game plays a walk, run or dash loop (its moves 020xxx and 022xxx: rolls
/// are 027xxx, jumps 202xxx, attacks 03xxxx) at this speed along the ground or more, the way he faces (the travel at
/// most about 45 degrees off his forward, model space -z, his clips' own).
const LOCO_SPEED: f64 = 0.4;
const LOCO_MOVES: [i32; 2] = [20, 22];
const FORWARD: f64 = 0.7;
/// Once in one of them: kept while the travel stays within about 70 degrees of his forward (a turn while running
/// drags it off for a moment; 0.1.90's reads at 3.5-7 m/s ran at 0.98-1.00 along -z), and another taken only when it
/// is this much nearer his speed (m/s), so neither flickers.
const FORWARD_KEEP: f64 = 0.35;
const SPEED_MARGIN: f64 = 0.3;
/// Between two clips; a walk, run or sprint plays at the player's speed over its own, within these.
const FADE_S: f32 = 0.25;
const RATE: (f32, f32) = (0.7, 1.5);
const LOG_EVERY_S: f32 = 20.0;

/// What is shown (the debug pose key cycles them).
const MODES: [&str; 4] = ["Kratos's moves", "Kratos's moves with the game's arms", "Elden Ring's rest pose (a check)", "off: the game's pose"];
const KRATOS: u8 = 0;
const GAME_ARMS: u8 = 1;
const REST: u8 = 2;
const OFF: u8 = 3;
static MODE: AtomicU8 = AtomicU8::new(KRATOS);
static STATE: Mutex<Option<State>> = Mutex::new(None);
static LOADED: AtomicBool = AtomicBool::new(false);
/// The animation the game plays on the player (`aCCC_MMMMMM` as CCC x 1,000,000 + MMMMMM), from the rules tick.
static GAME_ANIM: AtomicI32 = AtomicI32::new(-1);

/// Go to the next mode; its name.
pub fn cycle() -> &'static str {
    let m = (MODE.load(Ordering::Relaxed) + 1) % MODES.len() as u8;
    MODE.store(m, Ordering::Relaxed);
    MODES[m as usize]
}

/// The game's current animation on the player (see `host`).
pub fn set_game_anim(id: i32) {
    GAME_ANIM.store(id, Ordering::Relaxed);
}

struct Clip {
    name: String,
    fps: f32,
    /// How fast his root moves over it (m/s; 0 for the idle).
    speed: f32,
    frames: usize,
    /// frames x bones x 12 (row-major 3x4 skinning in model space).
    data: Vec<f32>,
}

impl Clip {
    /// A loop: its last frame is its first.
    fn length(&self) -> f32 {
        (self.frames.max(2) - 1) as f32 / self.fps
    }

    /// The skinning of each bone at `phase` (0-1 over the loop), between the two frames around it.
    fn pose(&self, phase: f32, n: usize) -> Vec<[f32; 12]> {
        let f = phase.rem_euclid(1.0) * (self.frames.max(2) - 1) as f32;
        let a = (f.floor() as usize).min(self.frames - 1);
        let (b, t) = ((a + 1).min(self.frames - 1), f - a as f32);
        (0..n)
            .map(|bone| {
                let (x, y) = (&self.data[(a * n + bone) * 12..(a * n + bone) * 12 + 12], &self.data[(b * n + bone) * 12..(b * n + bone) * 12 + 12]);
                std::array::from_fn(|i| x[i] * (1.0 - t) + y[i] * t)
            })
            .collect()
    }
}

struct State {
    clips: Vec<Clip>,
    idle: Option<usize>,
    /// The walk, run and sprint, slowest first.
    loco: Vec<usize>,
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
    /// From the last sample: his speed along the ground, and how close to his forward it is (cosine).
    speed: f64,
    forward: f64,
    weight: f32,
    /// The clip shown and where in it (0-1); the one it fades from, where in that, and how far the fade is.
    current: Option<usize>,
    phase: f32,
    previous: Option<(usize, f32)>,
    fade: f32,
    /// Whether the last frame wrote the pose (so stopping restores the offsets once).
    wrote: bool,
    // for the log
    shown_s: Vec<f32>,
    starts: u32,
    writes: u64,
    skipped: u64,
    t: f32,
    next_log: f32,
    relocate_at: f32,
}

/// The moves file (`gowring_gow::kratos_anim::moves_file`, version 1 or 2): its clips, if it is for this skeleton.
fn load(bones: &[Bone]) -> Result<Vec<Clip>, String> {
    let path = crate::paths::get().home.join("mod").join("parts").join("kratos_moves.bin");
    let b = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
    let f32_at = |o: usize| b.get(o..o + 4).map(|s| f32::from_le_bytes(s.try_into().unwrap()));
    let version = u32_at(4).unwrap_or(0);
    if b.get(..4) != Some(b"GRMV") || !(1..=2).contains(&version) {
        return Err("not a version 1 or 2 moves file".into());
    }
    let (n, count) = (u32_at(8).unwrap_or(0) as usize, u32_at(12).unwrap_or(0) as usize);
    if n != bones.len() {
        return Err(format!("made for {n} bones, the skeleton has {}", bones.len()));
    }
    if count == 0 {
        return Err("no clips in it (the launcher could not build them)".into());
    }
    let mut clips = Vec::new();
    let mut o = 16;
    for _ in 0..count.min(16) {
        let len = u32_at(o).ok_or("cut short")? as usize;
        let name = String::from_utf8_lossy(b.get(o + 4..o + 4 + len).ok_or("cut short")?).into_owned();
        o += 4 + len;
        let fps = f32_at(o).ok_or("cut short")?;
        o += 4;
        let speed = if version >= 2 {
            o += 4;
            f32_at(o - 4).ok_or("cut short")?
        } else {
            0.0
        };
        let frames = u32_at(o).ok_or("cut short")? as usize;
        o += 4;
        let floats = frames * n * 12;
        let raw = b.get(o..o + 4 * floats).ok_or("cut short")?;
        o += 4 * floats;
        let data: Vec<f32> = raw.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        if frames < 2 || !(1.0..=240.0).contains(&fps) || !(0.0..=20.0).contains(&speed) || data.iter().any(|x| !x.is_finite()) {
            return Err(format!("{name}: {frames} frames at {fps} fps and {speed} m/s, or bad values"));
        }
        clips.push(Clip { name, fps, speed, frames, data });
    }
    Ok(clips)
}

/// Bone `b`'s joint in his own frame (the root's skinning undone) in skinning `y`.
fn own_joint(y: &[f32], bind: &[[[f64; 4]; 4]], b: usize) -> Option<[f64; 3]> {
    let w = mul(&inverse(&matrix(&y[..12]))?, &mul(&matrix(&y[b * 12..b * 12 + 12]), &bind[b]));
    Some([w[0][3], w[1][3], w[2][3]])
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The clip to show now, if any: a walk, run or sprint (by speed, the nearest) while the game plays a forward walk,
/// run or dash loop; else the idle while he stands still.
fn choose(s: &State, mode: u8) -> Option<usize> {
    if mode == OFF {
        return None;
    }
    let anim = GAME_ANIM.load(Ordering::Relaxed);
    let moving = anim >= 0 && LOCO_MOVES.contains(&((anim % 1_000_000) / 1000));
    let off = |c: usize| (s.clips[c].speed as f64 - s.speed).abs();
    let shown = s.current.filter(|c| s.loco.contains(c) && s.weight > 0.0);
    let forward = if shown.is_some() { FORWARD_KEEP } else { FORWARD };
    if mode != REST && moving && s.speed >= LOCO_SPEED && s.forward >= forward && !s.loco.is_empty() {
        let nearest = s.loco.iter().copied().min_by(|a, b| off(*a).total_cmp(&off(*b)))?;
        return Some(match shown {
            Some(c) if off(c) <= off(nearest) + SPEED_MARGIN => c,
            _ => nearest,
        });
    }
    (s.calm_for >= CALM_S).then_some(s.idle).flatten()
}

/// Called just before drawing, every frame, after the cape.
pub fn step() {
    let Some(bones) = crate::skeleton::bones() else { return };
    if !LOADED.swap(true, Ordering::Relaxed) {
        match load(bones) {
            Ok(clips) => {
                let list: Vec<String> = clips.iter().map(|c| format!("{} ({} frames at {} fps, {:.2} m/s)", c.name, c.frames, c.fps, c.speed)).collect();
                crate::glog!("moves", "Kratos's moves for {} bones: {}", bones.len(), list.join(", "));
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
                let idle = clips.iter().position(|c| c.name == "idle");
                let mut loco: Vec<usize> = (0..clips.len()).filter(|i| clips[*i].speed > 0.1).collect();
                loco.sort_by(|a, b| clips[*a].speed.total_cmp(&clips[*b].speed));
                let shown_s = vec![0.0; clips.len()];
                if let Ok(mut s) = STATE.lock() {
                    *s = Some(State {
                        clips,
                        idle,
                        loco,
                        bones: bones.len(),
                        bind: bind_pose(bones),
                        hands: [l, r],
                        arms,
                        traced: 0,
                        last_mode: KRATOS,
                        full_s: 0.0,
                        buffers: Vec::new(),
                        last: None,
                        sample: None,
                        calm_for: 0.0,
                        speed: 0.0,
                        forward: 0.0,
                        weight: 0.0,
                        current: None,
                        phase: 0.0,
                        previous: None,
                        fade: 1.0,
                        wrote: false,
                        shown_s,
                        starts: 0,
                        writes: 0,
                        skipped: 0,
                        t: 0.0,
                        next_log: LOG_EVERY_S,
                        relocate_at: 0.0,
                    });
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
    // found again when the arrays change, and every 2 s while none reads as the model's own (more bones than the
    // skeleton: its extras); the bone count from the gap between the two buffers (0.1.94 and 0.1.95 searched for it,
    // and took blocks 31 and 14 entries short just after loading)
    let unsure = !s.buffers.iter().any(|b| b.1.is_some_and(|b| b.bones > bones.len()));
    if s.buffers.iter().map(|b| b.0).ne(arrays.iter().copied()) || (unsure && s.t >= s.relocate_at) {
        s.relocate_at = s.t + 2.0;
        let count = bone_count(&arrays);
        s.buffers = arrays.iter().map(|a| (*a, locate(*a, count, bones, &s.bind))).collect();
        let found: Vec<String> = s.buffers.iter().map(|(a, b)| b.map_or(format!("{a:#x}: none"), |b| format!("{a:#x}: skinning at {:#x}, {} bones", b.skinning, b.bones))).collect();
        let by = count.map_or("searched (not two buffers a whole number of entries apart)".to_string(), |n| format!("{n} bones by the buffers' gap"));
        crate::glog!("moves", "the game's pose by its offsets ({by}): {}", found.join("; "));
        s.wrote = false;
    }
    let n = s.bones;
    // only the model's own buffers (the most bones: the skeleton's and the model's extras). 0.1.92 found three more
    // by 150-bone blocks that the game updated once or never in 40 frames (stale, or another model's), and wrote 143
    // matrices a frame into them too until the game closed by itself at 3:05
    let most = s.buffers.iter().filter_map(|b| b.1).map(|b| b.bones).max().unwrap_or(0);
    let buffers: Vec<Buffer> = s.buffers.iter().filter_map(|b| b.1).filter(|b| b.bones == most && most > n).collect();
    let Some(first) = buffers.first().copied() else { return };
    // how he moves, sampled every SAMPLE_S from the first buffer: standing still (his root and hands), his speed along
    // the ground and its direction in his own frame
    if let Some(y) = read_vec::<f32>(first.skinning, n * 12).filter(|y| y.iter().all(|x| x.is_finite())) {
        let root = [y[3] as f64, y[7] as f64, y[11] as f64];
        let hands = [own_joint(&y, &s.bind, s.hands[0]), own_joint(&y, &s.bind, s.hands[1])];
        if let [Some(l), Some(r)] = hands {
            match s.sample {
                Some((at, p, h)) if now.duration_since(at).as_secs_f32() >= SAMPLE_S => {
                    let span = now.duration_since(at).as_secs_f64();
                    let calm = dist(root, p) / span < CALM_SPEED && dist(l, h[0]) / span < CALM_HANDS && dist(r, h[1]) / span < CALM_HANDS;
                    s.calm_for = if calm { s.calm_for + span as f32 } else { 0.0 };
                    // the travel into his frame (the root's skinning turned back; a scale in it does not matter)
                    let v = [root[0] - p[0], root[1] - p[1], root[2] - p[2]];
                    let c = matrix(&y[..12]);
                    let local: [f64; 3] = std::array::from_fn(|k| (0..3).map(|r| c[r][k] * v[r]).sum::<f64>());
                    let ground = (local[0] * local[0] + local[2] * local[2]).sqrt();
                    s.speed = dist([v[0], 0.0, v[2]], [0.0; 3]) / span;
                    s.forward = if ground > 1e-6 { -local[2] / ground } else { 0.0 };
                    s.sample = Some((now, root, [l, r]));
                }
                None => s.sample = Some((now, root, [l, r])),
                _ => {}
            }
        }
    }
    let mode = MODE.load(Ordering::Relaxed);
    let target = choose(s, mode);
    let before = s.weight;
    s.weight = if target.is_some() { (s.weight + dt / BLEND_IN_S).min(1.0) } else { (s.weight - dt / BLEND_OUT_S).max(0.0) };
    if let Some(t) = target {
        if before == 0.0 || s.current.is_none() {
            // a fresh start: straight into the clip
            s.current = Some(t);
            s.phase = 0.0;
            s.previous = None;
            s.fade = 1.0;
            s.starts += 1;
        } else if s.current != Some(t) {
            // into another clip: fade from the one shown; between his walk, run and sprint the stride carries on
            let from = s.current.unwrap_or(t);
            let both_loco = s.loco.contains(&from) && s.loco.contains(&t);
            s.previous = Some((from, s.phase));
            s.fade = 0.0;
            s.current = Some(t);
            if !both_loco {
                s.phase = 0.0;
            }
        }
    }
    let rate = |c: &Clip, speed: f64| if c.speed > 0.1 { ((speed / c.speed as f64) as f32).clamp(RATE.0, RATE.1) } else { 1.0 };
    if s.weight > 0.0 {
        if let Some(c) = s.current {
            s.shown_s[c] += dt;
            s.phase = (s.phase + dt * rate(&s.clips[c], s.speed) / s.clips[c].length()).rem_euclid(1.0);
        }
        if let Some((p, ph)) = s.previous {
            s.previous = Some((p, (ph + dt * rate(&s.clips[p], s.speed) / s.clips[p].length()).rem_euclid(1.0)));
            s.fade = (s.fade + dt / FADE_S).min(1.0);
            if s.fade >= 1.0 {
                s.previous = None;
            }
        }
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
        s.current = None;
    } else if let Some(cur) = s.current {
        // the clip's pose (faded from the one before), and the offsets that show it
        let k: Vec<[f32; 12]> = if mode == REST {
            vec![IDENTITY; n]
        } else {
            let now_pose = s.clips[cur].pose(s.phase, n);
            match s.previous {
                Some((p, ph)) => {
                    let was = s.clips[p].pose(ph, n);
                    let t = s.fade;
                    now_pose.iter().zip(&was).map(|(a, b)| std::array::from_fn(|i| b[i] * (1.0 - t) + a[i] * t)).collect()
                }
                None => now_pose,
            }
        };
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
        let shown: Vec<String> = s.clips.iter().zip(&s.shown_s).map(|(c, t)| format!("{} {t:.1} s", c.name)).collect();
        let anim = GAME_ANIM.load(Ordering::Relaxed);
        crate::glog!(
            "moves",
            "shown since the start: {} ({} starts); now {} at weight {:.2}; the game plays a{:03}_{:06}, he moves {:.2} m/s ({:.2} forward), {}; {} writes, {} skipped; {}",
            shown.join(", "),
            s.starts,
            s.current.map_or("nothing", |c| s.clips[c].name.as_str()),
            s.weight,
            anim.max(0) / 1_000_000,
            anim.max(0) % 1_000_000,
            s.speed,
            s.forward,
            if s.calm_for >= CALM_S { "standing still" } else { "not standing still" },
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

//! Elden Ring's live animated pose: step 2 of giving Kratos his own movement, found in 0.1.89's dumps offline and
//! checked here in play.
//!
//! What the dumps showed (MODLOG, 0.1.89): each offset array the cape writes (`bonepose`) is the last of three arrays
//! of one row-major 3x4 per model bone, back to back (181 bones for the player: the Havok skeleton's 150 in its order,
//! then 31 of the model's own), and the game keeps two such buffers. The middle array is the game's skinning: per
//! bone, the world transform times the inverse of the bind pose (the model's bind equals the one built from the
//! skeleton, for all 150 bones). Read that way it puts 74 of the 78 links whose two entries differ at their rest
//! offset (Pelvis and RootRotY carry animated translation, the SpineArmor bones the armour's), the left hand 0.54 m
//! from its bind position (the two-handed idle), and the cape's bones exactly on their parents' while the cape's
//! offsets turn them 3 to 5 degrees: the offsets go on after it, so the offset that shows a pose is this array's
//! entry inverted times the pose's skinning. The searches missed it because a run of good matrices read the end of
//! the first array and this one as one, and because bone 84 (L_Pectoral) carries the body's scale, which their 2 %
//! unit-row test refused.
//!
//! The check: for each offset array, the block just before it at the smallest bone count at which it reads as
//! skinning; then 40 reads, 100 ms apart, of the three arrays of each buffer while the player moves, with the cape
//! switched off for the middle second (its offsets go back to the identity; the skinning should not change at its
//! bones). The log gives the bone count, the links at rest per snapshot, the main joints in his own frame and how far
//! they went, and the cape check; `%LOCALAPPDATA%\GowRing\er_pose_live_<n>.bin` keeps all of it for the retargeting
//! (`tools/ragnarok_anim/er_pose_live.py`). It only reads, but for the cape switch (what the cape's debug key does).
//! It runs by itself 30, 90 and 150 s into the world, and on the debug pose key.

use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::bonepose::{bind_pose, inverse, mul, offsets_like, skin_match, IDENTITY};
use crate::skeleton::{read, read_vec, Bone};

/// Seconds into the world at which a round runs by itself.
pub const AT_S: [f32; 3] = [30.0, 90.0, 150.0];
/// The wait between the toast and the first read, for the player to start moving.
const LEAD: Duration = Duration::from_secs(2);
const SNAPSHOTS: usize = 40;
const GAP: Duration = Duration::from_millis(100);
/// The snapshots taken with the cape switched off (the middle second).
const CAPE_OFF: std::ops::Range<usize> = 15..25;
/// The model's bone count is looked for from the skeleton's up to this.
const MAX_BONES: usize = 400;
/// A block reads as the skinning when this share of its links whose two entries differ (at least `MIN_LINKS` of
/// them) sit at their rest offset; links between equal entries pass whatever the block is (0.1.89: 74 of 78, 29 equal).
const MATCH: f64 = 0.8;
const MIN_LINKS: usize = 30;
/// The joints the log follows, in his own frame.
const JOINTS: [&str; 9] = ["Pelvis", "Spine2", "Head", "L_UpperArm", "L_Hand", "R_UpperArm", "R_Hand", "L_Foot", "R_Foot"];
const ENTRY: usize = 48;

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Rounds started this session (the next round's number), and how many of `AT_S` have been passed.
static ROUNDS: AtomicUsize = AtomicUsize::new(0);
static PASSED: AtomicUsize = AtomicUsize::new(0);

/// One of the game's buffers: its skinning array, the model's bone count (each array's length) and its offsets.
struct Buffer {
    skinning: usize,
    bones: usize,
    offsets: usize,
}

/// Called every frame with the time in the world: starts a round when one of `AT_S` is passed (`model_item` is only
/// called then).
pub fn tick(in_world_s: f32, model_item: impl FnOnce() -> Option<usize>) {
    let passed = AT_S.iter().filter(|t| in_world_s >= **t).count();
    if passed > PASSED.load(Ordering::Relaxed) && !RUNNING.load(Ordering::SeqCst) {
        PASSED.store(passed, Ordering::Relaxed);
        start(model_item(), "timed");
    }
}

/// Start a round now, unless one is running (`model_item`: the player's, for finding the offsets when the cape has
/// not).
pub fn start(model_item: Option<usize>, why: &str) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        crate::overlay::toast("Pose check already running (debug)");
        return;
    }
    let round = ROUNDS.fetch_add(1, Ordering::SeqCst) + 1;
    let why = why.to_string();
    let spawned = std::thread::Builder::new().name("gowring-live-pose".into()).spawn(move || {
        if std::panic::catch_unwind(|| run(round, model_item, &why)).is_err() {
            crate::glog!("livepose", "pose check {round} panicked");
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::SeqCst);
        crate::glog!("livepose", "could not start the pose check thread: {e}");
    }
}

/// The offset arrays: the cape's, or searched for here as the cape does when it has none (no cape rig).
fn offset_arrays(model_item: Option<usize>, bones: &[Bone]) -> (Vec<usize>, &'static str) {
    let cape = crate::cape::offset_arrays();
    if !cape.is_empty() {
        return (cape, "the cape's");
    }
    let Some(m) = model_item else { return (Vec::new(), "no player model") };
    let mut roots: Vec<(&'static str, usize)> = Vec::new();
    if let Some(e) = read::<usize>(m + crate::pose::MTX43_ARRAY_ENTITY).filter(|p| *p > 0x10000) {
        roots.push(("mtx43 array entity", e));
    }
    roots.push(("model item", m));
    let (found, _) = crate::pose::find(&roots, crate::bonepose::SEARCH_READS, crate::bonepose::SEARCH_ARRAYS, crate::bonepose::SEARCH_DEPTH);
    (found.iter().filter(|f| offsets_like(f, bones)).map(|f| f.address).collect(), "searched for")
}

/// 12 floats (row-major 3x4) as a 4x4.
fn matrix(f: &[f32]) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for r in 0..3 {
        for c in 0..4 {
            m[r][c] = f[r * 4 + c] as f64;
        }
    }
    m[3][3] = 1.0;
    m
}

/// Bone `b`'s world transform in skinning `y` (its entry times its bind).
fn world(y: &[f32], bind: &[[[f64; 4]; 4]], b: usize) -> [[f64; 4]; 4] {
    mul(&matrix(&y[b * 12..b * 12 + 12]), &bind[b])
}

/// The links of skinning `y` whose parent and child entries differ, and how many of them sit at their rest offset
/// (the child's joint, in its parent's frame, where the skeleton puts it).
fn links_at_rest(y: &[f32], bind: &[[[f64; 4]; 4]], bones: &[Bone]) -> (usize, usize) {
    let (mut at_rest, mut links) = (0, 0);
    for (i, b) in bones.iter().enumerate() {
        let Some(p) = usize::try_from(b.parent).ok().filter(|p| *p < i) else { continue };
        if y[p * 12..p * 12 + 12] == y[i * 12..i * 12 + 12] {
            continue;
        }
        let Some(inv) = inverse(&world(y, bind, p)) else { continue };
        links += 1;
        let local = mul(&inv, &world(y, bind, i));
        if (0..3).map(|k| (local[k][3] - b.t[k] as f64).powi(2)).sum::<f64>().sqrt() < 0.01 {
            at_rest += 1;
        }
    }
    (at_rest, links)
}

/// The skinning before the offset array at `offsets`: the block of `n` entries just before it, for the smallest `n`
/// (the model's bone count) at which it reads as skinning. A misaligned block puts few of its differing links at rest.
fn locate(offsets: usize, bones: &[Bone], bind: &[[[f64; 4]; 4]]) -> Option<Buffer> {
    (bones.len()..=MAX_BONES).find_map(|n| {
        let at = offsets.checked_sub(n * ENTRY)?;
        let y = read_vec::<f32>(at, bones.len() * 12)?;
        if y.iter().any(|x| !x.is_finite()) {
            return None;
        }
        let (at_rest, links) = links_at_rest(&y, bind, bones);
        (links >= MIN_LINKS && at_rest as f64 >= MATCH * links as f64).then_some(Buffer { skinning: at, bones: n, offsets })
    })
}

/// One read of a buffer: the array before the skinning, the skinning and the offsets (each empty if unreadable).
fn snapshot(b: &Buffer) -> [Vec<f32>; 3] {
    let n = b.bones * 12;
    let at = |a: Option<usize>| a.and_then(|a| read_vec::<f32>(a, n)).unwrap_or_default();
    [at(b.skinning.checked_sub(b.bones * ENTRY)), at(Some(b.skinning)), at(Some(b.offsets))]
}

/// Puts the cape's swing back on when dropped (also if the round panics), if this round switched it off.
struct CapeBack(bool);

impl Drop for CapeBack {
    fn drop(&mut self) {
        if std::mem::take(&mut self.0) {
            crate::cape::set_enabled(true);
        }
    }
}

/// The largest difference between a 3x4 and the identity, and the angle it turns (degrees).
fn off_identity(m: &[f32]) -> (f32, f32) {
    let dev = m.iter().zip(IDENTITY).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    let angle = (((m[0] + m[5] + m[10] - 1.0) / 2.0).clamp(-1.0, 1.0)).acos().to_degrees();
    (dev, angle)
}

fn run(round: usize, model_item: Option<usize>, why: &str) {
    let t0 = Instant::now();
    let Some(bones) = crate::skeleton::bones() else {
        crate::glog!("livepose", "pose check {round} ({why}): the skeleton is not read yet");
        return;
    };
    let bind = bind_pose(bones);
    let (offsets, source) = offset_arrays(model_item, bones);
    let buffers: Vec<Buffer> = offsets.iter().filter_map(|o| locate(*o, bones, &bind)).collect();
    if buffers.is_empty() {
        let list: Vec<String> = offsets.iter().map(|a| format!("{a:#x}")).collect();
        crate::glog!("livepose", "pose check {round} ({why}): no skinning found before {} offset array(s) ({source}: {list:?})", offsets.len());
        return;
    }
    crate::overlay::toast(&format!("Pose check {round}: walk, run or swing for 6 seconds (debug)"));
    std::thread::sleep(LEAD);
    let mut cape = CapeBack(false);
    let mut switched = false;
    let mut snaps: Vec<Vec<[Vec<f32>; 3]>> = buffers.iter().map(|_| Vec::new()).collect();
    let mut times: Vec<u32> = Vec::new();
    let first = Instant::now();
    for k in 0..SNAPSHOTS {
        if k == CAPE_OFF.start {
            cape.0 = crate::cape::set_enabled(false);
            switched = cape.0;
        }
        if k == CAPE_OFF.end && std::mem::take(&mut cape.0) {
            crate::cape::set_enabled(true);
        }
        if k > 0 {
            std::thread::sleep(GAP);
        }
        times.push(first.elapsed().as_millis() as u32);
        for (b, s) in buffers.iter().zip(snaps.iter_mut()) {
            s.push(snapshot(b));
        }
    }
    drop(cape);
    let index = |name: &str| bones.iter().position(|b| b.name == name);
    let mut lines = Vec::new();
    for (i, (b, s)) in buffers.iter().zip(&snaps).enumerate() {
        let got: Vec<(usize, &[Vec<f32>; 3])> = s.iter().enumerate().filter(|(_, r)| r[1].len() == b.bones * 12 && r[1].iter().all(|x| x.is_finite())).collect();
        lines.push(format!("buffer {}: skinning at {:#x}, {} bones (offsets at {:#x} just after it), {} of {} snapshots read", i + 1, b.skinning, b.bones, b.offsets, got.len(), s.len()));
        let Some((_, r0)) = got.first() else { continue };
        let rest: Vec<(usize, usize)> = got.iter().map(|(_, r)| links_at_rest(&r[1], &bind, bones)).collect();
        let all: Vec<(usize, usize)> = got.iter().filter_map(|(_, r)| skin_match(&r[1], bones)).collect();
        let span = |v: &[(usize, usize)]| format!("{}..{} of {}", v.iter().map(|x| x.0).min().unwrap_or(0), v.iter().map(|x| x.0).max().unwrap_or(0), v.iter().map(|x| x.1).max().unwrap_or(0));
        let root = |y: &[f32]| world(y, &bind, 0);
        let travel = got.iter().map(|(_, r)| (0..3).map(|k| (root(&r[1])[k][3] - root(&r0[1])[k][3]).powi(2)).sum::<f64>().sqrt()).fold(0.0, f64::max);
        lines.push(format!("  links at rest: {} of the differing, {} of all; the root (Master) went up to {travel:.2} m", span(&rest), span(&all)));
        // the main joints in his own frame, in the first read, and how far each went in that frame: model space, the
        // bind's (the root's skinning undone; its bind turns it 90 degrees, so not its world transform)
        let joint = |y: &[f32], j: usize| inverse(&matrix(&y[..12])).map(|inv| mul(&inv, &world(y, &bind, j))).map(|m| [m[0][3], m[1][3], m[2][3]]);
        let mut joints = Vec::new();
        for name in JOINTS {
            let Some(j) = index(name) else { continue };
            let Some(p0) = joint(&r0[1], j) else { continue };
            let moved = got.iter().filter_map(|(_, r)| joint(&r[1], j)).map(|p| (0..3).map(|k| (p[k] - p0[k]).powi(2)).sum::<f64>().sqrt()).fold(0.0, f64::max);
            let off_bind = (0..3).map(|k| (p0[k] - bind[j][k][3]).powi(2)).sum::<f64>().sqrt();
            joints.push(format!("{name} ({:.2}, {:.2}, {:.2}) {off_bind:.2} m off bind, went {moved:.2}", p0[0], p0[1], p0[2]));
        }
        lines.push(format!("  joints: {}", joints.join("; ")));
        // the cape: its bones are those whose offset is not the identity with it on; their skinning against their
        // parent's, with it on and off
        let on = |k: usize| !CAPE_OFF.contains(&k);
        let chain: Vec<usize> = (1..bones.len()).filter(|j| got.iter().any(|(k, r)| on(*k) && r[2].len() == b.bones * 12 && off_identity(&r[2][j * 12..j * 12 + 12]).0 > 1e-4)).collect();
        if chain.is_empty() {
            lines.push("  cape: no offsets written in these reads (no cape rig, or the swing off)".to_string());
        }
        for j in chain {
            let Some(p) = usize::try_from(bones[j].parent).ok() else { continue };
            let (mut o, mut y) = ([(0.0f32, 0.0f32); 2], [0.0f64; 2]);
            for (k, r) in &got {
                let side = usize::from(!on(*k));
                if r[2].len() == b.bones * 12 {
                    let (dev, angle) = off_identity(&r[2][j * 12..j * 12 + 12]);
                    o[side] = (o[side].0.max(dev), o[side].1.max(angle));
                }
                if let Some(inv) = inverse(&matrix(&r[1][p * 12..p * 12 + 12])) {
                    let rel = mul(&inv, &matrix(&r[1][j * 12..j * 12 + 12]));
                    let dev = (0..3).flat_map(|a| (0..4).map(move |c| (a, c))).map(|(a, c)| (rel[a][c] - if a == c { 1.0 } else { 0.0 }).abs()).fold(0.0, f64::max);
                    y[side] = y[side].max(dev);
                }
            }
            lines.push(format!(
                "  cape bone {}: offset up to {:.1} deg ({:.4} off the identity) with the cape on, {:.1} deg ({:.4}) off; skinning off its parent's by {:.4} on, {:.4} off",
                bones[j].name, o[0].1, o[0].0, o[1].1, o[1].0, y[0], y[1]
            ));
        }
    }
    let file = write_dump(round, &buffers, &snaps, &times);
    crate::glog!(
        "livepose",
        "pose check {round} ({why}): {} buffer(s) from {} offset array(s) ({source}), cape {} for snapshots {}-{}, {} ms; {file}\n    {}",
        buffers.len(),
        offsets.len(),
        if switched { "switched off" } else { "left as it was (already off)" },
        CAPE_OFF.start,
        CAPE_OFF.end - 1,
        t0.elapsed().as_millis(),
        lines.join("\n    ")
    );
    crate::overlay::toast(&format!("Pose check {round} done (debug)"));
}

/// The reads, for offline work, in the format of 0.1.89's pose search (`er_pose_find.py`), version 2: "GRPF", u32 version, u32 arrays, u32 snapshots,
/// u32 first and last+1 snapshot with the cape off, u32 ms of each snapshot since the first; per array u32 path
/// length, the path, u64 address, u32 layout (1: 3x4), u32 entries, u32 floats per entry, u32 snapshots, then the
/// floats (zeros for a read that failed). Three arrays per buffer: before the skinning, the skinning, the offsets.
fn write_dump(round: usize, buffers: &[Buffer], snaps: &[Vec<[Vec<f32>; 3]>], times: &[u32]) -> String {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"GRPF");
    for v in [2u32, buffers.len() as u32 * 3, times.len() as u32, CAPE_OFF.start as u32, CAPE_OFF.end as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for t in times {
        out.extend_from_slice(&t.to_le_bytes());
    }
    for (i, (b, s)) in buffers.iter().zip(snaps).enumerate() {
        let starts = [b.skinning.saturating_sub(b.bones * ENTRY), b.skinning, b.offsets];
        for (a, (name, at)) in ["before the skinning", "skinning", "offsets"].iter().zip(starts).enumerate() {
            let path = format!("buffer {} {name}", i + 1);
            out.extend_from_slice(&(path.len() as u32).to_le_bytes());
            out.extend_from_slice(path.as_bytes());
            out.extend_from_slice(&(at as u64).to_le_bytes());
            for v in [1u32, b.bones as u32, 12, s.len() as u32] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for r in s {
                let f = &r[a];
                for k in 0..b.bones * 12 {
                    out.extend_from_slice(&f.get(k).copied().unwrap_or(0.0).to_le_bytes());
                }
            }
        }
    }
    let dir = &crate::paths::get().local;
    let path = dir.join(format!("er_pose_live_{round}.bin"));
    let wrote = std::fs::create_dir_all(dir).and_then(|_| std::fs::File::create(&path)).and_then(|mut f| f.write_all(&out));
    match wrote {
        Ok(()) => format!("reads written to {} ({} KB)", path.display(), out.len() / 1024),
        Err(e) => format!("could not write the reads: {e}"),
    }
}

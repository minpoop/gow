//! Kratos's own moves: his Ragnarök animation (CADO clips in the `ANM_heroa00` chunk of `r_heroa00.wad`), decoded on
//! the player's PC and retargeted onto the Elden Ring player skeleton, for gowring.dll to play over the game's pose
//! (`kratos_moves.bin` beside the parts).
//!
//! The format was worked out offline from the player's files (`tools/ragnarok_anim`, `NOTES.md`); this follows the
//! research decoder (`dec.py`, `anim.py`) step by step, in f64 as it does, and its test checks the result against
//! that decoder's on the same clip. In short: a clip has 8 groups of pose slots (the `HADO` object's default pose fills
//! what a clip leaves out), each group runs of sections of tracks (translations, rotations of four kinds, constants)
//! keyed per block of frames in MSB-first bitstreams; slots map onto the skeleton's joints in order, 190 procedural
//! joints left at rest.
//!
//! The retargeting: Kratos's mesh is re-posed onto the player skeleton (`crate::retarget`: each of his joints moves by
//! `moves[j]` and weighs on one Elden Ring bone). For each Elden Ring bone the anchor is the nearest of his joints that
//! weigh on it; geometry on that joint follows his animation exactly when the bone's skinning (model space) is
//! `animated(j) x bind(j)^-1 x moves[j]^-1`. The bones take that turn, and their joints follow the Elden Ring
//! hierarchy (a joint where its parent's skinning puts it; the first anchored bone of a chain where its own puts it),
//! so the limbs stay joined at the player's proportions. Bones with no anchor follow their parent. His root motion
//! (joint 5) is taken out: the game moves the character.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};

use crate::erpart::{world_matrices, Node};
use crate::kratos::Skeleton;
use crate::retarget::Retarget;

/// The file gowring.dll reads beside the parts.
pub const MOVES_FILE: &str = "kratos_moves.bin";
/// The clips built: name and CADO hash (u32 at base + 6), all in `ANM_heroa00` and picked by motion offline
/// (`survey.py`): the idle (61 frames, 2 s, the first gameplay clip), and his forward walk (1.5 m/s, 68 frames), run
/// (3.0 m/s, 45) and sprint (6.0 m/s, 36), loops.
pub const CLIPS: [(&str, u32); 4] = [("idle", 0x2950_eb3e), ("walk", 0x6290_eb3e), ("run", 0x47d0_eb3e), ("sprint", 0x4dc0_eb3e)];
/// His root-motion joint.
const ROOT_JOINT: usize = 5;

type M = [[f64; 4]; 4];
const I4: M = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
const S2: f64 = std::f64::consts::FRAC_1_SQRT_2;

fn at<const N: usize>(b: &[u8], o: usize) -> Result<[u8; N]> {
    b.get(o..o + N).and_then(|s| s.try_into().ok()).ok_or_else(|| anyhow!("read past the end at {o:#x}"))
}
fn u16_at(b: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(at(b, o)?))
}
fn i16_at(b: &[u8], o: usize) -> Result<i16> {
    Ok(i16::from_le_bytes(at(b, o)?))
}
fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(at(b, o)?))
}
fn f32_at(b: &[u8], o: usize) -> Result<f32> {
    Ok(f32::from_le_bytes(at(b, o)?))
}
fn byte(b: &[u8], o: usize) -> Result<u8> {
    b.get(o).copied().ok_or_else(|| anyhow!("read past the end at {o:#x}"))
}

/// MSB-first bit reader.
struct Bits<'a> {
    d: &'a [u8],
    p: usize,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> Result<u128> {
        if n > 128 {
            bail!("a key of {n} bits");
        }
        let mut v = 0u128;
        for _ in 0..n {
            let b = byte(self.d, self.p >> 3)?;
            v = (v << 1) | ((b >> (7 - (self.p & 7))) & 1) as u128;
            self.p += 1;
        }
        Ok(v)
    }
}

/// Keyed frames from an MSB-first mask over frames 1.. (frame 0 always).
fn popmask(bs: &[u8]) -> Vec<usize> {
    let mut fr = vec![0];
    for (i, b) in bs.iter().enumerate() {
        for k in 0..8 {
            if (b >> (7 - k)) & 1 == 1 {
                fr.push(i * 8 + k + 1);
            }
        }
    }
    fr
}

/// The `HADO` object: the pose slots' default pose and the slot range of each group.
pub struct Hado {
    /// Per slot: scale, rotation (x, y, z, w), translation.
    pub rest: Vec<([f64; 3], [f64; 4], [f64; 3])>,
    /// Groups 1-7: first slot and end.
    pub ranges: [(usize, usize); 8],
}

impl Hado {
    pub fn parse(anm: &[u8]) -> Result<Hado> {
        let h = anm.windows(6).position(|w| w == b"\x06\x01HADO").ok_or_else(|| anyhow!("no HADO object"))?;
        let slots = u16_at(anm, h + 0x12)? as usize;
        let mut ranges = [(0, 0); 8];
        for (gi, r) in ranges.iter_mut().enumerate().skip(1) {
            let base = h + 0xc0 + 10 * gi;
            let size = match u16_at(anm, base)? {
                0 => 256,
                n => n as usize,
            };
            let start = u16_at(anm, base + 6)? as usize;
            *r = (start, start + size);
        }
        if ranges[7].1 != slots || (1..7).any(|g| ranges[g].1 != ranges[g + 1].0) {
            bail!("HADO groups {ranges:?} do not tile its {slots} slots");
        }
        let f = |o: usize| -> Result<f64> { Ok(f32_at(anm, o)? as f64) };
        let mut rest = Vec::with_capacity(slots);
        for k in 0..slots {
            let o = h + 0x630 + 48 * k;
            rest.push(([f(o)?, f(o + 4)?, f(o + 8)?], [f(o + 16)?, f(o + 20)?, f(o + 24)?, f(o + 28)?], [f(o + 32)?, f(o + 36)?, f(o + 40)?]));
        }
        Ok(Hado { rest, ranges })
    }
}

/// A clip's header and group table.
struct Cado {
    base: usize,
    fps: f32,
    frames: usize,
    /// (descriptor offset, block table offset, blocks), from the base.
    groups: Vec<(usize, usize, usize)>,
}

fn cado(b: &[u8], base: usize) -> Result<Cado> {
    if b.get(base + 2..base + 6) != Some(b"CADO") {
        bail!("no CADO at {base:#x}");
    }
    let ngroups = u16_at(b, base + 12)? as usize;
    let groups_at = u32_at(b, base + 20)? as usize;
    let groups = (0..ngroups).map(|i| -> Result<(usize, usize, usize)> { let o = base + groups_at + 16 * i; Ok((u32_at(b, o)? as usize, u32_at(b, o + 8)? as usize, u32_at(b, o + 12)? as usize)) }).collect::<Result<_>>()?;
    Ok(Cado { base, fps: f32_at(b, base + 28)?, frames: u16_at(b, base + 34)? as usize, groups })
}

/// The CADO base of the clip with this hash.
pub fn find_clip(anm: &[u8], hash: u32) -> Option<usize> {
    let mut i = 0;
    while let Some(k) = anm[i..].windows(4).position(|w| w == b"CADO") {
        let base = (i + k).checked_sub(2)?;
        if u32_at(anm, base + 6).ok() == Some(hash) {
            return Some(base);
        }
        i += k + 4;
    }
    None
}

struct Rec {
    t: u8,
    f: Vec<u16>,
}

fn records(b: &[u8], gb: usize) -> Result<Vec<Rec>> {
    let mut out = Vec::new();
    let mut o = gb;
    loop {
        let t = byte(b, o)?;
        if t == 0 {
            return Ok(out);
        }
        let k = byte(b, o + 1)? as usize;
        let f = (0..k).map(|i| u16_at(b, o + 2 + 2 * i)).collect::<Result<Vec<_>>>()?;
        if f.len() < 3 || out.len() > 512 {
            bail!("a descriptor record of {k} fields at {o:#x}");
        }
        out.push(Rec { t, f });
        o += 2 + 2 * k;
    }
}

/// One run of animated tracks: widths (0d), ranges (1b), rotation records (1c / 1d / 1e), lists (12 / 13).
#[derive(Default)]
struct Section {
    trans: Vec<u16>,
    rots: Vec<u16>,
    widths: Vec<u8>,
    /// (min, step), each as the z, y, x fields.
    ranges: Vec<([f64; 3], [f64; 3])>,
    splits: Vec<[u8; 4]>,
    c_splits: Vec<[u8; 4]>,
    d_recs: Vec<[f64; 4]>,
    e_recs: Vec<([f64; 4], [f64; 4])>,
    nd: usize,
    ne: usize,
    nc: usize,
    /// Rotation category -> its first scratch slot.
    order: BTreeMap<char, i64>,
    b_start: i64,
    t_start: i64,
}

const KNOWN: [u8; 11] = [0x0d, 0x12, 0x13, 0x15, 0x16, 0x17, 0x18, 0x1b, 0x1c, 0x1d, 0x1e];

fn sf(v: u64, w: u32) -> f64 {
    f32::from_bits(((v << (32 - w)) & 0xffff_ffff) as u32) as f64
}
fn uf(v: u64, w: u32) -> f64 {
    f32::from_bits(((v << (31 - w)) & 0xffff_ffff) as u32) as f64
}
fn fields(q: u64, f: fn(u64, u32) -> f64) -> [f64; 3] {
    [f(q & 0x1f_ffff, 21), f((q >> 21) & 0x1f_ffff, 21), f((q >> 42) & 0x3f_ffff, 22)]
}

struct Group {
    gb: usize,
    recs: Vec<Rec>,
    sections: Vec<Section>,
}

impl Group {
    fn parse(b: &[u8], gb: usize) -> Result<Group> {
        let recs = records(b, gb)?;
        let unsupported: Vec<u8> = recs.iter().map(|r| r.t).filter(|t| !KNOWN.contains(t)).collect();
        if !unsupported.is_empty() {
            bail!("records {unsupported:02x?} (2-frame pose clips) are not decoded");
        }
        let mut sections: Vec<Section> = Vec::new();
        for r in &recs {
            let n = (r.f[0] & 0x7fff) as usize;
            let per = r.f[0] >> 15 == 1;
            let o = gb + r.f[2] as usize;
            let list = |n: usize| (0..n).map(|i| u16_at(b, o + 2 * i)).collect::<Result<Vec<_>>>();
            if r.t == 0x0d {
                let mut s = Section::default();
                s.widths = if per { b.get(o..o + n).ok_or_else(|| anyhow!("widths past the end"))?.to_vec() } else { vec![byte(b, o)?; n] };
                sections.push(s);
                continue;
            }
            if matches!(r.t, 0x15..=0x18) {
                continue;
            }
            let sec = sections.last_mut().ok_or_else(|| anyhow!("record {:02x} before a section", r.t))?;
            let field3 = || r.f.get(3).copied().map(|v| v as i64).ok_or_else(|| anyhow!("record {:02x} without its slot", r.t));
            match r.t {
                0x12 => {
                    sec.trans = list(n)?;
                    sec.t_start = field3()?;
                }
                0x13 => sec.rots = list(n)?,
                0x1b => {
                    sec.b_start = field3()?;
                    let m = if per { n } else { 1 };
                    for i in 0..m {
                        let lo = u64::from_le_bytes(at(b, o + 16 * i)?);
                        let hi = u64::from_le_bytes(at(b, o + 16 * i + 8)?);
                        sec.ranges.push((fields(hi, sf), fields(lo, uf)));
                    }
                    for i in 0..m {
                        sec.splits.push(at(b, o + 16 * m + 4 * i)?);
                    }
                    if m == 1 {
                        sec.ranges = vec![sec.ranges[0]; n];
                        sec.splits = vec![sec.splits[0]; n];
                    }
                }
                0x1c => {
                    sec.nc = n;
                    sec.order.insert('c', field3()?);
                    sec.c_splits = (0..n).map(|i| at(b, o + 4 * i * per as usize)).collect::<Result<_>>()?;
                }
                0x1d => {
                    sec.nd = n;
                    sec.order.insert('d', field3()?);
                    // two tracks per 16-byte block, components interleaved
                    for i in 0..n {
                        let blk = o + 16 * (i / 2);
                        sec.d_recs.push(std::array::from_fn::<f64, 4, _>(|c| i16_at(b, blk + 2 * ((i & 1) + 2 * c)).map(|v| v as f64 / 32768.0).unwrap_or(f64::NAN)));
                    }
                    if sec.d_recs.iter().flatten().any(|v| v.is_nan()) {
                        bail!("1d records past the end");
                    }
                }
                0x1e => {
                    sec.ne = n;
                    sec.order.insert('e', field3()?);
                    for i in 0..n {
                        let v = (0..8).map(|k| i16_at(b, o + 16 * i + 2 * k).map(|x| x as f64 / 32768.0)).collect::<Result<Vec<_>>>()?;
                        sec.e_recs.push(([v[0], v[2], v[4], v[6]], [v[1], v[3], v[5], v[7]]));
                    }
                }
                _ => {}
            }
        }
        Ok(Group { gb, recs, sections })
    }

    /// (kind, slot entry, section, record index) in bitstream order: every track owns a 32-byte scratch slot; each
    /// category record gives its first slot, categories run in slot order and an odd one leaves a pad slot (a
    /// sentinel in the 13 list between rotation categories). 1b ranges start at the 1b record's slot: translations
    /// before it are raw f32 x3; rotations left after the categories raw f32 x4.
    fn tracks(&self) -> Result<Vec<Track>> {
        let mut out: Vec<Track> = Vec::new();
        for (si, sec) in self.sections.iter().enumerate() {
            let mut cats: Vec<(i64, char, usize)> = Vec::new();
            if !sec.trans.is_empty() {
                cats.push((sec.t_start, 't', sec.trans.len()));
            }
            for (k, n) in [('d', sec.nd), ('e', sec.ne), ('c', sec.nc)] {
                if n > 0 {
                    cats.push((*sec.order.get(&k).ok_or_else(|| anyhow!("no slot for {k}"))?, k, n));
                }
            }
            cats.sort();
            let (mut pos, mut p) = (0i64, 0usize);
            let ranged = |at: i64| -> i64 { (at - sec.b_start).div_euclid(32) };
            let rot = |p: usize| sec.rots.get(p).copied().ok_or_else(|| anyhow!("the rotation list runs out"));
            for &(start, kind, n) in &cats {
                while pos < start {
                    out.push(Track { kind: Kind::Pad, v: 0, si, ri: 0 });
                    pos += 32;
                    if kind != 't' && out.len() >= 2 && out[out.len() - 2].kind != Kind::T && out[out.len() - 2].kind != Kind::TRaw {
                        p += 1;
                    }
                }
                for i in 0..n {
                    if kind == 't' {
                        let v = sec.trans[i];
                        out.push(if pos < sec.b_start { Track { kind: Kind::TRaw, v, si, ri: 0 } } else { Track { kind: Kind::T, v, si, ri: ranged(pos) } });
                    } else {
                        let v = rot(p)?;
                        p += 1;
                        out.push(match kind {
                            'c' => Track { kind: Kind::Rc, v, si, ri: i as i64 },
                            'd' => Track { kind: Kind::Rd, v, si, ri: ranged(pos) },
                            _ => Track { kind: Kind::Re, v, si, ri: ranged(pos) },
                        });
                    }
                    pos += 32;
                }
            }
            if p < sec.rots.len() && cats.last().is_some_and(|c| c.2 & 1 == 1 && c.1 != 't') {
                p += 1;
                out.push(Track { kind: Kind::Pad, v: 0, si, ri: 0 });
            }
            for &v in sec.rots.iter().skip(p) {
                out.push(Track { kind: Kind::RRaw, v, si, ri: 0 });
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Pad,
    T,
    TRaw,
    Rc,
    Rd,
    Re,
    RRaw,
}

#[derive(Clone, Copy)]
struct Track {
    kind: Kind,
    /// The slot entry: slot (within the group) x 48 + 0 (scale), 16 (rotation) or 32 (translation).
    v: u16,
    si: usize,
    /// The range (or 1c split) index; Python indexing, so a negative one counts from the end.
    ri: i64,
}

fn pick<T: Copy>(v: &[T], i: i64) -> Result<T> {
    let k = if i < 0 { v.len() as i64 + i } else { i };
    usize::try_from(k).ok().and_then(|k| v.get(k).copied()).ok_or_else(|| anyhow!("index {i} of {}", v.len()))
}

/// [(first frame, last frame, data offset)] for one group: an MSB-first frame mask marks block boundaries (padded to
/// 4 bytes), then a seek table (6 bytes per 128 frames, padded to 4), then n + 1 u32 block offsets.
fn blocks_of(b: &[u8], c: &Cado, g: (usize, usize, usize)) -> Result<Vec<(usize, usize, usize)>> {
    let (_, idx, n) = g;
    let f = c.frames;
    let mb = ((f + 7) / 8 + 3) & !3;
    let mask = b.get(c.base + idx..c.base + idx + mb).ok_or_else(|| anyhow!("block mask past the end"))?;
    let marks: Vec<usize> = (0..f).filter(|i| (mask[i >> 3] >> (7 - (i & 7))) & 1 == 1).collect();
    let seek = (6 * ((f.max(1) - 1) / 128 + 1) + 3) & !3;
    if marks.len() != n + 1 {
        bail!("{} block marks for {n} blocks", marks.len());
    }
    (0..n).map(|i| Ok((marks[i], marks[i + 1], c.base + u32_at(b, c.base + idx + mb + seek + 4 * i)? as usize))).collect()
}

fn split3(v: u128, sp: [u8; 4]) -> (u128, u128, u128) {
    let (by, bz) = (sp[1] as u32, sp[3] as u32);
    (v >> (by + bz), (v >> bz) & ((1u128 << by) - 1), v & ((1u128 << bz) - 1))
}

fn deq(v: u128, sp: [u8; 4], rng: ([f64; 3], [f64; 3])) -> [f64; 3] {
    let (mn, st) = rng;
    let (qx, qy, qz) = split3(v, sp);
    [mn[2] + qx as f64 * st[2], mn[1] + qy as f64 * st[1], mn[0] + qz as f64 * st[0]]
}

/// The largest component left out, the others centred, its index in the lowest two bits; it was swapped into w's place.
fn smallest3(v: u128, sp: [u8; 4]) -> [f64; 4] {
    let idx = (v & 3) as usize;
    let (a, b, c) = split3(v >> 2, sp);
    let comp = |q: u128, bb: u8| if bb > 0 { (q as f64 - (1u128 << (bb - 1)) as f64) / (1u128 << (bb - 1)) as f64 * S2 } else { 0.0 };
    let comps = [comp(a, sp[0]), comp(b, sp[1]), comp(c, sp[3])];
    let big = (1.0 - comps.iter().map(|x| x * x).sum::<f64>()).max(0.0).sqrt();
    let mut q = [comps[0], comps[1], comps[2], big];
    q.swap(idx, 3);
    q
}

/// 48-bit constant rotation: x 16 bits, y 15, z 15, index 2 (MSB first, little-endian bytes).
fn q48(bs: [u8; 6]) -> [f64; 4] {
    let v = bs.iter().rev().fold(0u64, |a, x| (a << 8) | *x as u64);
    let idx = (v & 3) as usize;
    let (z, y, x) = ((v >> 2) & 0x7fff, (v >> 17) & 0x7fff, (v >> 32) & 0xffff);
    let comps = [(x as f64 - 32768.0) / 32768.0 * S2, (y as f64 - 16384.0) / 16384.0 * S2, (z as f64 - 16384.0) / 16384.0 * S2];
    let big = (1.0 - comps.iter().map(|x| x * x).sum::<f64>()).max(0.0).sqrt();
    let mut q = [comps[0], comps[1], comps[2], big];
    q.swap(idx, 3);
    q
}

fn rawf(v: u128, n: usize) -> Vec<f64> {
    (0..n).map(|i| f32::from_bits(((v >> (32 * (n - 1 - i))) & 0xffff_ffff) as u32) as f64).collect()
}

fn lift(d: &[f64]) -> [f64; 4] {
    [d[0], d[1], d[2], (1.0 - (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])).max(0.0).sqrt()]
}

fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let ([ax, ay, az, aw], [bx, by, bz, bw]) = (a, b);
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}

fn normalized(q: [f64; 4]) -> [f64; 4] {
    let l = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    q.map(|x| x / l)
}

/// Linear between keys (both ends held).
fn interp(keys: &[usize], vals: &[Vec<f64>], frames: usize) -> Vec<Vec<f64>> {
    (0..frames)
        .map(|f| {
            let k = keys.partition_point(|x| *x <= f).saturating_sub(1).min(keys.len() - 1);
            if keys[k] == f || k == keys.len() - 1 {
                return vals[k].clone();
            }
            let t = (f - keys[k]) as f64 / (keys[k + 1] - keys[k]) as f64;
            vals[k].iter().zip(&vals[k + 1]).map(|(a, b)| a * (1.0 - t) + b * t).collect()
        })
        .collect()
}

/// A slot's animated values over the clip (None: the default pose).
#[derive(Default, Clone)]
struct SlotTrack {
    t: Option<Vec<[f64; 3]>>,
    r: Option<Vec<[f64; 4]>>,
    s: Option<Vec<[f64; 3]>>,
}

fn decode_group(b: &[u8], c: &Cado, gi: usize) -> Result<BTreeMap<usize, SlotTrack>> {
    let entry = c.groups[gi];
    let g = Group::parse(b, c.base + entry.0)?;
    let f = c.frames;
    let mut res: BTreeMap<usize, SlotTrack> = BTreeMap::new();
    if g.recs.is_empty() {
        return Ok(res);
    }
    let tr = g.tracks()?;
    // per (kind, slot entry): keys and values, in first-seen order
    let mut per: Vec<((Kind, u16), Vec<usize>, Vec<Vec<f64>>)> = Vec::new();
    if !tr.is_empty() {
        for (s, e, start) in blocks_of(b, c, entry)? {
            let a = u16_at(b, start)? as usize;
            let (nsec, mb) = ((a & !3) / 4, (a & 3) + 1);
            if nsec != g.sections.len() || (e - s + 7) / 8 > mb {
                bail!("block at {start:#x}: {nsec} sections, {mb} mask bytes for frames {s}-{e}");
            }
            for si in 0..nsec {
                let big = u16_at(b, start + 4 * si + 2)? as usize;
                let mo = start + (u16_at(b, start + 4 * si)? as usize & !3);
                let st: Vec<&Track> = tr.iter().filter(|t| t.si == si).collect();
                if big != mb * st.len() {
                    bail!("block at {start:#x} section {si}: {big} mask bytes for {} tracks", st.len());
                }
                let sec = &g.sections[si];
                let mut bits = Bits { d: b, p: (mo + big) * 8 };
                for (ti, t) in st.iter().enumerate() {
                    let mask = b.get(mo + mb * ti..mo + mb * (ti + 1)).ok_or_else(|| anyhow!("key mask past the end"))?;
                    let keys: Vec<usize> = popmask(mask).into_iter().map(|k| s + k).filter(|k| *k <= e).collect();
                    let w = *sec.widths.get(ti).ok_or_else(|| anyhow!("no width for track {ti}"))? as u32;
                    let sp = match t.kind {
                        Kind::Rc => pick(&sec.c_splits, t.ri)?,
                        Kind::T | Kind::Rd | Kind::Re => pick(&sec.splits, t.ri)?,
                        _ => [0; 4],
                    };
                    let mut vals = Vec::with_capacity(keys.len());
                    for _ in &keys {
                        vals.push(bits.read(w)?);
                    }
                    if t.kind == Kind::Pad {
                        continue;
                    }
                    let k = (t.kind, t.v);
                    let i = match per.iter().position(|p| p.0 == k) {
                        Some(i) => i,
                        None => {
                            per.push((k, Vec::new(), Vec::new()));
                            per.len() - 1
                        }
                    };
                    let (_, ks, vs) = &mut per[i];
                    for (key, v) in keys.iter().zip(vals) {
                        if ks.last() == Some(key) {
                            ks.pop();
                            vs.pop();
                        }
                        ks.push(*key);
                        vs.push(match t.kind {
                            Kind::Rc => smallest3(v, sp).to_vec(),
                            Kind::TRaw => rawf(v, 3),
                            Kind::RRaw => rawf(v, 4),
                            _ => deq(v, sp, pick(&sec.ranges, t.ri)?).to_vec(),
                        });
                    }
                }
            }
        }
    }
    // each 1d / 1e track's record within its section's list
    let mut count: BTreeMap<(usize, Kind), usize> = BTreeMap::new();
    let mut rec: Vec<((Kind, u16), (usize, usize))> = Vec::new();
    for t in &tr {
        if matches!(t.kind, Kind::Rd | Kind::Re) {
            let n = count.entry((t.si, t.kind)).or_insert(0);
            rec.retain(|r| r.0 != (t.kind, t.v));
            rec.push(((t.kind, t.v), (t.si, *n)));
            *n += 1;
        }
    }
    for ((kind, v), ks, vs) in &per {
        let d = res.entry(*v as usize / 48).or_default();
        let arr = interp(ks, vs, f);
        match kind {
            Kind::T | Kind::TRaw => d.t = Some(arr.iter().map(|x| [x[0], x[1], x[2]]).collect()),
            Kind::Rc | Kind::RRaw => d.r = Some(arr.iter().map(|x| normalized([x[0], x[1], x[2], x[3]])).collect()),
            Kind::Rd | Kind::Re => {
                let (si, i) = rec.iter().find(|r| r.0 == (*kind, *v)).map(|r| r.1).ok_or_else(|| anyhow!("no record for a rotation track"))?;
                let sec = &g.sections[si];
                d.r = Some(if *kind == Kind::Rd {
                    let rf = *sec.d_recs.get(i).ok_or_else(|| anyhow!("no 1d record {i}"))?;
                    arr.iter().map(|x| qmul(rf, lift(x))).collect()
                } else {
                    let (qa, qb) = *sec.e_recs.get(i).ok_or_else(|| anyhow!("no 1e record {i}"))?;
                    arr.iter().map(|x| qmul(qmul(qa, lift(x)), qb)).collect()
                });
            }
            Kind::Pad => {}
        }
    }
    // constants: 15 translations f32 x3, 16 f16 x3 (scale at +0, else translation), 17 rotations f32 x4, 18 q48
    for r in &g.recs {
        if !matches!(r.t, 0x15..=0x18) {
            continue;
        }
        let n = (r.f[0] & 0x7fff) as usize;
        let o = g.gb + r.f[2] as usize;
        let list = (0..n).map(|i| u16_at(b, o + 2 * i)).collect::<Result<Vec<_>>>()?;
        let data = (o + 2 * n + 15) & !15;
        for (i, v) in list.iter().enumerate() {
            let d = res.entry(*v as usize / 48).or_default();
            match r.t {
                0x18 => d.r = Some(vec![q48(at(b, data + 6 * i)?); f]),
                0x15 => {
                    let t = [f32_at(b, data + 12 * i)? as f64, f32_at(b, data + 12 * i + 4)? as f64, f32_at(b, data + 12 * i + 8)? as f64];
                    d.t = Some(vec![t; f]);
                }
                0x17 => {
                    let q = [f32_at(b, data + 16 * i)?, f32_at(b, data + 16 * i + 4)?, f32_at(b, data + 16 * i + 8)?, f32_at(b, data + 16 * i + 12)?].map(|x| x as f64);
                    d.r = Some(vec![normalized(q); f]);
                }
                _ => {
                    let h = |k: usize| -> Result<f64> { Ok(f16(u16_at(b, data + 6 * i + 2 * k)?)) };
                    let val = [h(0)?, h(1)?, h(2)?];
                    if v % 48 == 0 {
                        d.s = Some(vec![val; f]);
                    } else {
                        d.t = Some(vec![val; f]);
                    }
                }
            }
        }
    }
    Ok(res)
}

/// IEEE half to f64.
fn f16(h: u16) -> f64 {
    let (s, e, m) = ((h >> 15) as i32, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as f64);
    let v = match e {
        0 => m * 2f64.powi(-24),
        31 => if m == 0.0 { f64::INFINITY } else { f64::NAN },
        _ => (1.0 + m / 1024.0) * 2f64.powi(e - 15),
    };
    if s == 1 { -v } else { v }
}

fn mul(a: &M, b: &M) -> M {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

fn qmat(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = q;
    [[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)], [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)], [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)]]
}

fn mat2quat(m: &M) -> [f64; 4] {
    let t = m[0][0] + m[1][1] + m[2][2];
    let q = if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
    } else if m[0][0] >= m[1][1] && m[0][0] >= m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
    } else if m[1][1] >= m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
    };
    normalized(q)
}

/// A 16-float column-major matrix as rows.
fn rows(c: &[f32; 16]) -> M {
    std::array::from_fn(|r| std::array::from_fn(|k| c[k * 4 + r] as f64))
}

/// The inverse of an affine transform (3x3 by its adjugate).
fn invert(m: &M) -> Option<M> {
    let a = |r: usize, c: usize| m[r][c];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0)) + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-12 {
        return None;
    }
    let mut o = I4;
    for r in 0..3 {
        for c in 0..3 {
            let (r1, r2, c1, c2) = ((c + 1) % 3, (c + 2) % 3, (r + 1) % 3, (r + 2) % 3);
            o[r][c] = (a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1)) / det;
        }
    }
    for r in 0..3 {
        o[r][3] = -(0..3).map(|k| o[r][k] * m[k][3]).sum::<f64>();
    }
    Some(o)
}

/// Slot -> joint: slots follow the skeleton's order with the procedural joints left out, so each slot takes the
/// first joint after the previous slot's whose rest transform equals its default pose (rotation either sign and
/// translation within 2e-3); a slot no joint matches maps to none.
pub fn slot_joints(hado: &Hado, sk: &Skeleton) -> Result<Vec<Option<usize>>> {
    let local: Vec<M> = sk.local.iter().map(rows).collect();
    let jq: Vec<[f64; 4]> = local.iter().map(mat2quat).collect();
    let mut prev: i64 = -1;
    let mut out = Vec::with_capacity(hado.rest.len());
    for (k, (_, q, t)) in hado.rest.iter().enumerate() {
        let fits = |j: usize| -> bool {
            let d = |sign: f64| (0..4).map(|c| (jq[j][c] - sign * q[c]).abs()).fold(0.0, f64::max);
            d(1.0).min(d(-1.0)) < 2e-3 && (0..3).all(|c| (local[j][c][3] - t[c]).abs() < 2e-3)
        };
        let any = (0..local.len()).any(fits);
        if !any {
            out.push(None);
            continue;
        }
        let j = ((prev + 1) as usize..local.len()).find(|j| fits(*j)).ok_or_else(|| anyhow!("slot {k}: no joint after {prev} matches"))?;
        prev = j as i64;
        out.push(Some(j));
    }
    Ok(out)
}

/// A clip as his joints' world transforms (his model space, unmirrored, unscaled), frame by frame, and its rate.
pub fn decode_clip(anm: &[u8], base: usize, hado: &Hado, sk: &Skeleton, slots: &[Option<usize>]) -> Result<(f32, Vec<Vec<M>>)> {
    let c = cado(anm, base)?;
    let f = c.frames;
    if f == 0 || c.groups.len() < 8 {
        bail!("a clip of {f} frames and {} groups", c.groups.len());
    }
    let mut pose: Vec<Vec<([f64; 3], [f64; 4], [f64; 3])>> = vec![hado.rest.clone(); f];
    for gi in 1..8 {
        let (start, end) = hado.ranges[gi];
        for (j, d) in decode_group(anm, &c, gi).with_context(|| format!("group {gi}"))? {
            let k = start + j;
            if k >= end {
                continue; // sentinel / padding
            }
            for (fr, p) in pose.iter_mut().enumerate() {
                if let Some(s) = &d.s {
                    p[k].0 = s[fr];
                }
                if let Some(r) = &d.r {
                    p[k].1 = r[fr];
                }
                if let Some(t) = &d.t {
                    p[k].2 = t[fr];
                }
            }
        }
    }
    let rest_local: Vec<M> = sk.local.iter().map(rows).collect();
    let n = rest_local.len();
    let mut world = Vec::with_capacity(f);
    for p in &pose {
        let mut local = rest_local.clone();
        for (k, j) in slots.iter().enumerate() {
            let Some(j) = *j else { continue };
            let (s, r, t) = p[k];
            let q = qmat(normalized(r));
            let mut m = I4;
            for row in 0..3 {
                for col in 0..3 {
                    m[row][col] = q[row][col] * s[col];
                }
                m[row][3] = t[row];
            }
            local[j] = m;
        }
        let mut w: Vec<M> = Vec::with_capacity(n);
        for j in 0..n {
            let par = sk.parent[j];
            w.push(if par < 0 { local[j] } else { mul(&w[par as usize], &local[j]) });
        }
        world.push(w);
    }
    Ok((c.fps, world))
}

/// A clip on the Elden Ring skeleton: per frame, per bone, its skinning in model space (row-major 3x4: the animated
/// transform times the inverse of the bind), and a line about it.
pub struct Clip {
    pub name: String,
    pub fps: f32,
    /// How fast his root moves over the clip, along the ground (m/s): 0 for the idle.
    pub speed: f32,
    pub frames: Vec<Vec<[f32; 12]>>,
    pub note: String,
    /// Per Elden Ring bone, the joint of his it follows.
    pub anchors: Vec<Option<usize>>,
}

/// His world transforms (`world`, as [`decode_clip`] gives them) on the Elden Ring skeleton `er`, through his re-posing
/// `r` (made for `mirrored`, his skeleton mirrored as the parts are) at `scale`.
pub fn retarget_clip(name: &str, fps: f32, world: &[Vec<M>], mirrored: &Skeleton, scale: f64, r: &Retarget, er: &[Node], driven: &[bool]) -> Result<Clip> {
    let n = mirrored.parent.len();
    if world.first().map(|w| w.len()) != Some(n) || r.moves.len() != n {
        bail!("the clip, the skeleton and the re-posing disagree on the joint count");
    }
    // mirrored across x (negate the first row and column but their shared entry), translations scaled
    let fix = |m: &M| -> M {
        let mut o = *m;
        for k in 1..4 {
            o[0][k] = -o[0][k];
            o[k][0] = -o[k][0];
        }
        for row in o.iter_mut().take(3) {
            row[3] *= scale;
        }
        o
    };
    // his rest pose (bind) from his rest transforms composed down the hierarchy, translations scaled: it equals the
    // inverse binds for the joints that carry skin (817 of 826 to 1e-3), and is there for the chain joints that carry
    // none
    let mut rest_world: Vec<M> = Vec::with_capacity(n);
    for j in 0..n {
        let l = rows(&mirrored.local[j]);
        let p = mirrored.parent[j];
        rest_world.push(if p < 0 { l } else { mul(&rest_world[p as usize], &l) });
    }
    let bind: Vec<M> = rest_world
        .iter()
        .map(|m| {
            let mut w = *m;
            for row in w.iter_mut().take(3) {
                row[3] *= scale;
            }
            w
        })
        .collect();
    let unmove: Vec<M> = r.moves.iter().map(|m| invert(&std::array::from_fn(|row| std::array::from_fn(|c| m[c][row] as f64))).unwrap_or(I4)).collect();
    let unbind: Vec<M> = bind.iter().map(|m| invert(m).unwrap_or(I4)).collect();
    // his root at rest in every frame: the game moves the character
    let root_rest = bind[ROOT_JOINT.min(n - 1)];
    // the Elden Ring side
    let ew: Vec<M> = world_matrices(er).iter().map(|c| std::array::from_fn(|row| std::array::from_fn(|k| c[k][row] as f64))).collect();
    let nb = er.len();
    let joint_rest: Vec<[f64; 3]> = ew.iter().map(|m| [m[0][3], m[1][3], m[2][3]]).collect();
    // each bone's anchor: the nearest (in the new pose) of his moving joints that weigh on it ...
    let moved_pos = |j: usize| -> [f64; 3] {
        let m = &r.moves[j];
        let p = r.bind_positions[j].map(|x| x as f64);
        std::array::from_fn(|row| m[0][row] as f64 * p[0] + m[1][row] as f64 * p[1] + m[2][row] as f64 * p[2] + m[3][row] as f64)
    };
    let dist = |a: [f64; 3], b: [f64; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    // ... taken up to its first joint that the clip drives cleanly (it and every joint above it from a pose slot): the
    // others are moved in Ragnarök by its rig program, which is not run, and some carry an ambiguous slot's values
    // (0.1.91 anchored both upper arms on such a helper: the arms came apart)
    let mut clean = vec![false; n];
    for j in 0..n {
        let p = mirrored.parent[j];
        clean[j] = driven.get(j).copied().unwrap_or(false) && (p < 0 || clean[p as usize]);
    }
    let cleanly = |mut j: usize| -> Option<usize> {
        loop {
            if clean[j] {
                return Some(j);
            }
            j = usize::try_from(mirrored.parent[j]).ok()?;
        }
    };
    // ... rather, of those (each taken up to its first cleanly driven joint), the one whose motion carries the points
    // of all of them closest to where their own motion takes them, over a few frames: nearest by position alone
    // anchored a finger bone on a fingertip whose middle phalanx then followed the tip (0.1.91-0.1.92: the left
    // hand's fingers stretched 7 cm, the hand flattened)
    let pinned = |w: &Vec<M>| -> Vec<M> {
        let mut kw: Vec<M> = w.iter().map(fix).collect();
        let pin = mul(&root_rest, &invert(&kw[ROOT_JOINT.min(n - 1)]).unwrap_or(I4));
        for m in kw.iter_mut() {
            *m = mul(&pin, m);
        }
        kw
    };
    let samples: Vec<Vec<M>> = [0, world.len() / 3, 2 * world.len() / 3].iter().map(|f| pinned(&world[(*f).min(world.len() - 1)])).collect();
    let motion = |kw: &[M], j: usize| mul(&mul(&kw[j], &unbind[j]), &unmove[j]);
    let apply = |m: &M, p: [f64; 3]| -> [f64; 3] { std::array::from_fn(|row| m[row][0] * p[0] + m[row][1] * p[1] + m[row][2] * p[2] + m[row][3]) };
    let moved_rest = |j: usize| -> [f64; 3] {
        let m = &r.moves[j];
        let p = [bind[j][0][3], bind[j][1][3], bind[j][2][3]];
        std::array::from_fn(|row| m[0][row] as f64 * p[0] + m[1][row] as f64 * p[1] + m[2][row] as f64 * p[2] + m[3][row] as f64)
    };
    // ... and first, of those, the ones that carry the most of the skin of the bones below it (in his hierarchy), never
    // its parent bone's anchor or one above that, with his skin-carrying joints between the two; of those carrying
    // the most, the nearest to the bone's own joint. His chest joint has
    // dozens of helpers that move with the arms and the breath, and the least-error pick alone took them for the spine
    // and the left clavicle (0.1.96 in play: his spine bent at one bone, Spine1, Spine2 and the left clavicle never
    // turned, his chest swung with his arms, and the neck followed a helper on his left shoulder)
    let mut er_below = vec![Vec::new(); nb];
    for b in 0..nb {
        let mut p = er[b].parent;
        while let Ok(a) = usize::try_from(p) {
            if a >= nb {
                break;
            }
            er_below[a].push(b);
            p = er[a].parent;
        }
    }
    let above = |c: usize, mut k: usize| -> bool {
        loop {
            if k == c {
                return true;
            }
            match usize::try_from(mirrored.parent[k]) {
                Ok(p) => k = p,
                Err(_) => return false,
            }
        }
    };
    let mut anchor: Vec<Option<usize>> = vec![None; nb];
    for b in 0..nb {
        // the Elden Ring skeleton lists parents first
        let mut up = usize::try_from(er[b].parent).ok().filter(|p| *p < b);
        let mut parent_anchor = None;
        while let Some(p) = up {
            if anchor[p].is_some() {
                parent_anchor = anchor[p];
                break;
            }
            up = usize::try_from(er[p].parent).ok().filter(|q| *q < p);
        }
        let allowed = |c: usize| parent_anchor.map_or(true, |a| !above(c, a));
        let members: Vec<usize> = (0..n).filter(|j| r.deforms[*j] && r.bone[*j] as usize == b).collect();
        // a bone none of his skin goes to (the root's) keeps none: it follows its parent
        if members.is_empty() {
            continue;
        }
        let mut options: Vec<usize> = members.iter().filter_map(|j| cleanly(*j)).collect();
        options.sort_unstable();
        options.dedup();
        let lower: Vec<usize> = (0..n).filter(|j| r.deforms[*j] && er_below[b].contains(&(r.bone[*j] as usize))).collect();
        let carries = |c: usize| lower.iter().filter(|k| above(c, **k)).count();
        let cost = |c: usize| -> f64 {
            samples.iter().map(|kw| { let g = motion(kw, c); members.iter().map(|k| { let q = moved_pos(*k); dist(apply(&g, q), apply(&motion(kw, *k), q)) }).sum::<f64>() }).sum()
        };
        let fair: Vec<usize> = options.iter().copied().filter(|c| allowed(*c)).collect();
        // with the skin-carrying joints between the parent bone's anchor and the skin below (his own chain, which
        // carries no skin of this bone's when the nearest-bone weighting gave it to the next bone up or down)
        let mut cands = fair.clone();
        for k in &lower {
            let mut j = cleanly(*k);
            while let Some(c) = j.filter(|c| allowed(*c)) {
                if r.deforms[c] {
                    cands.push(c);
                }
                j = usize::try_from(mirrored.parent[c]).ok();
            }
        }
        cands.sort_unstable();
        cands.dedup();
        let most = cands.iter().map(|c| carries(*c)).max().unwrap_or(0);
        let to_joint = |c: usize| dist(moved_rest(c), joint_rest[b]);
        anchor[b] = if most > 0 {
            // of those carrying the most (his pelvis, spine, chest joints all carry the arms), the nearest to the bone's
            // joint
            cands.into_iter().filter(|c| carries(*c) == most).min_by(|x, y| to_joint(*x).total_cmp(&to_joint(*y)))
        } else {
            let pool = if fair.is_empty() { options } else { fair };
            pool.into_iter().min_by(|x, y| cost(*x).total_cmp(&cost(*y)))
        };
    }
    // the fingers, chain by chain: each Elden Ring finger (FingerN, FingerN1, FingerN2) on one of his finger chains
    // (from his hand joint down, cleanly driven joints), the one whose joints line up with its own best in the new pose.
    // Bone by bone, a finger's middle bone took his fingertip (the little finger folded back 170 degrees), the thumb's
    // second its third, and no finger's last bone had one at all (0.1.91-0.1.92)
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); n];
    for j in 0..n {
        if let Ok(p) = usize::try_from(mirrored.parent[j]) {
            if clean[j] {
                kids[p].push(j);
            }
        }
    }
    let below = |j: usize| -> usize {
        let mut stack = vec![j];
        let mut c = 0;
        while let Some(k) = stack.pop() {
            c += 1;
            stack.extend(kids[k].iter().copied());
        }
        c
    };
    let mut fingers_note = Vec::new();
    for side in ["L", "R"] {
        let Some(hand) = er.iter().position(|b| b.name == format!("{side}_Hand")).and_then(|h| anchor[h]) else { continue };
        // his finger chains: from each cleanly driven child of the hand with at least two joints below it
        let chains: Vec<Vec<usize>> = kids[hand]
            .iter()
            .filter(|c| below(**c) >= 3)
            .map(|c| {
                let mut chain = vec![*c];
                let mut j = *c;
                while let Some(next) = kids[j].iter().copied().max_by_key(|k| below(*k)) {
                    chain.push(next);
                    j = next;
                }
                chain
            })
            .collect();
        // each finger on a chain of its own: the assignment with the least misfit over all five
        let fingers: Vec<(usize, Vec<usize>)> = (0..5)
            .map(|f| (f, ["", "1", "2"].iter().filter_map(|k| er.iter().position(|b| b.name == format!("{side}_Finger{f}{k}"))).collect::<Vec<usize>>()))
            .filter(|(_, bones)| !bones.is_empty())
            .collect();
        // the last bone on the joint before the tip: a finger's metacarpal is left out, the thumb's kept (by position
        // alone the little finger started a joint too far down, its knuckle bent 123 degrees)
        let fit = |bones: &[usize], chain: &[usize]| -> Option<(f64, usize)> {
            let start = chain.len().checked_sub(bones.len() + 1)?;
            Some((bones.iter().enumerate().map(|(i, b)| dist(joint_rest[*b], moved_rest(chain[start + i]))).sum(), start))
        };
        let mut best: Option<(f64, Vec<usize>)> = None;
        let mut pick = vec![usize::MAX; fingers.len()];
        fn search(k: usize, used: &mut Vec<bool>, pick: &mut Vec<usize>, cost: f64, costs: &[Vec<Option<f64>>], best: &mut Option<(f64, Vec<usize>)>) {
            if k == pick.len() {
                if best.as_ref().is_none_or(|b| cost < b.0) {
                    *best = Some((cost, pick.clone()));
                }
                return;
            }
            for c in 0..used.len() {
                if let (false, Some(x)) = (used[c], costs[k][c]) {
                    used[c] = true;
                    pick[k] = c;
                    search(k + 1, used, pick, cost + x, costs, best);
                    used[c] = false;
                }
            }
        }
        let costs: Vec<Vec<Option<f64>>> = fingers.iter().map(|(_, bones)| chains.iter().map(|c| fit(bones, c).map(|x| x.0)).collect()).collect();
        if chains.len() >= fingers.len() {
            search(0, &mut vec![false; chains.len()], &mut pick, 0.0, &costs, &mut best);
        }
        if let Some((_, pick)) = best {
            for ((f, bones), c) in fingers.iter().zip(pick) {
                let chain = &chains[c];
                let Some((cost, start)) = fit(bones, chain) else { continue };
                for (i, b) in bones.iter().enumerate() {
                    anchor[*b] = Some(chain[start + i]);
                }
                fingers_note.push(format!("{side}{f} {}+{start} ({:.1} cm)", chain[0], 100.0 * cost / bones.len() as f64));
            }
        }
    }
    // the forearms' twist bones: each takes its share of the hand's twist about the forearm (by where it sits from
    // elbow to wrist), as Elden Ring's rig does and Ragnarök's rig program did for his own twist joints. Following the
    // elbow alone (0.1.91-0.1.92), every forearm vertex by the wrist stayed put while the hand turned, and the wrists
    // wrung flat ("stretched flat into his legs")
    let index = |name: &str| er.iter().position(|b| b.name == name);
    let mut twists: Vec<(usize, usize, usize, f64)> = Vec::new(); // (twist bone, forearm, hand, share)
    for side in ["L", "R"] {
        let (Some(fa), Some(hand)) = (index(&format!("{side}_Forearm")), index(&format!("{side}_Hand"))) else { continue };
        let axis = sub3(joint_rest[hand], joint_rest[fa]);
        let len2 = dot3(axis, axis);
        for name in ["ForeArmTwist", "ForeArmTwist1"] {
            let Some(t) = index(&format!("{side}_{name}")) else { continue };
            if len2 > 1e-8 && er[t].parent as usize == fa {
                twists.push((t, fa, hand, (dot3(sub3(joint_rest[t], joint_rest[fa]), axis) / len2).clamp(0.0, 1.0)));
            }
        }
    }
    let mut frames = Vec::with_capacity(world.len());
    let mut pelvis_y = (f64::MAX, f64::MIN);
    let pelvis = er.iter().position(|b| b.name == "Pelvis");
    for w in world {
        let mut kw: Vec<M> = w.iter().map(fix).collect();
        let pin = mul(&root_rest, &invert(&kw[ROOT_JOINT.min(n - 1)]).unwrap_or(I4));
        for m in kw.iter_mut() {
            *m = mul(&pin, m);
        }
        let mut skin: Vec<M> = Vec::with_capacity(nb);
        for b in 0..nb {
            let par = usize::try_from(er[b].parent).ok().filter(|p| *p < b);
            let parent_skin = par.map(|p| skin[p]).unwrap_or(I4);
            let s = match anchor[b] {
                Some(j) => {
                    let g = mul(&mul(&kw[j], &unbind[j]), &unmove[j]);
                    let anchored_parent = par.is_some_and(|p| has_anchor_above(er, &anchor, p));
                    let from = if anchored_parent { &parent_skin } else { &g };
                    let jr = joint_rest[b];
                    let jt: [f64; 3] = std::array::from_fn(|row| from[row][0] * jr[0] + from[row][1] * jr[1] + from[row][2] * jr[2] + from[row][3]);
                    let mut s = g;
                    for row in 0..3 {
                        s[row][3] = jt[row] - (g[row][0] * jr[0] + g[row][1] * jr[1] + g[row][2] * jr[2]);
                    }
                    s
                }
                None => parent_skin,
            };
            skin.push(s);
        }
        for &(t, fa, hand, share) in &twists {
            skin[t] = twisted(&skin[fa], &skin[hand], sub3(joint_rest[hand], joint_rest[fa]), share, joint_rest[t]);
        }
        if let Some(p) = pelvis {
            let y = skin[p][1][0] * joint_rest[p][0] + skin[p][1][1] * joint_rest[p][1] + skin[p][1][2] * joint_rest[p][2] + skin[p][1][3];
            pelvis_y = (pelvis_y.0.min(y), pelvis_y.1.max(y));
        }
        frames.push(skin.iter().map(|m| std::array::from_fn(|k| m[k / 4][k % 4] as f32)).collect());
    }
    let anchored = anchor.iter().filter(|a| a.is_some()).count();
    let note = format!("{name}: {} frames at {fps} fps, {anchored} of {nb} bones on his joints (fingers {}), pelvis {:.2}-{:.2} m (rest {:.2})", frames.len(), fingers_note.join(", "), pelvis_y.0, pelvis_y.1, pelvis.map_or(0.0, |p| joint_rest[p][1]));
    Ok(Clip { name: name.into(), fps, speed: 0.0, frames, note, anchors: anchor })
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A twist bone's skinning: its parent's (`limb`), turned by `share` of the twist of `next` (the bone the limb ends
/// at) relative to it about the limb's rest axis; its joint stays where the limb's skinning puts it.
fn twisted(limb: &M, next: &M, axis: [f64; 3], share: f64, joint: [f64; 3]) -> M {
    let l = dot3(axis, axis).sqrt();
    let a = axis.map(|x| x / l);
    // the relative turn (rest space): limb^-1 x next, rotation only
    let rel = mul(&invert(limb).unwrap_or(I4), next);
    let q = mat2quat(&rel);
    let along = q[0] * a[0] + q[1] * a[1] + q[2] * a[2];
    let angle = 2.0 * along.atan2(q[3]);
    let half = angle * share / 2.0;
    let part = qmat([a[0] * half.sin(), a[1] * half.sin(), a[2] * half.sin(), half.cos()]);
    let mut turn = I4;
    for r in 0..3 {
        turn[r][..3].copy_from_slice(&part[r]);
    }
    // about the twist bone's rest joint, then the limb's skinning
    for r in 0..3 {
        turn[r][3] = joint[r] - (turn[r][0] * joint[0] + turn[r][1] * joint[1] + turn[r][2] * joint[2]);
    }
    mul(limb, &turn)
}

/// Whether any bone above `b` (it included) has an anchor: then `b`'s children take their joints from its skinning.
fn has_anchor_above(er: &[Node], anchor: &[Option<usize>], b: usize) -> bool {
    let mut k = Some(b);
    while let Some(i) = k {
        if anchor[i].is_some() {
            return true;
        }
        k = usize::try_from(er[i].parent).ok().filter(|p| *p < i);
    }
    false
}

/// The moves file: "GRMV", u32 version 2, u32 bones, u32 clips; per clip u32 name length, the name, f32 fps, f32
/// speed (version 2), u32 frames, then per frame per bone 12 f32 (row-major 3x4 skinning in model space).
pub fn moves_file(bones: usize, clips: &[Clip]) -> Vec<u8> {
    let mut out = b"GRMV".to_vec();
    for v in [2u32, bones as u32, clips.len() as u32] {
        out.extend(v.to_le_bytes());
    }
    for c in clips {
        out.extend((c.name.len() as u32).to_le_bytes());
        out.extend(c.name.as_bytes());
        out.extend(c.fps.to_le_bytes());
        out.extend(c.speed.to_le_bytes());
        out.extend((c.frames.len() as u32).to_le_bytes());
        for f in &c.frames {
            for m in f {
                for x in m {
                    out.extend(x.to_le_bytes());
                }
            }
        }
    }
    out
}

/// Kratos's moves for gowring.dll: the [`CLIPS`] decoded from his `ANM_heroa00` chunk and retargeted onto the player
/// skeleton `er`, as the moves file, and a line about them. `kratos` is his skeleton as read (unmirrored).
pub fn build_moves(anm: &[u8], kratos: &Skeleton, er: &[Node], scale: f32) -> Result<(Vec<u8>, String)> {
    let hado = Hado::parse(anm)?;
    let slots = slot_joints(&hado, kratos)?;
    let mirrored = kratos.mirrored_x();
    let r = crate::retarget::retarget(&mirrored, scale, er)?;
    let mut driven = vec![false; kratos.parent.len()];
    for j in slots.iter().flatten() {
        driven[*j] = true;
    }
    let mut clips = Vec::new();
    let mut notes = Vec::new();
    for (name, hash) in CLIPS {
        let Some(base) = find_clip(anm, hash) else {
            notes.push(format!("{name}: no clip {hash:#010x}"));
            continue;
        };
        // a clip that does not build is left out (the others still play)
        let built = decode_clip(anm, base, &hado, kratos, &slots).and_then(|(fps, world)| {
            let mut clip = retarget_clip(name, fps, &world, &mirrored, scale as f64, &r, er, &driven)?;
            // his root's travel along the ground over the loop, scaled as he is
            let (a, b) = (world.first().map(|w| w[ROOT_JOINT]), world.last().map(|w| w[ROOT_JOINT]));
            if let (Some(a), Some(b)) = (a, b) {
                let d = ((b[0][3] - a[0][3]).powi(2) + (b[2][3] - a[2][3]).powi(2)).sqrt() * scale as f64;
                let secs = (world.len().max(2) - 1) as f64 / fps.max(1.0) as f64;
                clip.speed = (d / secs) as f32;
            }
            clip.note = format!("{} at {:.2} m/s", clip.note, clip.speed);
            Ok(clip)
        });
        match built {
            Ok(clip) => {
                notes.push(clip.note.clone());
                clips.push(clip);
            }
            Err(e) => notes.push(format!("{name}: {e:#}")),
        }
    }
    let mapped = slots.iter().filter(|s| s.is_some()).count();
    Ok((moves_file(er.len(), &clips), format!("{mapped} of {} pose slots on his joints; {}", slots.len(), notes.join("; "))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_and_masks_read_msb_first() {
        let d = [0b1010_0000u8, 0xff];
        let mut b = Bits { d: &d, p: 0 };
        assert_eq!(b.read(3).unwrap(), 0b101);
        assert_eq!(b.read(5).unwrap(), 0);
        assert_eq!(b.read(8).unwrap(), 0xff);
        assert!(b.read(1).is_err());
        assert_eq!(popmask(&[0b1000_0001]), vec![0, 1, 8]);
    }

    #[test]
    fn quaternions_come_back_unit_with_the_largest_in_its_place() {
        // index 1: the largest (left out) goes to y; x, z centred at zero
        let q = q48([0x01, 0x00, 0x01, 0x80, 0x00, 0x80]);
        assert!((q.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-9, "{q:?}");
        assert!((q[1] - 1.0).abs() < 1e-3, "{q:?}");
        let s = smallest3((8u128 << 8 | 8 << 4 | 8) << 2 | 3, [4, 4, 8, 4]);
        assert!((s[3] - 1.0).abs() < 1e-9 && s[0].abs() < 1e-9, "{s:?}");
        assert!((f16(0x3c00) - 1.0).abs() < 1e-12 && (f16(0xc000) + 2.0).abs() < 1e-12);
    }

    #[test]
    fn interpolation_holds_the_ends_and_runs_straight_between_keys() {
        let v = interp(&[0, 4], &[vec![0.0], vec![4.0]], 6);
        assert_eq!(v.iter().map(|x| x[0]).collect::<Vec<_>>(), vec![0.0, 1.0, 2.0, 3.0, 4.0, 4.0]);
    }

    /// Against the research decoder on the player's own files: `GOWRING_ANM=0078_ANM_heroa00_id1_s2.bin
    /// GOWRING_GOPROTO=goProtoheroa00.bin GOWRING_ANIM_REF=dir` (dir holding `slot2joint.i32` and `idle_world.f32`
    /// from `anim.py`) `cargo test -p gowring-gow idle_matches -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn idle_matches_the_research_decoder() {
        let (Ok(anm), Ok(proto), Ok(dir)) = (std::env::var("GOWRING_ANM"), std::env::var("GOWRING_GOPROTO"), std::env::var("GOWRING_ANIM_REF")) else { return };
        let anm = std::fs::read(anm).unwrap();
        let sk = Skeleton::parse(&std::fs::read(proto).unwrap()).unwrap();
        let hado = Hado::parse(&anm).unwrap();
        let slots = slot_joints(&hado, &sk).unwrap();
        let dir = std::path::Path::new(&dir);
        let want: Vec<i32> = std::fs::read(dir.join("slot2joint.i32")).unwrap().chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
        let got: Vec<i32> = slots.iter().map(|s| s.map_or(-1, |j| j as i32)).collect();
        assert_eq!(got, want, "slot -> joint");
        let base = find_clip(&anm, CLIPS[0].1).unwrap();
        assert_eq!(base, 0xff610);
        let (fps, world) = decode_clip(&anm, base, &hado, &sk, &slots).unwrap();
        let reff: Vec<f32> = std::fs::read(dir.join("idle_world.f32")).unwrap().chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        let n = sk.parent.len();
        assert_eq!(reff.len(), world.len() * n * 16);
        let mut worst = 0.0f64;
        for (f, w) in world.iter().enumerate() {
            for (j, m) in w.iter().enumerate() {
                for k in 0..16 {
                    worst = worst.max((m[k / 4][k % 4] - reff[(f * n + j) * 16 + k] as f64).abs());
                }
            }
        }
        println!("idle: {} frames at {fps} fps, worst difference {worst:e}", world.len());
        assert!(worst < 1e-4, "{worst}");
    }

    /// Where the geometry on each of his joints goes on the Elden Ring bone it weighs on, against where his own
    /// animation takes it (frame 0): the worst joints, with their bone and whether a pose slot drives them.
    #[test]
    #[ignore]
    fn moves_joint_errors() {
        let (Ok(anm), Ok(proto), Ok(er)) = (std::env::var("GOWRING_ANM"), std::env::var("GOWRING_GOPROTO"), std::env::var("GOWRING_ER_SKELETON")) else { return };
        let anm = std::fs::read(anm).unwrap();
        let sk = Skeleton::parse(&std::fs::read(proto).unwrap()).unwrap();
        let er = crate::kratos_part::skeleton_nodes(&std::fs::read_to_string(er).unwrap()).unwrap();
        let hado = Hado::parse(&anm).unwrap();
        let slots = slot_joints(&hado, &sk).unwrap();
        let driven: std::collections::HashSet<usize> = slots.iter().flatten().copied().collect();
        let mirrored = sk.mirrored_x();
        let scale = crate::kratos_part::SCALE;
        let r = crate::retarget::retarget(&mirrored, scale, &er).unwrap();
        let (fps, world) = decode_clip(&anm, find_clip(&anm, CLIPS[0].1).unwrap(), &hado, &sk, &slots).unwrap();
        let flags: Vec<bool> = (0..sk.parent.len()).map(|j| driven.contains(&j)).collect();
        let clip = retarget_clip("idle", fps, &world, &mirrored, scale as f64, &r, &er, &flags).unwrap();
        for (b, a) in clip.anchors.iter().enumerate() {
            if let Some(j) = a {
                println!("anchor {} <- joint {j} (parent {}) {}", er[b].name, mirrored.parent[*j], if driven.contains(j) { "driven" } else { "PROCEDURAL" });
            }
        }
        // his own: geometry point p (his bind) goes to pinned animated x bind^-1 x p
        let n = sk.parent.len();
        let fix = |m: &M| -> M { let mut o = *m; for k in 1..4 { o[0][k] = -o[0][k]; o[k][0] = -o[k][0]; } for row in o.iter_mut().take(3) { row[3] *= scale as f64; } o };
        let mut kw: Vec<M> = world[0].iter().map(fix).collect();
        let mut rw: Vec<M> = Vec::new();
        for j in 0..n { let l = rows(&mirrored.local[j]); let p = mirrored.parent[j]; rw.push(if p < 0 { l } else { mul(&rw[p as usize], &l) }); }
        let mut root = rw[ROOT_JOINT]; for row in root.iter_mut().take(3) { row[3] *= scale as f64; }
        let pin = mul(&root, &invert(&kw[ROOT_JOINT]).unwrap());
        for m in kw.iter_mut() { *m = mul(&pin, m); }
        let mut rows_out = Vec::new();
        for j in 0..n {
            if !r.deforms[j] { continue; }
            let mut b = invert(&rows(&mirrored.inverse_bind[j])).unwrap(); for row in b.iter_mut().take(3) { row[3] *= scale as f64; }
            let p = r.bind_positions[j].map(|x| x as f64);
            let own = { let m = mul(&kw[j], &invert(&b).unwrap()); [0, 1, 2].map(|k| m[k][0] * p[0] + m[k][1] * p[1] + m[k][2] * p[2] + m[k][3]) };
            let mv = &r.moves[j];
            let q: [f64; 3] = std::array::from_fn(|row| mv[0][row] as f64 * p[0] + mv[1][row] as f64 * p[1] + mv[2][row] as f64 * p[2] + mv[3][row] as f64);
            let bone = r.bone[j] as usize;
            let s = &clip.frames[0][bone];
            let ours: [f64; 3] = std::array::from_fn(|k| s[4 * k] as f64 * q[0] + s[4 * k + 1] as f64 * q[1] + s[4 * k + 2] as f64 * q[2] + s[4 * k + 3] as f64);
            let e = ((own[0] - ours[0]).powi(2) + (own[1] - ours[1]).powi(2) + (own[2] - ours[2]).powi(2)).sqrt();
            rows_out.push((e, j, er[bone].name.clone(), driven.contains(&j), mirrored.parent[j], own, ours));
        }
        rows_out.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (e, j, b, d, par, own, ours) in rows_out.iter().take(40) {
            println!("{e:.3} m  joint {j} (parent {par}, {}) on {b}: his {:?} ours {:?}", if *d { "driven" } else { "procedural" }, own.map(|x| (x * 100.0).round() / 100.0), ours.map(|x| (x * 100.0).round() / 100.0));
        }
        let mut per_bone: std::collections::BTreeMap<String, (usize, f64)> = Default::default();
        for (e, _, b, ..) in &rows_out { let x = per_bone.entry(b.clone()).or_default(); x.0 += 1; x.1 = x.1.max(*e); }
        let mut pb: Vec<_> = per_bone.into_iter().collect(); pb.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
        println!("per bone (joints, worst m): {:?}", pb.iter().take(20).map(|(b, (c, e))| format!("{b} {c} {e:.2}")).collect::<Vec<_>>());
    }

    /// The moves file from the player's own files, for a look offline: as above plus `GOWRING_ER_SKELETON=
    /// er_skeleton.json GOWRING_MOVES_OUT=file` (`cargo test -p gowring-gow moves_from -- --ignored --nocapture`).
    /// The torso's and shoulders' bones: the joints whose skin goes to each, whether the clip drives each and its
    /// ancestors (clean), and the chain above each bone's anchor, with heights.
    #[test]
    #[ignore]
    fn torso_anchors() {
        let (Ok(anm), Ok(proto), Ok(er)) = (std::env::var("GOWRING_ANM"), std::env::var("GOWRING_GOPROTO"), std::env::var("GOWRING_ER_SKELETON")) else { return };
        let anm = std::fs::read(anm).unwrap();
        let sk = Skeleton::parse(&std::fs::read(proto).unwrap()).unwrap();
        let er = crate::kratos_part::skeleton_nodes(&std::fs::read_to_string(er).unwrap()).unwrap();
        let hado = Hado::parse(&anm).unwrap();
        let slots = slot_joints(&hado, &sk).unwrap();
        let mirrored = sk.mirrored_x();
        let r = crate::retarget::retarget(&mirrored, crate::kratos_part::SCALE, &er).unwrap();
        let n = sk.parent.len();
        let mut driven = vec![false; n];
        for j in slots.iter().flatten() {
            driven[*j] = true;
        }
        let mut clean = vec![false; n];
        for j in 0..n {
            let p = mirrored.parent[j];
            clean[j] = driven[j] && (p < 0 || clean[p as usize]);
        }
        let (fps, world) = decode_clip(&anm, find_clip(&anm, CLIPS[0].1).unwrap(), &hado, &sk, &slots).unwrap();
        let clip = retarget_clip("idle", fps, &world, &mirrored, crate::kratos_part::SCALE as f64, &r, &er, &driven).unwrap();
        let tag = |j: usize| format!("{j}{}{} y{:.2}", if driven[j] { "d" } else { "" }, if clean[j] { "c" } else { "" }, r.bind_positions[j][1]);
        for name in ["Pelvis", "Spine", "Spine1", "Spine2", "Neck", "Head", "L_Clavicle", "R_Clavicle", "L_UpperArm", "R_UpperArm", "L_Forearm", "L_Hand", "L_Thigh", "L_Calf"] {
            let b = er.iter().position(|x| x.name == name).unwrap();
            let members: Vec<String> = (0..n).filter(|j| r.deforms[*j] && r.bone[*j] as usize == b).map(tag).collect();
            let mut chain = Vec::new();
            let mut j = clip.anchors[b];
            while let Some(k) = j {
                chain.push(tag(k));
                j = usize::try_from(mirrored.parent[k]).ok();
            }
            println!("{name}: {} members [{}]\n    anchor chain {}", members.len(), members.join(", "), chain.join(" <- "));
        }
    }

    #[test]
    #[ignore]
    fn moves_from_real_files() {
        let (Ok(anm), Ok(proto), Ok(er), Ok(out)) = (std::env::var("GOWRING_ANM"), std::env::var("GOWRING_GOPROTO"), std::env::var("GOWRING_ER_SKELETON"), std::env::var("GOWRING_MOVES_OUT")) else { return };
        let anm = std::fs::read(anm).unwrap();
        let sk = Skeleton::parse(&std::fs::read(proto).unwrap()).unwrap();
        let er = crate::kratos_part::skeleton_nodes(&std::fs::read_to_string(er).unwrap()).unwrap();
        let (bytes, note) = build_moves(&anm, &sk, &er, crate::kratos_part::SCALE).unwrap();
        println!("{note}; {} bytes", bytes.len());
        std::fs::write(out, bytes).unwrap();
    }
}

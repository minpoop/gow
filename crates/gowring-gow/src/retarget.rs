//! Kratos re-posed into the Elden Ring player skeleton's rest pose, and his skin weights handed to its bones.
//!
//! Elden Ring skins a part with its own skeleton: a vertex follows bone b by `world(b) * inverse(rest world(b))`, so
//! the mesh has to sit in the skeleton's rest pose (a light A-pose, 1.75 m) and name its bones. Kratos's rest pose is
//! close (hips, spine, head and shoulders within a few centimetres once he is scaled to the player's height) but holds
//! his arms about 15 degrees higher and his feet wider apart.
//!
//! His skeleton (1458 joints, `goProto`) names joints only by hash, so the limbs are found by shape: his upper arm
//! (thigh) is the joint nearest Elden Ring's that starts a limb (at least 4 moving joints below it, reaching at least
//! 0.4 m: the nearest joint of all is often a lone muscle helper); the arm (leg) is the chain from it to its farthest
//! joint; the hand
//! is the chain joint with the most joints branching off it (the fingers), the elbow the chain joint halfway between
//! shoulder and hand; the foot is the chain joint nearest Elden Ring's foot, the knee halfway, the toe the next.
//! Each of those turns, in hierarchy order, so that it points from its joint to the next the way Elden Ring's bone
//! does; every other joint follows its parent. A vertex moves by the blend of its joints' moves (its own skin weights),
//! and each joint's weight goes to an Elden Ring bone: the limb joints to their bones, the rest to the nearest bone of
//! their own region (that arm, that leg, the head, the torso) in the new pose.

use anyhow::{bail, Result};

use crate::erpart::{world_matrices, Node};
use crate::kratos::Skeleton;

/// A transform as four columns (the last is the translation).
pub type M4 = [[f32; 4]; 4];

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}

pub fn apply(m: &M4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

pub fn rotate(m: &M4, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    len(sub(a, b))
}

/// The inverse of an affine transform (rotation and scale, then translation).
fn invert(m: &M4) -> Option<M4> {
    let a = |c: usize, r: usize| m[c][r] as f64;
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(2, 1) * a(1, 2)) - a(1, 0) * (a(0, 1) * a(2, 2) - a(2, 1) * a(0, 2)) + a(2, 0) * (a(0, 1) * a(1, 2) - a(1, 1) * a(0, 2));
    if det.abs() < 1e-12 {
        return None;
    }
    let mut o = IDENTITY;
    for c in 0..3 {
        for r in 0..3 {
            let (r1, r2, c1, c2) = ((c + 1) % 3, (c + 2) % 3, (r + 1) % 3, (r + 2) % 3);
            o[c][r] = ((a(c1, r1) * a(c2, r2) - a(c2, r1) * a(c1, r2)) / det) as f32;
        }
    }
    for r in 0..3 {
        o[3][r] = -(0..3).map(|k| o[k][r] * m[3][k]).sum::<f32>();
    }
    Some(o)
}

/// The smallest rotation taking direction `from` to direction `to`, about `pivot`.
fn turn(from: [f32; 3], to: [f32; 3], pivot: [f32; 3]) -> M4 {
    let (lf, lt) = (len(from), len(to));
    if lf < 1e-6 || lt < 1e-6 {
        return IDENTITY;
    }
    let (a, b) = (from.map(|x| x / lf), to.map(|x| x / lt));
    let c = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    let mut axis = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let s = len(axis);
    if s < 1e-6 {
        return IDENTITY; // already aligned (a half turn never comes up between two limbs pointing down and out)
    }
    axis = axis.map(|x| x / s);
    let (x, y, z) = (axis[0], axis[1], axis[2]);
    let t = 1.0 - c;
    // Rodrigues, as columns
    let r = [[t * x * x + c, t * x * y + s * z, t * x * z - s * y], [t * x * y - s * z, t * y * y + c, t * y * z + s * x], [t * x * z + s * y, t * y * z - s * x, t * z * z + c]];
    let mut m = IDENTITY;
    for col in 0..3 {
        m[col][..3].copy_from_slice(&r[col]);
    }
    // about the pivot: p' = R (p - pivot) + pivot
    let rp = rotate(&m, pivot);
    m[3] = [pivot[0] - rp[0], pivot[1] - rp[1], pivot[2] - rp[2], 1.0];
    m
}

/// Kratos re-posed onto an Elden Ring skeleton.
pub struct Retarget {
    /// Per Kratos joint: how geometry weighted to it moves from his bind pose to the new one.
    pub moves: Vec<M4>,
    /// Per Kratos joint: the Elden Ring bone (index into the skeleton's nodes) its weight goes to.
    pub bone: Vec<u8>,
    /// Kratos's joints that move geometry (their bind pose is known), in his bind pose (scaled).
    pub deforms: Vec<bool>,
    pub bind_positions: Vec<[f32; 3]>,
    /// How far the player skeleton's shoulders (upper arm bones) sit above Kratos's shoulder joints, on average (his
    /// torso is longer: about -5 cm); the arms turn about his own joints, [`crate::kratos_part::fit_proportions`] then
    /// moves them onto the player's.
    pub shoulder_step: f32,
    pub note: String,
}

/// The bones of the Elden Ring player skeleton that carry Kratos, by region.
fn region_bones(side: &str, part: &str) -> Vec<String> {
    let s = side;
    match part {
        "arm" => {
            let mut v: Vec<String> = ["UpperArm", "UpArmTwist", "UpArmTwist1", "Forearm", "ForeArmTwist", "ForeArmTwist1", "Hand"].iter().map(|b| format!("{s}_{b}")).collect();
            for f in 0..5 {
                v.push(format!("{s}_Finger{f}"));
                v.push(format!("{s}_Finger{f}1"));
                v.push(format!("{s}_Finger{f}2"));
            }
            v
        }
        "leg" => ["Thigh", "ThighTwist", "ThighTwist1", "Calf", "CalfTwist", "CalfTwist1", "Foot", "Toe0"].iter().map(|b| format!("{s}_{b}")).collect(),
        _ => ["Pelvis", "Spine", "Spine1", "Spine2", "Neck", "Head", "L_Clavicle", "R_Clavicle"].iter().map(|b| b.to_string()).collect(),
    }
}

/// Re-pose Kratos (`sk`, positions scaled by `scale`) onto the Elden Ring skeleton `er` (model nodes in its order).
pub fn retarget(sk: &Skeleton, scale: f32, er: &[Node]) -> Result<Retarget> {
    let n = sk.parent.len();
    let er_world = world_matrices(er);
    let er_index = |name: &str| er.iter().position(|b| b.name == name);
    let er_pos = |name: &str| -> Result<[f32; 3]> {
        let i = er_index(name).ok_or_else(|| anyhow::anyhow!("no {name} in the player skeleton"))?;
        let t = er_world[i][3];
        Ok([t[0], t[1], t[2]])
    };
    // his bind pose: the inverse of each inverse bind matrix, translation scaled
    let mut bind: Vec<M4> = Vec::with_capacity(n);
    let mut deforms = vec![false; n];
    for (j, ib) in sk.inverse_bind.iter().enumerate() {
        let m: M4 = std::array::from_fn(|c| std::array::from_fn(|r| ib[c * 4 + r]));
        let identity = (0..4).all(|c| (0..4).all(|r| (m[c][r] - IDENTITY[c][r]).abs() < 1e-6));
        let mut w = if identity { IDENTITY } else { invert(&m).unwrap_or(IDENTITY) };
        deforms[j] = !identity;
        for r in 0..3 {
            w[3][r] *= scale;
        }
        bind.push(w);
    }
    let pos: Vec<[f32; 3]> = bind.iter().map(|m| [m[3][0], m[3][1], m[3][2]]).collect();
    let mut children = vec![Vec::new(); n];
    for (j, p) in sk.parent.iter().enumerate() {
        if *p >= 0 {
            children[*p as usize].push(j);
        }
    }
    let nearest = |to: [f32; 3]| -> Option<usize> { (0..n).filter(|j| deforms[*j]).min_by(|a, b| dist(pos[*a], to).total_cmp(&dist(pos[*b], to))) };
    let descendants = |a: usize| -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![a];
        while let Some(j) = stack.pop() {
            out.push(j);
            stack.extend(children[j].iter().copied());
        }
        out
    };
    let reach = |a: usize| -> (usize, f32) {
        let below: Vec<usize> = descendants(a).into_iter().filter(|j| *j != a && deforms[*j]).collect();
        (below.len(), below.iter().map(|j| dist(pos[*j], pos[a])).fold(0.0, f32::max))
    };
    // the joint nearest `to` that starts a limb
    let limb_root = |to: [f32; 3]| -> Option<usize> {
        (0..n).filter(|j| deforms[*j]).filter(|j| dist(pos[*j], to) < 0.15).filter(|j| reach(*j).0 >= 4 && reach(*j).1 >= 0.4).min_by(|a, b| dist(pos[*a], to).total_cmp(&dist(pos[*b], to)))
    };
    // the chain from `a` to its farthest moving descendant
    let chain_from = |a: usize| -> Vec<usize> {
        let far = descendants(a).into_iter().filter(|j| deforms[*j]).max_by(|x, y| dist(pos[*x], pos[a]).total_cmp(&dist(pos[*y], pos[a]))).unwrap_or(a);
        let mut path = vec![far];
        let mut j = far;
        while j != a && sk.parent[j] >= 0 {
            j = sk.parent[j] as usize;
            if deforms[j] || j == a {
                path.push(j);
            }
        }
        path.reverse();
        path
    };
    let halfway = |path: &[usize], from: usize, to: usize| -> Option<usize> {
        let (i, k) = (path.iter().position(|j| *j == from)?, path.iter().position(|j| *j == to)?);
        path[i + 1..k].iter().copied().max_by(|x, y| dist(pos[*x], pos[from]).min(dist(pos[*x], pos[to])).total_cmp(&dist(pos[*y], pos[from]).min(dist(pos[*y], pos[to]))))
    };

    let mut note = String::new();
    // (Kratos joint, its chain child, the Elden Ring bone it becomes, the bone's own chain child or the points the
    // direction aims at)
    let mut aims: Vec<(usize, Vec<usize>, String, Vec<String>)> = Vec::new();
    let mut fixed: Vec<(usize, String)> = Vec::new();
    let mut regions: Vec<(Vec<usize>, Vec<String>)> = Vec::new();
    let mut shoulder_step = 0.0;
    for side in ["L", "R"] {
        // the arm
        let shoulder = limb_root(er_pos(&format!("{side}_UpperArm"))?).ok_or_else(|| anyhow::anyhow!("no {side} upper arm"))?;
        let path = chain_from(shoulder);
        let Some(hand) = path.iter().skip(1).copied().filter(|j| children[*j].iter().filter(|c| deforms[**c]).count() >= 3).max_by_key(|j| children[*j].iter().filter(|c| deforms[**c]).count()) else {
            bail!("no hand on the {side} arm chain ({} joints)", path.len());
        };
        let elbow = halfway(&path, shoulder, hand).ok_or_else(|| anyhow::anyhow!("no elbow on the {side} arm"))?;
        shoulder_step += (er_pos(&format!("{side}_UpperArm"))?[1] - pos[shoulder][1]) / 2.0;
        let fingers: Vec<usize> = children[hand].iter().copied().filter(|c| deforms[*c]).collect();
        aims.push((shoulder, vec![elbow], format!("{side}_UpperArm"), vec![format!("{side}_Forearm")]));
        aims.push((elbow, vec![hand], format!("{side}_Forearm"), vec![format!("{side}_Hand")]));
        aims.push((hand, fingers.clone(), format!("{side}_Hand"), (0..5).map(|f| format!("{side}_Finger{f}")).collect()));
        let _ = write_note(&mut note, side, "arm", &[("shoulder", shoulder), ("elbow", elbow), ("hand", hand)], &pos, er_pos(&format!("{side}_UpperArm"))?);
        regions.push((descendants(shoulder), region_bones(side, "arm")));
        // the leg
        let thigh = limb_root(er_pos(&format!("{side}_Thigh"))?).ok_or_else(|| anyhow::anyhow!("no {side} thigh"))?;
        let path = chain_from(thigh);
        let er_foot = er_pos(&format!("{side}_Foot"))?;
        let foot = path.iter().skip(1).copied().min_by(|x, y| dist(pos[*x], er_foot).total_cmp(&dist(pos[*y], er_foot))).ok_or_else(|| anyhow::anyhow!("no foot"))?;
        let knee = halfway(&path, thigh, foot).ok_or_else(|| anyhow::anyhow!("no knee on the {side} leg"))?;
        aims.push((thigh, vec![knee], format!("{side}_Thigh"), vec![format!("{side}_Calf")]));
        aims.push((knee, vec![foot], format!("{side}_Calf"), vec![format!("{side}_Foot")]));
        if let Some(toe) = path.iter().position(|j| *j == foot).and_then(|i| path.get(i + 1)).copied() {
            aims.push((foot, vec![toe], format!("{side}_Foot"), vec![format!("{side}_Toe0")]));
            fixed.push((toe, format!("{side}_Toe0")));
        }
        let _ = write_note(&mut note, side, "leg", &[("thigh", thigh), ("knee", knee), ("foot", foot)], &pos, er_pos(&format!("{side}_Thigh"))?);
        regions.push((descendants(thigh), region_bones(side, "leg")));
    }
    let head = nearest(er_pos("Head")?).ok_or_else(|| anyhow::anyhow!("no head"))?;
    regions.push((descendants(head), vec!["Head".into()]));
    fixed.push((head, "Head".into()));
    for (j, _, b, _) in &aims {
        fixed.push((*j, b.clone()));
    }

    // the new pose, parents first
    let mut moves = vec![IDENTITY; n];
    for j in 0..n {
        let mut m = if sk.parent[j] >= 0 { moves[sk.parent[j] as usize] } else { IDENTITY };
        if let Some((_, next, bone, targets)) = aims.iter().find(|a| a.0 == j) {
            let p = apply(&m, pos[j]);
            let mean = |ps: Vec<[f32; 3]>| -> [f32; 3] {
                let k = ps.len().max(1) as f32;
                ps.iter().fold([0.0; 3], |s, q| [s[0] + q[0] / k, s[1] + q[1] / k, s[2] + q[2] / k])
            };
            let now = mean(next.iter().map(|c| apply(&m, pos[*c])).collect());
            let from_bone = er_pos(bone)?;
            let to = mean(targets.iter().filter_map(|t| er_pos(t).ok()).collect());
            m = mul(&turn(sub(now, p), sub(to, from_bone), p), &m);
        }
        moves[j] = m;
    }

    // whose weight goes where
    let er_has = |name: &str| er_index(name).is_some();
    let mut bone = vec![0u8; n];
    for j in 0..n {
        if !deforms[j] {
            continue;
        }
        let at = apply(&moves[j], pos[j]);
        let name = if let Some((_, b)) = fixed.iter().find(|f| f.0 == j) {
            b.clone()
        } else {
            let names = regions.iter().find(|(members, _)| members.contains(&j)).map(|r| r.1.clone()).unwrap_or_else(|| region_bones("", "torso"));
            names.into_iter().filter(|b| er_has(b)).min_by(|a, b| dist(er_pos(a).unwrap(), at).total_cmp(&dist(er_pos(b).unwrap(), at))).unwrap_or_else(|| "Spine1".into())
        };
        bone[j] = er_index(&name).unwrap_or(0).min(255) as u8;
    }
    Ok(Retarget { moves, bone, deforms, bind_positions: pos, shoulder_step, note })
}

fn write_note(note: &mut String, side: &str, limb: &str, joints: &[(&str, usize)], pos: &[[f32; 3]], er_root: [f32; 3]) -> std::fmt::Result {
    use std::fmt::Write as _;
    write!(note, "{side} {limb}:")?;
    for (what, j) in joints {
        let p = pos[*j];
        write!(note, " {what} {j} ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2])?;
    }
    write!(note, " [Elden Ring root ({:.2}, {:.2}, {:.2})]; ", er_root[0], er_root[1], er_root[2])
}

/// One vertex skinned by Kratos's joints `influences` (joint, weight): its new position and normal, and its four
/// strongest Elden Ring bones with their weights (summing to 1).
pub fn skin(r: &Retarget, p: [f32; 3], n: [f32; 3], influences: &[(u16, f32)]) -> ([f32; 3], [f32; 3], [u8; 4], [f32; 4]) {
    let mut np = [0.0f32; 3];
    let mut nn = [0.0f32; 3];
    let mut total = 0.0;
    let mut per_bone: Vec<(u8, f32)> = Vec::new();
    for &(j, w) in influences {
        let j = j as usize;
        if w <= 0.0 || j >= r.moves.len() {
            continue;
        }
        let q = apply(&r.moves[j], p);
        let m = rotate(&r.moves[j], n);
        for k in 0..3 {
            np[k] += w * q[k];
            nn[k] += w * m[k];
        }
        total += w;
        let b = r.bone[j];
        match per_bone.iter_mut().find(|e| e.0 == b) {
            Some(e) => e.1 += w,
            None => per_bone.push((b, w)),
        }
    }
    if total <= 0.0 {
        return (p, n, [0; 4], [1.0, 0.0, 0.0, 0.0]);
    }
    let np = np.map(|x| x / total);
    let l = len(nn).max(1e-6);
    let nn = nn.map(|x| x / l);
    per_bone.sort_by(|a, b| b.1.total_cmp(&a.1));
    per_bone.truncate(4);
    let sum: f32 = per_bone.iter().map(|e| e.1).sum();
    let mut bones = [0u8; 4];
    let mut weights = [0.0f32; 4];
    for (k, (b, w)) in per_bone.iter().enumerate() {
        bones[k] = *b;
        weights[k] = w / sum;
    }
    (np, nn, bones, weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_takes_one_direction_onto_another_about_its_pivot() {
        let m = turn([1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [1.0, 1.0, 0.0]);
        let p = apply(&m, [2.0, 1.0, 0.0]);
        assert!((p[0] - 1.0).abs() < 1e-5 && (p[1] - 0.0).abs() < 1e-5 && p[2].abs() < 1e-5, "{p:?}");
        let q = apply(&m, [1.0, 1.0, 0.0]);
        assert!((q[0] - 1.0).abs() < 1e-5 && (q[1] - 1.0).abs() < 1e-5);
        let inv = invert(&m).unwrap();
        let back = apply(&inv, p);
        assert!((back[0] - 2.0).abs() < 1e-5 && (back[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_vertex_takes_its_joints_moves_and_their_bones() {
        let r = Retarget {
            moves: vec![IDENTITY, turn([1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 0.0])],
            bone: vec![8, 56],
            deforms: vec![true, true],
            bind_positions: vec![[0.0; 3]; 2],
            shoulder_step: 0.0,
            note: String::new(),
        };
        let (p, _, bones, weights) = skin(&r, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], &[(1, 0.75), (0, 0.25)]);
        // three quarters of the way to the turned position
        assert!((p[0] - 0.25).abs() < 1e-5 && (p[1] + 0.75).abs() < 1e-5, "{p:?}");
        assert_eq!(bones, [56, 8, 0, 0]);
        assert!((weights[0] - 0.75).abs() < 1e-6 && (weights[1] - 0.25).abs() < 1e-6);
    }
}

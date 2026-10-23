//! Kratos's cloak on Elden Ring's helper bones under his spine and pelvis, for gowring.dll to swing (its `cape`
//! module). Elden Ring moves capes with Havok cloth data in each armour's files, which the mod cannot make. What the
//! mod can do (the pose probes, 0.1.67-0.1.69): write a per-bone offset the game applies in rest space under the
//! animation (a bone's skinning matrix becomes its own times the offset, not passed to its children). So the back of
//! the cloak below his shoulder blades is skinned to two chains of helper bones that hang off his spine and pelvis and
//! that his mesh uses nowhere else ([`CHAINS`]: the Mantle bones, a SpineArmor bone, the pelvis dummies), and the DLL
//! writes each one's offset from a small simulation of its own: the cloak bends in rest space and his spine and hips
//! carry it. If the DLL writes nothing, those bones follow his spine and hips and the cloak stays as before.
//!
//! The rig: each chain is [`POINTS`] points from the height of the anchor bone ([`ANCHOR`], where the cloak starts to
//! hang free) down to the hem, each the mean of that side's cloak vertices behind the body at that height (rest pose,
//! model space). A cloak vertex below the anchor, behind the body or at its sides ([`SIDES`]), takes the chain's weights
//! by its height (the two points it lies between) and its side (both chains near the middle), blended in over [`BLEND`]
//! below the anchor, and keeps the rest of its own; the four largest are kept.
//!
//! The fur over his shoulders and upper arms is draped first: Elden Ring's rest pose holds the arms raised and out
//! (about 20 degrees below level), so the fur outside the shoulder joint and on top of that arm (shoulder to elbow) is
//! turned down about the shoulder by up to [`DROP_DEG`], toward where the arm hangs, and that share of its arm weights
//! goes to the shoulder's clavicle: it hangs from his shoulder and the chains, and the arm moves under it.
//! 0.1.72-0.1.79 hung it on the chains where the raised arm is, and it stood off his lowered arm like a bell ("there
//! seems to be like a air pocket here"); 0.1.80 left it on the arm, and with the arm forward it stretched from the
//! shoulder past his hip ("its now just stretching lower").
//!
//! 0.1.70-0.1.71 hung the chains from mid-back (`Spine1`, 1.15 m) with a 0.25 m blend: the top half of the bear cloak
//! stayed on his torso and arms and only the part below his waist swung ("only the bottom of the cloak is moving"), and
//! the cloak by his right arm, which hangs lower on that side, sat in the blend half on the arm ("a section near his
//! right arm that looks like it's stuck").

use crate::erpart::{world_matrices, Mesh, Node};

/// The chains, top to hem: on his left, on his right. Each bone follows the body near its point's height: the top
/// (by his shoulder blades) his upper spine (Spine2, Spine1), the next (by his waist) his lower spine (Spine, Spine1),
/// the lower two his pelvis, so the cloak's top follows his back and its lower half his hips.
pub const CHAINS: [[&str; POINTS]; 2] = [["Spine2_Mantle", "Spine_Mantle", "Pelvis_Mantle", "L_Thigh_Root_Dummy"], ["SpineArmor2", "Spine1_Mantle", "Pelvis_Dummy", "R_Thigh_Root_Dummy"]];
/// Points (bones) per chain.
pub const POINTS: usize = 4;
/// The bone whose height the chains hang from (his shoulders, 1.39 m): the cloak above it stays on his shoulders.
pub const ANCHOR: &str = "L_Clavicle";
/// How far below the anchor the cloak is wholly on the chains (m).
pub const BLEND: f32 = 0.15;
/// How far behind the body's middle a vertex is wholly on the chains (m; from 0 at the middle).
const BEHIND: f32 = 0.08;
/// How far in front of the body's middle the cloak's sides start to follow the chains (m): the cloak hanging beside
/// his arms goes with the back of it.
const SIDES: f32 = 0.03;
/// How far the fur on his arms is turned down about the shoulder (degrees, for a vertex wholly on the arm): from the
/// rest pose's arm, about 20 degrees below level, to about 60, short of where the arm hangs so it stays outside it.
pub const DROP_DEG: f32 = 40.0;
/// The fur draped is outside the shoulder joint (from this far inside it, m) and on top of the arm, each over this
/// ramp (m). By where it lies, not by its weights: the cloak copies the skin's weights under it, and his upper back's
/// skin is half on the upper arms right up to his spine (0.1.80's rule by weights draped the whole upper cloak).
const DRAPE_INSIDE: f32 = 0.03;
const DRAPE_RAMP: f32 = 0.08;

/// `v` turned by `angle` (radians) about the unit `axis` (Rodrigues).
fn turned(v: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    let k = axis;
    let kxv = [k[1] * v[2] - k[2] * v[1], k[2] * v[0] - k[0] * v[2], k[0] * v[1] - k[1] * v[0]];
    let kv = dot(k, v);
    std::array::from_fn(|i| v[i] * c + kxv[i] * s + k[i] * kv * (1.0 - c))
}
/// The width over which the two chains share a vertex at his back's middle (m).
const MIDDLE: f32 = 0.12;

/// The rig gowring.dll reads (`kratos_cape.json` beside the parts): chains of bone names and rest points.
#[derive(Debug, Clone, PartialEq)]
pub struct Rig {
    pub anchor: String,
    pub chains: Vec<(Vec<String>, Vec<[f32; 3]>)>,
    /// Cloak vertices that now follow the chains, at least in part.
    pub vertices: usize,
    /// Cloak vertices draped off the arms (see the module notes).
    pub draped: usize,
}

impl Rig {
    pub fn json(&self) -> String {
        let chains: Vec<String> = self
            .chains
            .iter()
            .map(|(bones, points)| {
                let b: Vec<String> = bones.iter().map(|n| format!("{n:?}")).collect();
                let p: Vec<String> = points.iter().map(|q| format!("[{:.4}, {:.4}, {:.4}]", q[0], q[1], q[2])).collect();
                format!("    {{\"bones\": [{}], \"points\": [{}]}}", b.join(", "), p.join(", "))
            })
            .collect();
        format!("{{\n  \"anchor\": {:?},\n  \"vertices\": {},\n  \"chains\": [\n{}\n  ]\n}}\n", self.anchor, self.vertices, chains.join(",\n"))
    }
}

/// The rig file when there is no rig (no cloak, no chain bones, or the cape is off): no chains, so the DLL leaves the
/// bones alone.
pub fn no_rig_json() -> String {
    "{\n  \"anchor\": \"\",\n  \"vertices\": 0,\n  \"chains\": []\n}\n".into()
}

fn horizontal(v: [f32; 3]) -> [f32; 3] {
    [v[0], 0.0, v[2]]
}

fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
    let l = dot(v, v).sqrt();
    (l > 1e-4).then(|| [v[0] / l, v[1] / l, v[2] / l])
}

/// Skin the cloak meshes (Kratos materials in `cloaks`; `used` maps a mesh's material index to Kratos's) to the chains
/// of `skeleton` (the FLVER's nodes, whose indices the vertices' bones are). `None` when a bone it needs is missing or
/// the cloak has nothing behind the body below the anchor.
pub fn rig(meshes: &mut [Mesh], used: &[u16], cloaks: &[u16], skeleton: &[Node]) -> Option<Rig> {
    let index = |name: &str| skeleton.iter().position(|n| n.name == name);
    let world = world_matrices(skeleton);
    let at = |name: &str| -> Option<[f32; 3]> { index(name).map(|i| [world[i][3][0], world[i][3][1], world[i][3][2]]) };
    // his front, by his toes; his left, by his thighs
    let toes = [sub(at("L_Toe0")?, at("L_Foot")?), sub(at("R_Toe0")?, at("R_Foot")?)];
    let forward = unit(horizontal([toes[0][0] + toes[1][0], 0.0, toes[0][2] + toes[1][2]]))?;
    let left = unit(horizontal(sub(at("L_Thigh")?, at("R_Thigh")?)))?;
    let middle = at("Pelvis")?;
    let top = at(ANCHOR)?[1];
    let bones: Vec<Vec<u8>> = CHAINS.iter().map(|c| c.iter().map(|n| index(n).and_then(|i| u8::try_from(i).ok())).collect::<Option<Vec<u8>>>()).collect::<Option<Vec<_>>>()?;
    let kratos = |m: &Mesh| used.get(m.material as usize).copied().unwrap_or(u16::MAX);
    // his arms' bones (the upper arms and everything below them), by side (true: his left)
    let arm_side: Vec<Option<bool>> = (0..skeleton.len())
        .map(|i| {
            let mut j = i as i32;
            while j >= 0 {
                match skeleton[j as usize].name.as_str() {
                    "L_UpperArm" => return Some(true),
                    "R_UpperArm" => return Some(false),
                    _ => j = skeleton[j as usize].parent as i32,
                }
            }
            None
        })
        .collect();
    // the fur on his arms draped: what lies outside the shoulder joint and on top of the rest pose's arm (shoulder to
    // elbow) is turned down about the shoulder, toward where the arm hangs, and that share of its arm weights goes to
    // the clavicle
    let ends = [(at("L_UpperArm")?, at("L_Elbow")?), (at("R_UpperArm")?, at("R_Elbow")?)];
    let clavicles = [index("L_Clavicle").and_then(|i| u8::try_from(i).ok())?, index("R_Clavicle").and_then(|i| u8::try_from(i).ok())?];
    // the turn about his forward axis that takes his left downward
    let lowers_left = if turned(left, forward, 0.1)[1] < 0.0 { 1.0 } else { -1.0 };
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let middle_x = at("Pelvis")?;
    let mut draped = 0;
    for m in meshes.iter_mut().filter(|m| cloaks.contains(&kratos(m))) {
        for v in m.vertices.iter_mut() {
            let s = if dot(horizontal(sub(v.position, middle_x)), left) >= 0.0 { 0 } else { 1 };
            let (shoulder, elbow) = ends[s];
            let Some(along) = unit(sub(elbow, shoulder)) else { continue };
            let up = [0.0, 1.0, 0.0];
            let Some(above) = unit(sub(up, scale3(along, dot(up, along)))) else { continue };
            let r = sub(v.position, shoulder);
            let w = smooth((dot(r, along) + DRAPE_INSIDE) / DRAPE_RAMP) * smooth(dot(r, above) / DRAPE_RAMP);
            if w <= 0.0 {
                continue;
            }
            let angle = DROP_DEG.to_radians() * w * lowers_left * if s == 0 { 1.0 } else { -1.0 };
            v.position = add3(turned(r, forward, angle), shoulder);
            v.normal = turned(v.normal, forward, angle);
            let t = turned([v.tangent[0], v.tangent[1], v.tangent[2]], forward, angle);
            v.tangent = [t[0], t[1], t[2], v.tangent[3]];
            // that share of the arm's weights to the clavicle, one entry per bone
            let mut merged: Vec<(u8, f32)> = Vec::new();
            for (b, wt) in v.bones.iter().zip(v.weights) {
                if wt <= 0.0 {
                    continue;
                }
                let parts = if arm_side.get(*b as usize).copied().flatten().is_some() { [(clavicles[s], wt * w), (*b, wt * (1.0 - w))] } else { [(*b, wt), (*b, 0.0)] };
                for (b, wt) in parts {
                    match merged.iter_mut().find(|e| e.0 == b) {
                        Some(e) => e.1 += wt,
                        None => merged.push((b, wt)),
                    }
                }
            }
            merged.retain(|e| e.1 > 0.0);
            merged.sort_by(|a, b| b.1.total_cmp(&a.1));
            merged.truncate(4);
            let total: f32 = merged.iter().map(|e| e.1).sum();
            if total > 0.0 {
                v.bones = [0; 4];
                v.weights = [0.0; 4];
                for (i, (b, wt)) in merged.into_iter().enumerate() {
                    v.bones[i] = b;
                    v.weights[i] = wt / total;
                }
            }
            draped += 1;
        }
    }
    // the cloak behind the body below the anchor, with its side
    let behind = |p: [f32; 3]| -dot(horizontal(sub(p, middle)), forward);
    let side = |p: [f32; 3]| dot(horizontal(sub(p, middle)), left);
    let back: Vec<[f32; 3]> = meshes.iter().filter(|m| cloaks.contains(&kratos(m))).flat_map(|m| m.vertices.iter().map(|v| v.position)).filter(|p| p[1] < top && behind(*p) > 0.0).collect();
    if back.len() < POINTS * 4 {
        return None;
    }
    let bottom = back.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    if top - bottom < 0.2 {
        return None;
    }
    let spacing = (top - bottom) / (POINTS - 1) as f32;
    // each chain's points: the mean of its side's cloak at each height (a height with none takes its neighbours')
    let mut chains = Vec::new();
    for (c, chain_bones) in CHAINS.iter().enumerate() {
        let mine = |p: &&[f32; 3]| if c == 0 { side(**p) >= 0.0 } else { side(**p) < 0.0 };
        let mut points: Vec<Option<[f32; 3]>> = (0..POINTS)
            .map(|k| {
                let y = top - spacing * k as f32;
                let near: Vec<&[f32; 3]> = back.iter().filter(mine).filter(|p| (p[1] - y).abs() <= spacing * 0.5 + 0.02).collect();
                (!near.is_empty()).then(|| {
                    let n = near.len() as f32;
                    let s = near.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
                    [s[0] / n, y, s[2] / n]
                })
            })
            .collect();
        if points.iter().filter(|p| p.is_some()).count() < POINTS / 2 {
            return None;
        }
        for k in 0..POINTS {
            if points[k].is_none() {
                let before = (0..k).rev().find_map(|j| points[j]);
                let after = (k + 1..POINTS).find_map(|j| points[j]);
                let y = top - spacing * k as f32;
                points[k] = match (before, after) {
                    (Some(a), Some(b)) => Some([(a[0] + b[0]) / 2.0, y, (a[2] + b[2]) / 2.0]),
                    (Some(a), None) | (None, Some(a)) => Some([a[0], y, a[2]]),
                    (None, None) => None,
                };
            }
        }
        let names = chain_bones.iter().map(|n| n.to_string()).collect();
        chains.push((names, points.into_iter().collect::<Option<Vec<_>>>()?));
    }
    // the weights
    let mut moved = 0;
    for m in meshes.iter_mut().filter(|m| cloaks.contains(&kratos(m))) {
        for v in m.vertices.iter_mut() {
            let p = v.position;
            let down = ((top - p[1]) / BLEND).clamp(0.0, 1.0);
            let back_share = ((behind(p) + SIDES) / (BEHIND + SIDES)).clamp(0.0, 1.0);
            let b = down * down * (3.0 - 2.0 * down) * back_share;
            if b <= 0.0 {
                continue;
            }
            let t = ((top - p[1]) / spacing).clamp(0.0, (POINTS - 1) as f32);
            let k0 = (t.floor() as usize).min(POINTS - 2);
            let fr = t - k0 as f32;
            let l = (0.5 + side(p) / MIDDLE).clamp(0.0, 1.0);
            let mut all: Vec<(u8, f32)> = Vec::with_capacity(8);
            for (bone, w) in v.bones.iter().zip(v.weights) {
                if w > 0.0 {
                    all.push((*bone, w * (1.0 - b)));
                }
            }
            for (c, share) in [(0, l), (1, 1.0 - l)] {
                all.push((bones[c][k0], b * share * (1.0 - fr)));
                all.push((bones[c][k0 + 1], b * share * fr));
            }
            // one entry per bone, the four largest, summing to 1
            let mut merged: Vec<(u8, f32)> = Vec::new();
            for (bone, w) in all {
                match merged.iter_mut().find(|e| e.0 == bone) {
                    Some(e) => e.1 += w,
                    None => merged.push((bone, w)),
                }
            }
            merged.sort_by(|a, b| b.1.total_cmp(&a.1));
            merged.truncate(4);
            let total: f32 = merged.iter().map(|e| e.1).sum();
            if total <= 0.0 {
                continue;
            }
            v.bones = [0; 4];
            v.weights = [0.0; 4];
            for (i, (bone, w)) in merged.into_iter().enumerate() {
                v.bones[i] = bone;
                v.weights[i] = w / total;
            }
            moved += 1;
        }
    }
    Some(Rig { anchor: ANCHOR.into(), chains, vertices: moved, draped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erpart::Vertex;

    fn node(name: &str, parent: i16, t: [f32; 3]) -> Node {
        Node { name: name.into(), parent, t, r: [0.0; 3], s: [1.0; 3], flags: 8 }
    }

    /// A skeleton facing +z with his left at +x, the chain bones (from index 10) under the spine and pelvis.
    fn skeleton() -> Vec<Node> {
        let mut s = vec![node("Master", -1, [0.0; 3]), node("Pelvis", 0, [0.0, 1.0, 0.0]), node("Spine1", 1, [0.0, 0.3, 0.0]), node("L_Clavicle", 2, [0.02, 0.0, 0.0])];
        for (side, x) in [("L", 0.1), ("R", -0.1)] {
            let thigh = s.len() as i16;
            s.push(node(&format!("{side}_Thigh"), 1, [x, -0.1, 0.0]));
            s.push(node(&format!("{side}_Foot"), thigh, [0.0, -0.8, 0.0]));
            s.push(node(&format!("{side}_Toe0"), thigh + 1, [0.0, 0.0, 0.15]));
        }
        for c in CHAINS {
            for (k, n) in c.iter().enumerate() {
                s.push(node(n, if k < 2 { 2 } else { 1 }, [0.0; 3]));
            }
        }
        s.push(node("L_UpperArm", 3, [0.15, 0.0, 0.0]));
        s.push(node("R_Clavicle", 2, [-0.02, 0.0, 0.0]));
        s.push(node("R_UpperArm", 2, [-0.17, 0.0, 0.0]));
        // the elbows about 20 degrees below level, as in Elden Ring's rest pose
        s.push(node("L_Elbow", 3, [0.39, -0.0874, 0.0]));
        s.push(node("R_Elbow", 2, [-0.41, -0.0874, 0.0]));
        s
    }

    fn vertex(p: [f32; 3]) -> Vertex {
        Vertex { position: p, normal: [0.0, 0.0, -1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [2, 0, 0, 0], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.0; 2]; 2] }
    }

    #[test]
    fn the_back_of_the_cloak_below_the_anchor_hangs_from_the_chains_and_the_rest_stays_on_the_torso() {
        let sk = skeleton();
        // a cloak behind him (z < 0) from his shoulders (1.6 m) to his knees (0.5 m), 30 cm wide, and a front flap
        let mut verts = Vec::new();
        for i in 0..23 {
            for j in 0..7 {
                verts.push(vertex([-0.15 + 0.05 * j as f32, 1.6 - 0.05 * i as f32, -0.15]));
            }
        }
        // on his arm (the skin under it is the upper arm's), half on it, beside him (at his body's middle), then the
        // front flap
        let arm = sk.iter().position(|n| n.name == "L_UpperArm").unwrap() as u8;
        verts.push(Vertex { bones: [arm, 0, 0, 0], ..vertex([0.3, 1.36, -0.05]) });
        verts.push(Vertex { bones: [arm, 2, 0, 0], weights: [0.5, 0.5, 0.0, 0.0], ..vertex([0.3, 1.15, -0.05]) });
        verts.push(vertex([0.2, 0.9, 0.0]));
        verts.push(vertex([0.1, 0.9, 0.2]));
        let mut meshes = vec![Mesh { material: 0, node: 0, vertices: verts, indices: vec![0, 1, 2] }];
        let rig = rig(&mut meshes, &[24], &[24], &sk).expect("a rig");
        assert_eq!(rig.chains.len(), 2);
        let (bones, points) = &rig.chains[0];
        assert_eq!(bones[0], "Spine2_Mantle");
        assert_eq!(points.len(), POINTS);
        // from the anchor's height (1.3 m) down to the hem, on his left (+x) and behind him
        assert!((points[0][1] - 1.3).abs() < 1e-4 && (points[POINTS - 1][1] - 0.5).abs() < 1e-4);
        assert!(points.iter().all(|p| p[0] > 0.0 && p[2] < 0.0));
        assert!(rig.chains[1].1.iter().all(|p| p[0] < 0.0));
        let v = &meshes[0].vertices;
        let chain_share = |v: &Vertex| v.bones.iter().zip(v.weights).filter(|(b, _)| (10..10 + 2 * POINTS).contains(&(**b as usize))).map(|(_, w)| w).sum::<f32>();
        // above the anchor: untouched; at the hem: wholly on the chains, summing to 1; the front flap: untouched
        assert_eq!(chain_share(&v[0]), 0.0);
        let hem = &v[22 * 7 + 6];
        assert!((chain_share(hem) - 1.0).abs() < 1e-5 && (hem.weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert_eq!(chain_share(v.last().unwrap()), 0.0);
        // the cloak beside him goes partly with the back of it; on top of his arm (shoulder at 1.3 m, 0.17 out, elbow
        // 20 degrees below) it is draped: turned down about the shoulder, its arm weight on the clavicle; under the arm
        // it stays where it is
        let beside = chain_share(&v[v.len() - 2]);
        assert!((0.2..0.35).contains(&beside), "{beside}");
        let (on, under) = (&v[v.len() - 4], &v[v.len() - 3]);
        assert!(on.bones.iter().zip(on.weights).all(|(b, w)| w == 0.0 || *b != arm), "{:?} {:?}", on.bones, on.weights);
        let (s, c) = DROP_DEG.to_radians().sin_cos();
        let (x, y) = (0.13f32, 0.06f32);
        let (x, y) = (0.17 + x * c + y * s, 1.3 - x * s + y * c);
        assert!((on.position[0] - x).abs() < 1e-4 && (on.position[1] - y).abs() < 1e-4, "{:?} vs ({x}, {y})", on.position);
        assert_eq!(under.position, [0.3, 1.15, -0.05]);
        assert_eq!(rig.draped, 1);
        // a vertex at his back's middle is shared by both chains
        let mid = &v[20 * 7 + 3];
        let left = mid.bones.iter().zip(mid.weights).filter(|(b, _)| (10..10 + POINTS).contains(&(**b as usize))).map(|(_, w)| w).sum::<f32>();
        assert!((left - 0.5).abs() < 0.05);
        assert!(rig.vertices > 0 && rig.vertices < v.len());
        // the file names the chains
        assert!(rig.json().contains("\"R_Thigh_Root_Dummy\""));
        // no chain bones: no rig
        let short: Vec<Node> = sk.iter().filter(|n| !n.name.contains("Mantle")).cloned().collect();
        assert!(rig_none(&short));
    }

    fn rig_none(sk: &[Node]) -> bool {
        let mut meshes = vec![Mesh { material: 0, node: 0, vertices: vec![vertex([0.0, 1.0, -0.15]); 40], indices: vec![] }];
        rig(&mut meshes, &[24], &[24], sk).is_none()
    }
}

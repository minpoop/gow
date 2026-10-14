//! Kratos as an Elden Ring armour part, built on the player's PC from their Ragnarök install: his default outfit
//! ([`crate::kratos::DEFAULT_OUTFIT`], most detailed level) read from `r_heroa00.wad` and `root.lodpack`, written as
//! armour parts with [`crate::erpart`] (see [`build_parts`]).
//!
//! Scaled to the player's height, with placeholder colours. When the game has given us its player skeleton
//! (`er_skeleton.json`, which gowring.dll reads from memory on the player's PC), the model carries all of its bones at
//! their rest pose, as the game's own parts do, and Kratos is re-posed into that rest pose and skinned to its bones
//! ([`crate::retarget`]), so he moves with the player. Without it he follows the root bone rigidly. Each of his
//! materials gets its own colour map and surface (normal and roughness) from the player's install
//! ([`crate::kratos_look`]), re-encoded as BC1 with their mips, with tangents from his UVs; layers drawn through transparency in Ragnarök (face overlays, eyelashes, the long beard's hair cards) are left
//! out until a cut-out material carries them.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::erpart::{self, Material, Mesh, Node, Vertex};
use crate::kratos::{draw_vertices, mesh_records, mg_parts, DEFAULT_OUTFIT};

/// The armour model id the part replaces (`bd_m_9701`, `bd_f_9701`): unused by the game.
pub const MODEL_ID: u16 = 9701;
/// Kratos is 1.94 m in his rest pose; the player is about 1.78 m.
pub const SCALE: f32 = 0.92;

/// Materials of the default outfit drawn through transparency in Ragnarök, which an opaque material would turn into
/// shells and slabs, left out: the eye shadow border (3), a face layer (5), tear line (6), the snow flakes decal (76)
/// and the eyes' outer shell (78).
pub const SEE_THROUGH: [u16; 5] = [3, 5, 6, 76, 78];
/// Materials made of strands on cards: the eyebrows (4), eyelashes (7) and the long beard (86, 87). They are drawn
/// with Elden Ring's cut-out material when their look has a coverage (see [`crate::kratos_look::cut_out_image`]) and
/// left out otherwise.
pub const CUT_OUT: [u16; 4] = [4, 7, 86, 87];
/// Elden Ring's cut-out character material: Torrent's body (the `C[AMSN][PixelKill]` shader, the armour's own with
/// pixels under half albedo alpha not drawn, and the same vertex layout), with its three samplers.
pub const CUT_OUT_MTD: &str = "N:\\GR\\data\\Material\\mtd\\Chr\\matxml\\C[c4000]_Horse_Body.matxml";
const CUT_OUT_SAMPLERS: [&str; 3] = ["C_AMSN_PixelKill__snp_Texture2D_2_AlbedoMap_0", "C_AMSN_PixelKill__snp_Texture2D_0_MetallicMap_0", "C_AMSN_PixelKill__snp_Texture2D_7_NormalMap_4"];

/// Cloth simulated in Ragnarök, with how much of the legs' pull it takes: its own weights go to physics joints, which
/// land on torso bones here, so it would hang still while the arms and legs move through it. It takes the arms' share
/// of the weights of the body underneath (see [`transfer_cloth_weights`]): the cloak (75) and the shoulder fur (29)
/// none of the legs' (a cloak that followed each leg would tear down the middle), the skirt (60) a little over half.
pub const CLOTH: [(u16, f32); 3] = [(75, 0.0), (29, 0.0), (60, 0.6)];
/// The body surfaces cloth takes its weights from: the torso and arm skin (72, 74), the trousers (88) and boots (62).
pub const BODY: [u16; 4] = [72, 74, 88, 62];

/// Kratos's eyes, the landmark [`fit_proportions`] puts on the character's.
pub const EYES: u16 = 77;

/// The largest side of a colour map in the part (Ragnarök's are up to 2048).
pub const MAX_TEXTURE: u32 = 1024;
/// The largest side of a surface (normal and roughness) map.
pub const MAX_SURFACE: u32 = 512;

/// The Kratos materials the body part draws: every material of the default outfit's most detailed level, less the
/// see-through ones.
pub fn outfit_materials(mesh: &[u8], mg: &[u8]) -> Vec<u16> {
    let rs = mesh_records(mesh);
    let parts = mg_parts(mg);
    let mut m: Vec<u16> = DEFAULT_OUTFIT
        .iter()
        .filter_map(|p| parts.get(*p as usize))
        .filter_map(|l| l.first())
        .flat_map(|l| l.first as usize..l.first as usize + l.draws as usize)
        .filter_map(|k| rs.get(k))
        .map(|r| r.material)
        .filter(|m| !SEE_THROUGH.contains(m))
        .collect();
    m.sort_unstable();
    m.dedup();
    m
}

/// The root bone alone, at the origin: the skeleton a part has when the game's is not known yet.
fn master_only() -> Vec<Node> {
    vec![Node { name: "Master".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 8 }]
}

/// The player skeleton gowring.dll wrote (`{"skeleton": .., "bones": [{"name", "parent", "t", "r", "s"}]}`, Havok's
/// rest pose: translation, rotation quaternion x y z w, scale, each relative to the parent) as model nodes.
pub fn skeleton_nodes(json: &str) -> Result<Vec<Node>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let bones = v["bones"].as_array().context("no bones")?;
    let floats = |b: &serde_json::Value, k: &str, n: usize| -> Result<Vec<f32>> {
        let a = b[k].as_array().with_context(|| format!("bone without {k}"))?;
        let out: Vec<f32> = a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
        if out.len() != n || out.iter().any(|x| !x.is_finite()) {
            bail!("bone {k} is not {n} numbers");
        }
        Ok(out)
    };
    let mut nodes = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let name = b["name"].as_str().context("bone without a name")?.to_string();
        let parent = b["parent"].as_i64().context("bone without a parent")?;
        if parent < -1 || parent >= i as i64 {
            bail!("bone {name} has parent {parent}");
        }
        let (t, r, sc) = (floats(b, "t", 3)?, floats(b, "r", 4)?, floats(b, "s", 3)?);
        nodes.push(Node { name, parent: parent as i16, t: [t[0], t[1], t[2]], r: erpart::euler_xzy([r[0], r[1], r[2], r[3]]), s: [sc[0], sc[1], sc[2]], flags: 8 });
    }
    if nodes.first().map(|n| n.name.as_str()) != Some("Master") || nodes.len() > 1000 {
        bail!("not a character skeleton (first bone {:?}, {} bones)", nodes.first().map(|n| &n.name), nodes.len());
    }
    Ok(nodes)
}

/// One part's model as meshes: Kratos's draws from `records`, skinned to `skeleton` when `skin` re-posed him onto
/// it, else rigid on its root bone; shifted by `offset` (metres).
/// Each mesh's `material` is the position of its draw's Kratos material in `materials`.
fn kratos_meshes(rs: &[crate::kratos::MeshRecord], records: &[usize], gpu: &[u8], toc: &crate::lodpack::Toc, file: &mut std::fs::File, offset: [f32; 3], skeleton: &[Node], skin: Option<&crate::retarget::Retarget>, materials: &[u16]) -> Result<(Vec<Node>, Vec<Mesh>, Vec<usize>)> {
    let mut cache: std::collections::HashMap<u64, Vec<u8>> = Default::default();
    let mut nodes = skeleton.to_vec();
    let master = nodes.iter().position(|n| n.name == "Master").context("no Master bone")? as u8;
    let mut meshes = Vec::new();
    let mut failed = Vec::new();
    for &k in records {
        let Some(r) = rs.get(k) else { continue };
        let Some(material) = materials.iter().position(|m| *m == r.material) else { continue };
        let res: &[u8] = if r.hash == 0 {
            gpu
        } else {
            if !cache.contains_key(&r.hash) {
                match toc.read(file, r.hash)? {
                    Some(b) => {
                        cache.insert(r.hash, b);
                    }
                    None => {
                        failed.push(k);
                        continue;
                    }
                }
            }
            &cache[&r.hash]
        };
        let Some(v) = draw_vertices(res, r) else {
            failed.push(k);
            continue;
        };
        let node = nodes.len() as u32;
        nodes.push(Node { name: format!("kratos_{k}"), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 });
        // a rigid draw (no skin weights) follows the moving joint nearest its middle
        let rigid: Vec<(u16, f32)> = match skin {
            Some(r) if v.influences.iter().all(|i| i.is_empty()) && !v.position.is_empty() => {
                let k = v.position.len() as f32;
                let c = v.position.iter().fold([0.0f32; 3], |s, p| [s[0] + p[0] * SCALE / k, s[1] + p[1] * SCALE / k, s[2] + p[2] * SCALE / k]);
                let d = |j: usize| -> f32 { (0..3).map(|a| (r.bind_positions[j][a] - c[a]).powi(2)).sum() };
                (0..r.deforms.len()).filter(|j| r.deforms[*j]).min_by(|a, b| d(*a).total_cmp(&d(*b))).map(|j| vec![(j as u16, 1.0)]).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let vertices: Vec<Vertex> = (0..v.position.len())
            .map(|i| {
                let p = v.position[i];
                let p = [p[0] * SCALE, p[1] * SCALE, p[2] * SCALE];
                let n = v.normal.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                let (p, n, bones, weights) = match skin {
                    Some(r) => {
                        let inf = v.influences.get(i).filter(|x| !x.is_empty()).unwrap_or(&rigid);
                        crate::retarget::skin(r, p, n, inf)
                    }
                    None => (p, n, [master; 4], [1.0, 0.0, 0.0, 0.0]),
                };
                // any unit vector across the normal serves as a tangent until his normal maps come in
                let up = if n[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
                let t = [up[1] * n[2] - up[2] * n[1], up[2] * n[0] - up[0] * n[2], up[0] * n[1] - up[1] * n[0]];
                let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-6);
                Vertex {
                    position: [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]],
                    normal: n,
                    tangent: [t[0] / len, t[1] / len, t[2] / len, 1.0],
                    bones,
                    weights,
                    color: [255; 4],
                    uv: [v.uv[0].get(i).copied().unwrap_or([0.0; 2]), v.uv[1].get(i).copied().unwrap_or([0.0; 2])],
                }
            })
            .collect();
        let mut mesh = Mesh { material: material as u32, node, vertices, indices: v.triangles.iter().flat_map(|t| t.map(|x| x as u32)).collect() };
        erpart::compute_tangents(&mut mesh);
        meshes.push(mesh);
    }
    let (nodes, meshes) = merge_by_material(nodes, meshes, skeleton.len());
    Ok((nodes, meshes, failed))
}

/// Give each vertex of the cloth meshes (Kratos material in `CLOTH`; `used` maps a mesh's material index to Kratos's)
/// the weights of the body under it. The body's weights there are the inverse-distance blend of the nearest body
/// vertices within 15 cm (a 5 cm grid): at most six, and only those within 2 cm of the nearest's distance, so cloth
/// near an armpit takes the skin right under it, not the arm across the gap. Their arm-bone part (clavicle to fingers)
/// replaces that much of the cloth's own weights, their leg-bone part (thigh to toes) times the material's leg factor,
/// and their other bones (spine, neck) by how close the cloth is: wholly within 4 cm, fading to none at 12 cm. So cloth
/// lying on the body moves exactly as the skin under it (no skin pushing through as an arm lowers), cloth over an arm
/// follows the arm, and loose cloth stays with the torso. Bone names come from `skeleton`; top four bones. Cloth
/// farther than 15 cm from the body keeps its own. Returns how many vertices changed.
pub fn transfer_cloth_weights(meshes: &mut [Mesh], used: &[u16], skeleton: &[Node]) -> usize {
    let limb = |b: u8| -> u8 {
        let n = skeleton.get(b as usize).map(|n| n.name.as_str()).unwrap_or("");
        if ["Clavicle", "UpperArm", "UpArmTwist", "Forearm", "ForeArmTwist", "Hand", "Finger"].iter().any(|k| n.contains(k)) {
            1
        } else if ["Thigh", "Calf", "Foot", "Toe"].iter().any(|k| n.contains(k)) {
            2
        } else {
            0
        }
    };
    const CELL: f32 = 0.05;
    let kratos = |m: &Mesh| used.get(m.material as usize).copied().unwrap_or(u16::MAX);
    let cell = |p: [f32; 3]| -> (i32, i32, i32) { ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32) };
    let mut body: Vec<([f32; 3], [u8; 4], [f32; 4])> = Vec::new();
    for m in meshes.iter().filter(|m| BODY.contains(&kratos(m))) {
        body.extend(m.vertices.iter().map(|v| (v.position, v.bones, v.weights)));
    }
    if body.is_empty() {
        return 0;
    }
    let mut grid: std::collections::HashMap<(i32, i32, i32), Vec<usize>> = Default::default();
    for (i, b) in body.iter().enumerate() {
        grid.entry(cell(b.0)).or_default().push(i);
    }
    let mut changed = 0;
    for m in meshes.iter_mut() {
        let Some(&(_, leg_factor)) = CLOTH.iter().find(|c| c.0 == kratos(m)) else { continue };
        for v in m.vertices.iter_mut() {
            let c = cell(v.position);
            let mut near: Vec<(f32, usize)> = Vec::new();
            for reach in 1..=3 {
                near.clear();
                for dx in -reach..=reach {
                    for dy in -reach..=reach {
                        for dz in -reach..=reach {
                            if let Some(list) = grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                                for &i in list {
                                    let d = (0..3).map(|k| (body[i].0[k] - v.position[k]).powi(2)).sum::<f32>().sqrt();
                                    if d <= CELL * reach as f32 {
                                        near.push((d, i));
                                    }
                                }
                            }
                        }
                    }
                }
                if near.len() >= 6 {
                    break;
                }
            }
            if near.is_empty() {
                continue;
            }
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            let nearest = near[0].0;
            near.retain(|n| n.0 <= nearest + 0.02);
            near.truncate(6);
            let close = ((0.12 - nearest) / (0.12 - 0.04)).clamp(0.0, 1.0);
            // the body's weights here, normalised
            let mut under: Vec<(u8, f32)> = Vec::new();
            let mut total = 0.0;
            for (d, i) in &near {
                let w = 1.0 / (d + 0.01);
                for k in 0..4 {
                    let (b, bw) = (body[*i].1[k], body[*i].2[k]);
                    if bw <= 0.0 {
                        continue;
                    }
                    total += w * bw;
                    match under.iter_mut().find(|e| e.0 == b) {
                        Some(e) => e.1 += w * bw,
                        None => under.push((b, w * bw)),
                    }
                }
            }
            if total <= 0.0 {
                continue;
            }
            // the limbs' share of it, and the rest as near as the cloth lies, replaces that much of the cloth's own
            let pull: Vec<(u8, f32)> = under.iter().map(|&(b, w)| (b, w / total * [close, 1.0, leg_factor][limb(b) as usize])).filter(|e| e.1 > 0.0).collect();
            let share: f32 = pull.iter().map(|e| e.1).sum::<f32>().min(1.0);
            if share < 0.02 {
                continue;
            }
            let mut per_bone: Vec<(u8, f32)> = (0..4).filter(|k| v.weights[*k] > 0.0).map(|k| (v.bones[k], v.weights[k] * (1.0 - share))).collect();
            for (b, w) in pull {
                match per_bone.iter_mut().find(|e| e.0 == b) {
                    Some(e) => e.1 += w,
                    None => per_bone.push((b, w)),
                }
            }
            per_bone.sort_by(|a, b| b.1.total_cmp(&a.1));
            per_bone.truncate(4);
            let sum: f32 = per_bone.iter().map(|e| e.1).sum();
            if sum <= 0.0 {
                continue;
            }
            v.bones = [0; 4];
            v.weights = [0.0; 4];
            for (k, (b, w)) in per_bone.iter().enumerate() {
                v.bones[k] = *b;
                v.weights[k] = w / sum;
            }
            changed += 1;
        }
    }
    changed
}

/// Fit Kratos's proportions to the character's skeleton. Elden Ring draws the character's face under every helmet
/// (the face scale on helmets is 1.0 everywhere and changes nothing), and Kratos's torso and neck are longer: with his
/// hips on the character's, his shoulders sat 5 cm above its shoulder joints (so his arms turned about points below his
/// shoulders) and his eyes 6.4 cm above its eyes, whose ears and hair showed below his. Each bone gets a step: none for
/// the legs and hips, `shoulder_step` (height) for the arms (clavicle to fingers), his eyes' step onto the mean of the
/// eye bones (height and depth; Kratos's eyes are material [`EYES`]) for the head and the bones under it, and for the
/// rest by the bone's rest height: none at the hips, growing to the shoulders' step at the shoulders, then to the
/// head's at the head. Every vertex moves by its bones' steps blended by its weights, so the torso shortens evenly, the
/// neck barely, and cloth moves with the body under it. Returns the head's step and how many vertices moved.
pub fn fit_proportions(meshes: &mut [Mesh], used: &[u16], skeleton: &[Node], shoulder_step: f32) -> Option<([f32; 3], usize)> {
    let world = erpart::world_matrices(skeleton);
    let at = |name: &str| skeleton.iter().position(|n| n.name == name).map(|i| [world[i][3][0], world[i][3][1], world[i][3][2]]);
    let head = skeleton.iter().position(|n| n.name == "Head")?;
    let (hip_y, shoulder_y, head_y) = (at("L_Thigh")?[1], at("L_UpperArm")?[1], world[head][3][1]);
    let mean = |ps: &[[f32; 3]]| -> Option<[f32; 3]> { (!ps.is_empty()).then(|| std::array::from_fn(|k| ps.iter().map(|p| p[k]).sum::<f32>() / ps.len() as f32)) };
    let eye_bones: Vec<[f32; 3]> = ["L_eyeA", "L_eyeB", "R_eyeA", "R_eyeB"].iter().filter_map(|n| at(n)).collect();
    let his_eyes: Vec<[f32; 3]> = meshes.iter().filter(|m| used.get(m.material as usize) == Some(&EYES)).flat_map(|m| m.vertices.iter().map(|v| v.position)).collect();
    let (to, from) = (mean(&eye_bones)?, mean(&his_eyes)?);
    let head_step = [0.0, to[1] - from[1], to[2] - from[2]];
    let under_head = |mut i: usize| loop {
        if i == head {
            break true;
        }
        match skeleton[i].parent {
            p if p >= 0 => i = p as usize,
            _ => break false,
        }
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t.clamp(0.0, 1.0);
    let steps: Vec<[f32; 3]> = skeleton
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let name = n.name.as_str();
            let y = world[i][3][1];
            if under_head(i) {
                head_step
            } else if ["Clavicle", "UpperArm", "UpArm", "Forearm", "ForeArm", "Hand", "Finger", "Elbow", "Weapon", "Shield"].iter().any(|k| name.contains(k)) {
                [0.0, shoulder_step, 0.0]
            } else if ["Thigh", "Calf", "Foot", "Toe", "Knee", "Hip"].iter().any(|k| name.contains(k)) || y <= hip_y {
                [0.0; 3]
            } else if y <= shoulder_y {
                [0.0, shoulder_step * (y - hip_y) / (shoulder_y - hip_y), 0.0]
            } else {
                let t = (y - shoulder_y) / (head_y - shoulder_y).max(1e-3);
                [0.0, lerp(shoulder_step, head_step[1], t), lerp(0.0, head_step[2], t)]
            }
        })
        .collect();
    let mut moved = 0;
    for v in meshes.iter_mut().flat_map(|m| m.vertices.iter_mut()) {
        let step: [f32; 3] = std::array::from_fn(|k| (0..4).map(|j| v.weights[j] * steps.get(v.bones[j] as usize).map_or(0.0, |s| s[k])).sum());
        if step.iter().any(|d| d.abs() > 1e-5) {
            for k in 0..3 {
                v.position[k] += step[k];
            }
            moved += 1;
        }
    }
    Some((head_step, moved))
}

/// One mesh per material (split only where a mesh would pass 65,535 vertices), each with its own mesh node after the
/// skeleton's `bones` nodes: Kratos's 62 draws become about two dozen meshes, nearer the game's own armour.
fn merge_by_material(nodes: Vec<Node>, meshes: Vec<Mesh>, bones: usize) -> (Vec<Node>, Vec<Mesh>) {
    let mut nodes: Vec<Node> = nodes.into_iter().take(bones).collect();
    let mut merged: Vec<Mesh> = Vec::new();
    let mut order: Vec<u32> = meshes.iter().map(|m| m.material).collect();
    order.dedup();
    let mut seen = std::collections::BTreeSet::new();
    order.retain(|m| seen.insert(*m));
    for mat in order {
        let mut cur: Option<Mesh> = None;
        for m in meshes.iter().filter(|m| m.material == mat) {
            if cur.as_ref().is_some_and(|c| c.vertices.len() + m.vertices.len() > 65535) {
                merged.push(cur.take().unwrap());
            }
            let c = cur.get_or_insert_with(|| Mesh { material: mat, node: 0, vertices: Vec::new(), indices: Vec::new() });
            let base = c.vertices.len() as u32;
            c.vertices.extend_from_slice(&m.vertices);
            c.indices.extend(m.indices.iter().map(|i| i + base));
        }
        merged.extend(cur);
    }
    for (k, m) in merged.iter_mut().enumerate() {
        m.node = nodes.len() as u32;
        nodes.push(Node { name: format!("kratos_mesh_{k}"), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 });
    }
    (nodes, merged)
}

/// An empty piece: one triangle with no area (so nothing draws) on the root bone. The head, arms and legs slots wear
/// it: Elden Ring hides the character's own head, arms and legs under armour, and Kratos's whole figure is the body.
fn empty_piece() -> (Vec<Node>, Vec<Mesh>) {
    let mut nodes = master_only();
    nodes.push(Node { name: "empty".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 });
    let v = Vertex { position: [0.0, 1.0, 0.0], normal: [0.0, 0.0, -1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.0; 2]; 2] };
    (nodes, vec![Mesh { material: 0, node: 1, vertices: vec![v; 3], indices: vec![0, 1, 2] }])
}

/// A box `size` metres wide centred at `at`, on the root bone: the smallest model there is.
#[allow(dead_code)]
fn cube(at: [f32; 3], size: f32) -> (Vec<Node>, Vec<Mesh>) {
    let mut nodes = master_only();
    nodes.push(Node { name: "marker".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 });
    let h = size / 2.0;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    // six faces, each its own four corners so the normals are flat
    for (axis, sign) in [(0usize, 1.0f32), (0, -1.0), (1, 1.0), (1, -1.0), (2, 1.0), (2, -1.0)] {
        let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
        let base = vertices.len() as u32;
        for (a, b) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let mut p = [0.0f32; 3];
            p[axis] = sign * h;
            p[u] = a * h;
            p[w] = b * h;
            let mut n = [0.0f32; 3];
            n[axis] = sign;
            let mut t = [0.0f32; 4];
            t[u] = 1.0;
            t[3] = 1.0;
            vertices.push(Vertex { position: [p[0] + at[0], p[1] + at[1], p[2] + at[2]], normal: n, tangent: t, bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[(a + 1.0) / 2.0, (b + 1.0) / 2.0], [0.0; 2]] });
        }
        let quad = if sign > 0.0 { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
        indices.extend(quad.iter().map(|q| base + q));
    }
    (nodes, vec![Mesh { material: 0, node: 1, vertices, indices }])
}

fn tex_path(stem: &str) -> String {
    format!("N:\\GR\\data\\INTERROOT_win64\\parts\\kratos\\tex\\{stem}.tif")
}

/// The part's material for colour map `albedo` and normal map `normal` (texture stems in its TPF), with the shared flat
/// metal map.
fn material_with(id: u16, name: &str, albedo: &str, normal: &str) -> Material {
    Material { name: name.into(), mtd: erpart::MATERIAL_MTD.into(), textures: vec![(erpart::ALBEDO.into(), tex_path(albedo)), (erpart::METALLIC.into(), tex_path(&format!("kratos_{id}_m"))), (erpart::NORMAL.into(), tex_path(normal))] }
}

fn material(id: u16) -> Material {
    material_with(id, "kratos", &format!("kratos_{id}_a"), &format!("kratos_{id}_n"))
}

fn texture_pack(id: u16, albedo: [u8; 3]) -> Vec<u8> {
    let solid = |c: [u8; 3]| erpart::dds_bc1(4, 4, &erpart::bc1_solid(c[0], c[1], c[2]));
    erpart::tpf(&[(format!("kratos_{id}_a"), solid(albedo), 0), (format!("kratos_{id}_m"), solid([0, 160, 0]), 0), (format!("kratos_{id}_n"), solid([128, 128, 255]), 0)])
}

/// Every part file this version installs, as (file name, bytes), and a line about them. The body (`bd_*_9701`) is
/// Kratos's whole default outfit, re-posed and skinned onto the player `skeleton` when both it and his own (`kratos`,
/// from his `goProto`) are known; the head, arms and legs (`hd_*_9700`, `am_*_9702`, `lg_*_9703`) are empty pieces
/// that hide the character's own. Each comes as the normal and the low-detail (`_l`) file, male and female.
/// `looks` holds a colour map and surface per Kratos material (a missing colour is a neutral grey, a missing surface
/// flat and rough).
pub fn build_parts(mesh: &[u8], mg: &[u8], gpu: &[u8], pack: &Path, skeleton: Option<&[Node]>, kratos: Option<&crate::kratos::Skeleton>, looks: &std::collections::BTreeMap<u16, crate::kratos_look::Look>) -> Result<(Vec<(String, Vec<u8>)>, String)> {
    let rs = mesh_records(mesh);
    let parts = mg_parts(mg);
    let level0 = |list: &[u16]| -> Vec<usize> { list.iter().filter_map(|p| parts.get(*p as usize)).filter_map(|l| l.first()).flat_map(|l| l.first as usize..l.first as usize + l.draws as usize).collect() };
    let outfit = level0(DEFAULT_OUTFIT);
    if outfit.is_empty() {
        bail!("no default outfit draws in the mesh tables");
    }
    let toc = crate::lodpack::Toc::read_from_pack(pack)?;
    let mut file = std::fs::File::open(pack)?;
    let mut out = Vec::new();
    let mut note = String::new();
    let mut add = |slot: &str, id: u16, nodes: &[Node], meshes: &[Mesh], mats: &[Material], tpf: Vec<u8>| -> usize {
        let flver = erpart::flver2(nodes, mats, meshes);
        let size = flver.len();
        for sex in ["m", "f"] {
            for low in ["", "_l"] {
                let stem = format!("{slot}_{sex}_{id:04}{low}");
                out.push((format!("{stem}.partsbnd.dcx"), erpart::partsbnd(&stem, flver.clone(), tpf.clone())));
            }
        }
        size
    };
    let fallback = master_only();
    let skeleton = skeleton.unwrap_or(&fallback);
    note.push_str(&format!("{} bones; ", skeleton.len()));
    let retargeted = match kratos {
        Some(k) if skeleton.len() > 1 => match crate::retarget::retarget(k, SCALE, skeleton) {
            Ok(r) => {
                note.push_str(&format!("re-posed and skinned: {}", r.note));
                Some(r)
            }
            Err(e) => {
                note.push_str(&format!("rigid (re-posing failed: {e}); "));
                None
            }
        },
        _ => {
            note.push_str("rigid (no skeleton); ");
            None
        }
    };
    // his materials (the see-through ones left out, and the strands too when they have no coverage), each with its
    // colour map or a neutral grey
    let used: Vec<u16> = outfit_materials(mesh, mg).into_iter().filter(|m| !CUT_OUT.contains(m) || looks.get(m).is_some_and(|l| l.cut_out)).collect();
    let (nodes, mut meshes, failed) = kratos_meshes(&rs, &outfit, gpu, &toc, &mut file, [0.0; 3], skeleton, retargeted.as_ref(), &used)?;
    if retargeted.is_some() {
        let shoulder_step = retargeted.as_ref().map_or(0.0, |r| r.shoulder_step);
        match fit_proportions(&mut meshes, &used, skeleton, shoulder_step) {
            Some((step, n)) => note.push_str(&format!("fitted to the character: shoulders {shoulder_step:.3} m, head ({:.3}, {:.3}, {:.3}) m ({n} vertices); ", step[0], step[1], step[2])),
            None => note.push_str("proportions left as they are (no eye bones or no eyes); "),
        }
        let n = transfer_cloth_weights(&mut meshes, &used, skeleton);
        note.push_str(&format!("{n} cloth vertices follow the body under them; "));
    }
    let tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    let mut textures: Vec<(String, Vec<u8>, u8)> = Vec::new();
    let mut mats = Vec::new();
    let (mut coloured, mut surfaced, mut strands) = (0, 0, 0);
    for m in &used {
        let look = looks.get(m);
        let cut = look.is_some_and(|l| l.cut_out);
        let stem = format!("kratos_{MODEL_ID}_m{m}_a");
        let dds = match look.and_then(|l| l.colour.as_ref()) {
            Some(img) if cut => {
                strands += 1;
                erpart::dds_bc1_cut_mips(img.w, img.h, &img.rgba)
            }
            Some(img) => {
                coloured += 1;
                erpart::dds_bc1_mips(img.w, img.h, &img.rgba, true)
            }
            None => erpart::dds_bc1(4, 4, &erpart::bc1_solid(150, 140, 130)),
        };
        textures.push((stem.clone(), dds, 0));
        let normal = match look.and_then(|l| l.surface.as_ref()) {
            Some(img) => {
                surfaced += 1;
                let n = format!("kratos_{MODEL_ID}_m{m}_n");
                textures.push((n.clone(), erpart::dds_bc1_mips(img.w, img.h, &img.rgba, false), 0));
                n
            }
            None => format!("kratos_{MODEL_ID}_n"),
        };
        let mut mat = material_with(MODEL_ID, &format!("kratos_m{m}"), &stem, &normal);
        if cut {
            mat.mtd = CUT_OUT_MTD.into();
            for (t, name) in mat.textures.iter_mut().zip(CUT_OUT_SAMPLERS) {
                t.0 = name.into();
            }
        }
        mats.push(mat);
    }
    let solid = |c: [u8; 3]| erpart::dds_bc1(4, 4, &erpart::bc1_solid(c[0], c[1], c[2]));
    textures.push((format!("kratos_{MODEL_ID}_m"), solid([0, 160, 0]), 0));
    textures.push((format!("kratos_{MODEL_ID}_n"), solid([128, 128, 255]), 0));
    let size = add("bd", MODEL_ID, &nodes, &meshes, &mats, erpart::tpf(&textures));
    note.push_str(&format!("body {} meshes ({tris} triangles, {size} bytes; {} failed {failed:?}), {} materials ({coloured} with their colour map, {strands} of strands cut out, {surfaced} with their surface); ", meshes.len(), failed.len(), used.len()));
    let (nodes, meshes) = empty_piece();
    for (slot, id) in [("hd", MODEL_ID - 1), ("am", MODEL_ID + 1), ("lg", MODEL_ID + 2)] {
        add(slot, id, &nodes, &meshes, &[material(id)], texture_pack(id, [128, 128, 128]));
    }
    note.push_str("head, arms and legs: empty pieces");
    Ok((out, note))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloth_takes_the_weights_of_the_body_under_it() {
        let v = |x: f32, bone: u8| Vertex { position: [x, 1.0, 0.0], normal: [0.0, 0.0, 1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [bone, 0, 0, 0], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.0; 2]; 2] };
        // an arm (bone 56) from x = 0.3 to 0.5 and a torso (bone 50) from x = -0.1 to 0.1
        let arm = Mesh { material: 1, node: 0, vertices: (0..10).map(|i| v(0.3 + 0.02 * i as f32, 56)).collect(), indices: vec![0, 1, 2] };
        let torso = Mesh { material: 0, node: 0, vertices: (0..10).map(|i| v(-0.1 + 0.02 * i as f32, 50)).collect(), indices: vec![0, 1, 2] };
        // a cloak on bone 48: one vertex over the arm, one on the torso, one 8 cm off it, one far away
        let mut cloak = Mesh { material: 2, node: 0, vertices: vec![v(0.42, 48), v(0.0, 48), v(0.0, 48), v(3.0, 48)], indices: vec![0, 1, 2, 1, 2, 3] };
        cloak.vertices[0].position[2] = 0.03;
        cloak.vertices[2].position[2] = 0.08;
        let mut meshes = vec![torso, arm, cloak];
        let mut skeleton: Vec<Node> = (0..60).map(|i| Node { name: format!("bone{i}"), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 8 }).collect();
        skeleton[56].name = "L_UpperArm".into();
        skeleton[50].name = "Spine2".into();
        // the meshes' materials stand for Kratos's torso skin, arm skin and cloak
        let n = transfer_cloth_weights(&mut meshes, &[72, 74, 75], &skeleton);
        // over the arm it follows the arm; on the torso it moves as the torso's skin; 8 cm off it, half of each (it
        // fades from 4 to 12 cm); far away it keeps its own
        assert_eq!(n, 3);
        let c = &meshes[2].vertices;
        assert_eq!((c[0].bones[0], c[1].bones[0], c[3].bones[0]), (56, 50, 48));
        assert_eq!((c[1].weights[0], c[3].weights[0]), (1.0, 1.0));
        let half: Vec<(u8, f32)> = (0..2).map(|k| (c[2].bones[k], c[2].weights[k])).collect();
        assert!(half.iter().all(|(b, w)| [48, 50].contains(b) && (w - 0.5).abs() < 0.01), "{half:?}");
        assert!((c[0].weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn kratos_fits_the_characters_proportions_bone_by_bone() {
        let node = |name: &str, parent: i16, t: [f32; 3]| Node { name: name.into(), parent, t, r: [0.0; 3], s: [1.0; 3], flags: 8 };
        // hips 0.97, Spine2 1.24, shoulders 1.391, Neck 1.453, Head 1.557, eyes 1.617 and 7.6 cm forward
        let skeleton = vec![
            node("Pelvis", -1, [0.0, 0.97, 0.0]),
            node("L_Thigh", 0, [0.1, 0.0, 0.0]),
            node("Spine2", 0, [0.0, 0.27, 0.0]),
            node("L_UpperArm", 2, [0.17, 0.151, 0.0]),
            node("Neck", 2, [0.0, 0.213, 0.025]),
            node("Head", 4, [0.0, 0.104, -0.018]),
            node("L_eyeA", 5, [-0.025, 0.06, -0.083]),
            node("R_eyeA", 5, [0.025, 0.06, -0.083]),
        ];
        let v = |y: f32, bone: u8| Vertex { position: [0.0, y, -0.068], normal: [0.0, 0.0, 1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [bone, 0, 0, 0], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.0; 2]; 2] };
        // his eyes 6.4 cm higher and 8 mm further back than the eye bones; a thigh, chest, arm and neck vertex
        let eyes = Mesh { material: 0, node: 0, vertices: vec![v(1.681, 5)], indices: vec![0, 0, 0] };
        let body = Mesh { material: 1, node: 0, vertices: vec![v(0.8, 1), v(1.3, 2), v(1.4, 3), v(1.5, 4)], indices: vec![0, 1, 2] };
        let mut meshes = vec![eyes, body];
        let (step, moved) = fit_proportions(&mut meshes, &[EYES, 72], &skeleton, -0.05).unwrap();
        assert!(step[0] == 0.0 && (step[1] + 0.064).abs() < 1e-4 && (step[2] + 0.008).abs() < 1e-4, "{step:?}");
        assert_eq!(moved, 4);
        let p: Vec<[f32; 3]> = meshes.iter().flat_map(|m| m.vertices.iter().map(|v| v.position)).collect();
        // eyes onto the eye bones; the thigh stays; the chest by its share of the way to the shoulders; the arm by the
        // shoulders' step; the neck between the shoulders' and the head's
        assert!((p[0][1] - 1.617).abs() < 1e-4 && (p[0][2] + 0.076).abs() < 1e-4, "{:?}", p[0]);
        assert_eq!(p[1][1], 0.8);
        assert!((p[2][1] - (1.3 - 0.05 * 0.27 / 0.421)).abs() < 1e-4, "{:?}", p[2]);
        assert!((p[3][1] - 1.35).abs() < 1e-4);
        let t = 0.062 / 0.166;
        assert!((p[4][1] - (1.5 - 0.05 - 0.014 * t)).abs() < 1e-4 && (p[4][2] - (-0.068 - 0.008 * t)).abs() < 1e-4, "{:?}", p[4]);
        // no eye bones: nothing moves
        assert!(fit_proportions(&mut meshes, &[EYES, 72], &skeleton[..6], -0.05).is_none());
    }

    #[test]
    fn draws_merge_into_one_mesh_per_material_with_their_own_nodes() {
        let v = |x: f32| Vertex { position: [x, 0.0, 0.0], normal: [0.0, 0.0, 1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.0; 2]; 2] };
        let mesh = |mat: u32, node: u32, n: usize| Mesh { material: mat, node, vertices: (0..n).map(|i| v(i as f32)).collect(), indices: vec![0, 1, 2] };
        let mut nodes = master_only();
        for k in 0..3 {
            nodes.push(Node { name: format!("kratos_{k}"), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 });
        }
        let (nodes, meshes) = merge_by_material(nodes, vec![mesh(1, 1, 3), mesh(0, 2, 4), mesh(1, 3, 5)], 1);
        assert_eq!(meshes.len(), 2);
        assert_eq!((meshes[0].material, meshes[0].vertices.len()), (1, 8));
        // the second draw's indices follow the first's vertices
        assert_eq!(meshes[0].indices, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(nodes.len(), 3);
        assert_eq!((meshes[0].node, meshes[1].node), (1, 2));
        assert!(nodes[1..].iter().all(|n| n.flags == 4));
    }

    #[test]
    fn a_skeleton_file_becomes_bones_at_their_rest_pose() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let json = format!(
            r#"{{"skeleton": "c0000", "bones": [
                {{"name": "Master", "parent": -1, "t": [0, 0, 0], "r": [0, {h}, 0, {h}], "s": [1, 1, 1]}},
                {{"name": "RootPos", "parent": 0, "t": [0, 0.97, 0], "r": [0, -{h}, 0, {h}], "s": [1, 1, 1]}},
                {{"name": "L_Thigh", "parent": 1, "t": [0.103, 0, 0], "r": [0, 0, 0, 1], "s": [1, 1, 1]}}
            ]}}"#
        );
        let nodes = skeleton_nodes(&json).unwrap();
        assert_eq!(nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Master", "RootPos", "L_Thigh"]);
        assert!((nodes[0].r[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-5, "{:?}", nodes[0].r);
        let w = erpart::world_matrices(&nodes);
        // the quarter turns cancel: the thigh sits 0.103 m along x at hip height
        let p = w[2][3];
        assert!((p[0] - 0.103).abs() < 1e-5 && (p[1] - 0.97).abs() < 1e-5 && p[2].abs() < 1e-5, "{p:?}");
        assert!(skeleton_nodes(r#"{"bones": [{"name": "Pelvis", "parent": -1, "t": [0,0,0], "r": [0,0,0,1], "s": [1,1,1]}]}"#).is_err());
        assert!(skeleton_nodes(r#"{"bones": [{"name": "Master", "parent": 0, "t": [0,0,0], "r": [0,0,0,1], "s": [1,1,1]}]}"#).is_err());
    }

    /// The skeleton a test session wrote (`GOWRING_ER_SKELETON=.../er_skeleton.json cargo test -p gowring-gow
    /// real_er_skeleton -- --ignored --nocapture`): every bone's model-space position from our Euler angles matches the
    /// quaternions composed directly.
    #[test]
    #[ignore]
    fn real_er_skeleton() {
        let Ok(path) = std::env::var("GOWRING_ER_SKELETON") else { return };
        let json = std::fs::read_to_string(path).unwrap();
        let nodes = skeleton_nodes(&json).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut direct: Vec<([f64; 9], [f64; 3])> = Vec::new();
        for b in v["bones"].as_array().unwrap() {
            let f = |k: &str| b[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
            let (t, q, sc) = (f("t"), f("r"), f("s"));
            let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
            let r = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), 2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), 2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)];
            let m: [f64; 9] = std::array::from_fn(|i| r[i] * sc[i % 3]);
            let parent = b["parent"].as_i64().unwrap();
            if parent < 0 {
                direct.push((m, [t[0], t[1], t[2]]));
            } else {
                let (pm, pt) = direct[parent as usize];
                let rm: [f64; 9] = std::array::from_fn(|i| (0..3).map(|k| pm[3 * (i / 3) + k] * m[3 * k + i % 3]).sum());
                let rt: [f64; 3] = std::array::from_fn(|i| (0..3).map(|k| pm[3 * i + k] * t[k]).sum::<f64>() + pt[i]);
                direct.push((rm, rt));
            }
        }
        let ours = erpart::world_matrices(&nodes);
        let mut worst = 0.0f64;
        for (i, (_, t)) in direct.iter().enumerate() {
            let p = ours[i][3];
            worst = worst.max((0..3).map(|k| (p[k] as f64 - t[k]).abs()).fold(0.0, f64::max));
        }
        println!("{} bones; worst position difference {worst:.2e} m", nodes.len());
        assert!(worst < 1e-4);
    }

    /// How much of the default outfit each material covers: `GOWRING_MESH=... GOWRING_MG=... cargo test -p gowring-gow
    /// outfit_materials -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn outfit_materials() {
        let (Ok(mesh), Ok(mg)) = (std::env::var("GOWRING_MESH"), std::env::var("GOWRING_MG")) else { return };
        let (mesh, mg) = (std::fs::read(mesh).unwrap(), std::fs::read(mg).unwrap());
        let rs = mesh_records(&mesh);
        let parts = mg_parts(&mg);
        let mut by: std::collections::BTreeMap<u16, (usize, usize, usize, Vec<usize>)> = Default::default();
        for p in DEFAULT_OUTFIT {
            let Some(l) = parts.get(*p as usize).and_then(|l| l.first()) else { continue };
            for k in l.first as usize..l.first as usize + l.draws as usize {
                let r = &rs[k];
                let e = by.entry(r.material).or_default();
                e.0 += 1;
                e.1 += r.indices as usize / 3;
                e.2 += r.vertices as usize;
                e.3.push(*p as usize);
            }
        }
        for (m, (d, t, v, ps)) in by {
            println!("material {m:3}: {d:2} draws, {t:6} triangles, {v:6} vertices, parts {ps:?}");
        }
    }

    /// Kratos's real tables with a pack of the resources saved in `GOWRING_SAMPLES_DIR` (files named
    /// `<hash>_records_...bin`); missing resources are stand-ins with every position at the draw's centre. Writes the
    /// part files to `GOWRING_ERPART_OUT`: `GOWRING_MESH=... GOWRING_MG=... GOWRING_MG_GPU=... GOWRING_SAMPLES_DIR=...
    /// GOWRING_ERPART_OUT=... cargo test -p gowring-gow kratos_part_from -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn kratos_part_from_real_tables() {
        let (Ok(mesh), Ok(mg), Ok(gpu), Ok(samples), Ok(out)) = (std::env::var("GOWRING_MESH"), std::env::var("GOWRING_MG"), std::env::var("GOWRING_MG_GPU"), std::env::var("GOWRING_SAMPLES_DIR"), std::env::var("GOWRING_ERPART_OUT")) else { return };
        let (mesh, mg, gpu) = (std::fs::read(mesh).unwrap(), std::fs::read(mg).unwrap(), std::fs::read(gpu).unwrap());
        let rs = mesh_records(&mesh);
        let mut res: std::collections::BTreeMap<u64, Vec<u8>> = Default::default();
        for e in std::fs::read_dir(&samples).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(h) = name.split('_').next().and_then(|h| u64::from_str_radix(h, 16).ok()) {
                res.insert(h, std::fs::read(e.path()).unwrap());
            }
        }
        let real = res.len();
        for r in rs.iter().filter(|r| r.hash != 0) {
            if res.contains_key(&r.hash) && real > 0 && res[&r.hash].len() >= r.vertex_bytes as usize + 2 * r.indices as usize {
                continue;
            }
            let b = res.entry(r.hash).or_default();
            let k = r.streams.iter().position(|s| s.0 == 0).unwrap();
            let (pos, stride) = (r.streams[k].3 as usize, r.strides[k] as usize);
            let end = (r.vertex_bytes as usize + 2 * r.indices as usize).max(pos + stride * r.vertices as usize);
            if b.len() < end {
                b.resize(end, 0);
            }
            for v in 0..r.vertices as usize {
                for c in 0..3 {
                    b[pos + stride * v + 4 * c..pos + stride * v + 4 * c + 4].copy_from_slice(&r.centre[c].to_le_bytes());
                }
            }
            for i in 0..r.indices as usize {
                let at = r.vertex_bytes as usize + 2 * i;
                b[at..at + 2].copy_from_slice(&((i % r.vertices as usize) as u16).to_le_bytes());
            }
        }
        // the pack: one block per resource
        let n = res.len();
        let table = 16 + 24 * 2 * n;
        let mut pack = Vec::new();
        for v in [n as u32, n as u32, 0, 1] {
            pack.extend(v.to_le_bytes());
        }
        let mut at = table as u64;
        for (h, b) in &res {
            pack.extend(at.to_le_bytes());
            pack.extend(h.to_le_bytes());
            pack.extend((b.len() as u32).to_le_bytes());
            pack.extend(1u32.to_le_bytes());
            at += b.len() as u64;
        }
        for (k, (h, b)) in res.iter().enumerate() {
            pack.extend((k as u32).to_le_bytes());
            pack.extend(0u32.to_le_bytes());
            pack.extend(h.to_le_bytes());
            pack.extend((b.len() as u32).to_le_bytes());
            pack.extend(0u32.to_le_bytes());
        }
        for b in res.values() {
            pack.extend_from_slice(b);
        }
        let dir = std::path::Path::new(&out);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("root.lodpack"), &pack).unwrap();
        let skeleton = std::env::var("GOWRING_ER_SKELETON").ok().map(|p| skeleton_nodes(&std::fs::read_to_string(p).unwrap()).unwrap());
        let kratos = std::env::var("GOWRING_PROTO").ok().map(|p| crate::kratos::Skeleton::parse(&std::fs::read(p).unwrap()).unwrap());
        // a colour map per material when GOWRING_COLOURS names a folder of `<material>.png` files (with alpha for the
        // strands, see CUT_OUT)
        let mut colours = std::collections::BTreeMap::new();
        if let Ok(d) = std::env::var("GOWRING_COLOURS") {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(m) = name.strip_suffix(".png").and_then(|n| n.parse::<u16>().ok()) else { continue };
                let dec = png::Decoder::new(std::fs::File::open(e.path()).unwrap());
                let mut r = dec.read_info().unwrap();
                let mut buf = vec![0; r.output_buffer_size()];
                let info = r.next_frame(&mut buf).unwrap();
                let rgba: Vec<u8> = match info.color_type {
                    png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
                    png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
                    _ => continue,
                };
                // a strand material's map with alpha stands for a cut-out look
                let cut_out = CUT_OUT.contains(&m) && rgba.chunks_exact(4).any(|p| p[3] < 128);
                colours.insert(m, crate::kratos_look::Look { colour: Some(crate::dds::Image { w: info.width, h: info.height, rgba, format: crate::dds::Format::Bc1 }), surface: None, cut_out });
            }
        }
        let (files, note) = build_parts(&mesh, &mg, &gpu, &dir.join("root.lodpack"), skeleton.as_deref(), kratos.as_ref(), &colours).unwrap();
        println!("{real} real resources; {note}");
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let _ = std::fs::remove_file(dir.join("root.lodpack"));
    }
}

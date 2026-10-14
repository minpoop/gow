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
/// shells and slabs: the eye shadow border (3), eyebrows (4), a face layer (5), tear line (6), eyelashes (7), the snow
/// flakes decal (76), the eyes' outer shell (78) and the long beard's hair cards (86, 87).
pub const SEE_THROUGH: [u16; 9] = [3, 4, 5, 6, 7, 76, 78, 86, 87];

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
    Ok((nodes, meshes, failed))
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
    // his materials (the see-through ones left out), each with its colour map or a neutral grey
    let used = outfit_materials(mesh, mg);
    let (nodes, meshes, failed) = kratos_meshes(&rs, &outfit, gpu, &toc, &mut file, [0.0; 3], skeleton, retargeted.as_ref(), &used)?;
    let tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    let mut textures: Vec<(String, Vec<u8>, u8)> = Vec::new();
    let mut mats = Vec::new();
    let (mut coloured, mut surfaced) = (0, 0);
    for m in &used {
        let look = looks.get(m);
        let stem = format!("kratos_{MODEL_ID}_m{m}_a");
        let dds = match look.and_then(|l| l.colour.as_ref()) {
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
        mats.push(material_with(MODEL_ID, &format!("kratos_m{m}"), &stem, &normal));
    }
    let solid = |c: [u8; 3]| erpart::dds_bc1(4, 4, &erpart::bc1_solid(c[0], c[1], c[2]));
    textures.push((format!("kratos_{MODEL_ID}_m"), solid([0, 160, 0]), 0));
    textures.push((format!("kratos_{MODEL_ID}_n"), solid([128, 128, 255]), 0));
    let size = add("bd", MODEL_ID, &nodes, &meshes, &mats, erpart::tpf(&textures));
    note.push_str(&format!("body {} draws ({tris} triangles, {size} bytes; {} failed {failed:?}), {} materials ({coloured} with their colour map, {surfaced} with their surface); ", meshes.len(), failed.len(), used.len()));
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
        // a colour map per material when GOWRING_COLOURS names a folder of `<material>.png` files
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
                colours.insert(m, crate::kratos_look::Look { colour: Some(crate::dds::Image { w: info.width, h: info.height, rgba, format: crate::dds::Format::Bc1 }), surface: None });
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

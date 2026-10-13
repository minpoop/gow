//! Kratos as an Elden Ring armour part, built on the player's PC from their Ragnarök install: his default outfit
//! ([`crate::kratos::DEFAULT_OUTFIT`], most detailed level) read from `r_heroa00.wad` and `root.lodpack`, written as
//! `bd_m_9701` / `bd_f_9701` with [`crate::erpart`].
//!
//! First version: the whole figure follows the character's root bone ("Master") rigidly, scaled to the player's
//! height, with placeholder colours. It answers whether Elden Ring loads a part we wrote; binding him to the player's
//! skeleton and his own textures come next.

use std::path::Path;

use anyhow::{bail, Result};

use crate::erpart::{self, Material, Mesh, Node, Vertex};
use crate::kratos::{draw_vertices, mesh_records, mg_parts, DEFAULT_OUTFIT};

/// The armour model id the part replaces (`bd_m_9701`, `bd_f_9701`): unused by the game.
pub const MODEL_ID: u16 = 9701;
/// Kratos is 1.94 m in his rest pose; the player is about 1.78 m.
pub const SCALE: f32 = 0.92;

pub fn stem(female: bool) -> String {
    format!("bd_{}_{MODEL_ID:04}", if female { "f" } else { "m" })
}

/// The model and texture pack, and a line about what went in.
pub fn build_flver_and_tpf(mesh: &[u8], mg: &[u8], gpu: &[u8], pack: &Path) -> Result<(Vec<u8>, Vec<u8>, String)> {
    let rs = mesh_records(mesh);
    let parts = mg_parts(mg);
    let records: Vec<usize> = DEFAULT_OUTFIT.iter().filter_map(|p| parts.get(*p as usize)).filter_map(|l| l.first()).flat_map(|l| l.first as usize..l.first as usize + l.draws as usize).collect();
    if records.is_empty() {
        bail!("no default outfit draws in the mesh tables");
    }
    let toc = crate::lodpack::Toc::read_from_pack(pack)?;
    let mut file = std::fs::File::open(pack)?;
    let mut cache: std::collections::HashMap<u64, Vec<u8>> = Default::default();
    let mut nodes = vec![Node { name: "Master".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 8 }];
    let mut meshes = Vec::new();
    let (mut failed, mut verts, mut tris) = (Vec::new(), 0, 0);
    for k in records {
        let Some(r) = rs.get(k) else { continue };
        let res: &[u8] = if r.hash == 0 {
            gpu
        } else {
            if !cache.contains_key(&r.hash) {
                match toc.read(&mut file, r.hash)? {
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
        let vertices: Vec<Vertex> = (0..v.position.len())
            .map(|i| {
                let n = v.normal.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                // any unit vector across the normal serves as a tangent until his normal maps come in
                let up = if n[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
                let t = [up[1] * n[2] - up[2] * n[1], up[2] * n[0] - up[0] * n[2], up[0] * n[1] - up[1] * n[0]];
                let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-6);
                Vertex {
                    position: v.position[i].map(|x| x * SCALE),
                    normal: n,
                    tangent: [t[0] / len, t[1] / len, t[2] / len, 1.0],
                    bones: [0; 4],
                    weights: [1.0, 0.0, 0.0, 0.0],
                    color: [255; 4],
                    uv: [v.uv[0].get(i).copied().unwrap_or([0.0; 2]), v.uv[1].get(i).copied().unwrap_or([0.0; 2])],
                }
            })
            .collect();
        verts += vertices.len();
        tris += v.triangles.len();
        meshes.push(Mesh { material: 0, node, vertices, indices: v.triangles.iter().flat_map(|t| t.map(|x| x as u32)).collect() });
    }
    if meshes.is_empty() {
        bail!("none of the default outfit's draws decoded");
    }
    let tex = |suffix: &str| format!("N:\\GR\\data\\INTERROOT_win64\\parts\\kratos\\tex\\kratos_{MODEL_ID}_{suffix}.tif");
    let material = Material {
        name: "kratos".into(),
        mtd: erpart::MATERIAL_MTD.into(),
        textures: vec![(erpart::ALBEDO.into(), tex("a")), (erpart::METALLIC.into(), tex("m")), (erpart::NORMAL.into(), tex("n"))],
    };
    let flver = erpart::flver2(&nodes, &[material], &meshes);
    let solid = |r, g, b| erpart::dds_bc1(4, 4, &erpart::bc1_solid(r, g, b));
    let tpf = erpart::tpf(&[
        (format!("kratos_{MODEL_ID}_a"), solid(196, 150, 118), 0),
        (format!("kratos_{MODEL_ID}_m"), solid(0, 160, 0), 0),
        (format!("kratos_{MODEL_ID}_n"), solid(128, 128, 255), 0),
    ]);
    let note = format!("{} draws ({verts} vertices, {tris} triangles) on the root bone; {} would not decode {failed:?}; model {} bytes", meshes.len(), failed.len(), flver.len());
    Ok((flver, tpf, note))
}

/// Both part files (male and female), as (file name, bytes).
pub fn part_files(flver: &[u8], tpf: &[u8]) -> Vec<(String, Vec<u8>)> {
    [false, true].iter().map(|f| (format!("{}.partsbnd.dcx", stem(*f)), erpart::partsbnd(&stem(*f), flver.to_vec(), tpf.to_vec()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (flver, tpf, note) = build_flver_and_tpf(&mesh, &mg, &gpu, &dir.join("root.lodpack")).unwrap();
        println!("{real} real resources; {note}");
        for (name, bytes) in part_files(&flver, &tpf) {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let _ = std::fs::remove_file(dir.join("root.lodpack"));
    }
}

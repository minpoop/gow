//! Kratos's colours, from the player's own Ragnarök install: for each material his default outfit uses, its colour
//! map, decoded to RGBA.
//!
//! The mesh's material table is the id-60 chunk right after `MESH_heroa00_0` (one `MAT_*` name per material); each
//! `MAT_*` chunk is followed by its own id-60 chunk listing the textures it references (`TX_<name>_<hex key>`).
//! Ragnarök's character materials are layered (region maps, tiling detail maps) but each also carries the baked
//! result: `*_gen_0d` (or `_gen_2d`), and for Kratos's head and arms `*_m_paint_cine_*`. Those are taken first, then
//! a plain `*_d` map. A texture lives in a texture pack (`*.texpack`, found through the packs' tables, see
//! [`crate::texpack`]) or, when small, in the WAD itself ([`crate::wadtex`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::dds::Image;
use crate::wtoc::Entry;

/// The colour map among a material's texture references, if it has one.
pub fn colour_map(refs: &[String]) -> Option<&String> {
    let stem = |t: &String| -> String {
        // the name without its `TX_` and its hex key (a 60-character name may have lost the key)
        let t = t.strip_prefix("TX_").unwrap_or(t);
        match t.rsplit_once('_') {
            Some((head, key)) if key.len() == 16 && u64::from_str_radix(key, 16).is_ok() => head.to_string(),
            _ => t.to_string(),
        }
    };
    let baked = |t: &&String| {
        let s = stem(t);
        s.split("_gen_").nth(1).is_some_and(|g| g.len() == 2 && g.ends_with('d') && g.as_bytes()[0].is_ascii_digit())
    };
    let painted = |t: &&String| stem(t).contains("_m_paint_cine");
    let plain = |t: &&String| stem(t).ends_with("_d") || stem(t).ends_with("_color");
    let tx: Vec<&String> = refs.iter().filter(|r| r.starts_with("TX_")).collect();
    tx.iter().copied().find(baked).or_else(|| tx.iter().copied().find(painted)).or_else(|| tx.iter().copied().find(plain))
}

/// The texture packs next to the WAD: the shared one (`root.texpack`, which holds Kratos's) when `all` is false, else
/// every other one.
pub fn texture_packs(wad_dir: &Path, all: bool) -> Vec<(PathBuf, crate::texpack::Toc)> {
    let is_root = |p: &PathBuf| p.to_string_lossy().to_ascii_lowercase().ends_with("root.texpack.toc");
    let mut names: Vec<PathBuf> = std::fs::read_dir(wad_dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().to_ascii_lowercase().ends_with(".texpack.toc")).collect()).unwrap_or_default();
    names.retain(|p| is_root(p) != all);
    names.sort();
    names.into_iter().filter_map(|p| crate::texpack::Toc::read(&p).ok().map(|t| (p.with_extension(""), t))).collect()
}

/// Decode the texture `name` (its reference id's key in `id_key`) from the packs, or from the WAD (`buf`, `es`).
pub fn decode_texture(name: &str, id_key: u64, buf: &[u8], es: &[Entry], packs: &[(PathBuf, crate::texpack::Toc)]) -> anyhow::Result<Image> {
    let key = name.rsplit('_').next().filter(|h| h.len() == 16).and_then(|h| u64::from_str_radix(h, 16).ok());
    let found = [key, Some(id_key), Some(id_key.swap_bytes())].into_iter().flatten().find_map(|k| packs.iter().find_map(|(p, t)| t.find(k).map(|x| (p, x))));
    match found {
        Some((p, t)) => {
            let mut f = std::fs::File::open(p)?;
            let blocks = t.records.iter().map(|r| crate::texpack::read_record(&mut f, r)).collect::<anyhow::Result<Vec<_>>>()?;
            crate::texpack::decode(&blocks)
        }
        None => {
            let bytes = |e: &Entry| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize);
            let chunk = |id: u16| es.iter().find(|e| e.name == name && e.id == id).and_then(bytes);
            let (Some(parm), Some(gpu)) = (chunk(1).and_then(crate::wadtex::parse_parm), chunk(29)) else {
                anyhow::bail!("{name} is in no texture pack and not in the WAD's first bytes");
            };
            Ok(crate::wadtex::decode(&parm, gpu)?.0)
        }
    }
}

/// For each of `materials` (indices into the mesh's material table), its colour map decoded and scaled to at most
/// `max_side`, and a line per material saying what was found. `buf` holds the start of r_heroa00.wad (its chunk table
/// `es`), `wad_dir` the folder with the texture packs.
pub fn outfit_colours(buf: &[u8], es: &[Entry], wad_dir: &Path, materials: &[u16], max_side: u32) -> (BTreeMap<u16, Image>, String) {
    let mut out = BTreeMap::new();
    let mut note = String::new();
    let bytes = |e: &Entry| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize);
    let Some(mesh_at) = es.iter().position(|e| e.name == "MESH_heroa00_0" && e.id == 1) else { return (out, "no MESH_heroa00_0".into()) };
    let Some(table) = es.get(mesh_at + 1).filter(|e| e.id == 60).and_then(bytes) else { return (out, "no material table after MESH_heroa00_0".into()) };
    let names = crate::kratos::material_names(table);
    let mut packs = texture_packs(wad_dir, false);
    let mut all_read = false;
    note.push_str(&format!("{} texture pack(s) first; ", packs.len()));
    for &m in materials {
        let Some(name) = names.get(m as usize) else { continue };
        let at = es.iter().position(|e| &e.name == name && e.id == 1);
        let refs = at.and_then(|a| es.get(a + 1)).filter(|e| e.id == 60).and_then(bytes).map(crate::kratos::references).unwrap_or_default();
        let ref_names: Vec<String> = refs.iter().map(|r| r.1.clone()).collect();
        let Some(tex) = colour_map(&ref_names) else {
            note.push_str(&format!("{m}: no colour map; "));
            continue;
        };
        let id_key = refs.iter().find(|r| &r.1 == tex).map(|r| u64::from_le_bytes(r.0[8..16].try_into().unwrap())).unwrap_or(0);
        let mut decoded = decode_texture(tex, id_key, buf, es, &packs);
        if decoded.is_err() && !all_read {
            // not in the shared pack: read the others' tables once
            packs.extend(texture_packs(wad_dir, true));
            all_read = true;
            note.push_str(&format!("{} texture packs in all; ", packs.len()));
            decoded = decode_texture(tex, id_key, buf, es, &packs);
        }
        match decoded {
            Ok(img) => {
                note.push_str(&format!("{m}: {tex} {}x{}; ", img.w, img.h));
                out.insert(m, crate::dds::downscale(&img, max_side));
            }
            Err(e) => note.push_str(&format!("{m}: {tex} failed ({e:#}); ")),
        }
    }
    (out, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baked_colour_map_comes_first_then_the_painted_one_then_a_plain_one() {
        let v = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let acc = v(&["TX_dynamicmaterial_m1_535B93377186405F", "TX_kratos00_int9_acc_sand_m_gen_0n_4474E39761581525", "TX_kratos00_int9_acc_m_gen_0d_111E00F602CC3843", "TX_fabric07_d_BA8EC0E140118F71"]);
        assert_eq!(colour_map(&acc).unwrap(), "TX_kratos00_int9_acc_m_gen_0d_111E00F602CC3843");
        let head = v(&["TX_kratos00_wrinklemap_maxregions_1", "TX_kratos00_int9_head_m_paint_cine_", "TX_kratos00_int9_head_sand_m_gen_0n_ABCDEF0123456789", "TX_kratosashsmall_d_0123456789ABCDEF"]);
        assert_eq!(colour_map(&head).unwrap(), "TX_kratos00_int9_head_m_paint_cine_");
        let crystal = v(&["TX_bifrost00_m_gen_2n_0123456789ABCDEF", "TX_bifrost00_m_gen_2d_0123456789ABCDEF"]);
        assert_eq!(colour_map(&crystal).unwrap(), "TX_bifrost00_m_gen_2d_0123456789ABCDEF");
        let eye = v(&["TX_death01_112E6D2408347E77", "TX_kratos00_int9_eye_d_0123456789ABCDEF"]);
        assert_eq!(colour_map(&eye).unwrap(), "TX_kratos00_int9_eye_d_0123456789ABCDEF");
        assert!(colour_map(&v(&["TX_death01_112E6D2408347E77", "TX_standardeye00_int9_tearline_n_0123456789ABCDEF"])).is_none());
    }
}

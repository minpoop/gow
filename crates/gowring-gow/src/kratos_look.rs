//! Kratos's colours, from the player's own Ragnarök install: for each material his default outfit uses, its colour
//! map, decoded to RGBA.
//!
//! The mesh's material table is the id-60 chunk right after `MESH_heroa00_0` (one `MAT_*` name per material); each
//! `MAT_*` chunk is followed by its own id-60 chunk listing the textures it references (`TX_<name>_<hex key>`).
//! Ragnarök's character materials are layered (region maps, tiling detail maps) but each also carries the baked
//! result: `*_gen_0d` (or `_gen_2d`), and for Kratos's head and arms `*_m_paint_cine_*`. Those are taken first, then
//! a plain `*_d` map. Its surface comes from the normal map (`*_gen_0n`) and the gloss map (`*_gen_0g`; for the head,
//! whose list has no `_gen_0g`, the second `*_m_paint_cine_*`, which sits where the others list their gloss), joined
//! the way Elden Ring's normal maps hold them: the normal's X and Y in red and green and the shininess (the gloss) in
//! blue (the game's own stand-in `SYSTEX_Dummy_n` is 128, 128, 128, 255). Until 0.1.65 blue held 255 - gloss, which
//! made the roughest parts the shiniest. A texture lives in a texture pack (`*.texpack`, found through the packs' tables, see
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
    // a baked set whose names the game itself cut short (`kratosspear00_set_handle_e_gen_0_<key>`, four in a row in the
    // full names' order: colour, normal, gloss, ao): the first is the colour, and it wins over a tiling detail map
    // (`wood01_d`) listed with it
    let cut_short = |t: &&String| stem(t).split("_gen_").nth(1).is_some_and(|g| g.len() == 1 && g.as_bytes()[0].is_ascii_digit());
    let plain = |t: &&String| stem(t).ends_with("_d") || stem(t).ends_with("_color");
    let tx: Vec<&String> = refs.iter().filter(|r| r.starts_with("TX_")).collect();
    tx.iter().copied().find(baked).or_else(|| tx.iter().copied().find(painted)).or_else(|| tx.iter().copied().find(cut_short)).or_else(|| tx.iter().copied().find(plain))
}

fn stem(t: &str) -> &str {
    let t = t.strip_prefix("TX_").unwrap_or(t);
    match t.rsplit_once('_') {
        Some((head, key)) if key.len() == 16 && u64::from_str_radix(key, 16).is_ok() => head,
        _ => t,
    }
}

/// A material's own metal mask among its references, the region map its metal is painted with
/// (`kratos00_int9_legs_m2_metals`, `youngkratos_boots_m1_metal`: `_m<digits>_metal...`), not a shared tiling map
/// (`metal01_n`).
pub fn metal_mask(refs: &[String]) -> Option<&String> {
    refs.iter().filter(|r| r.starts_with("TX_")).find(|t| {
        let words: Vec<&str> = stem(t).split('_').collect();
        words.windows(2).any(|w| w[0].len() >= 2 && w[0].starts_with('m') && w[0][1..].bytes().all(|c| c.is_ascii_digit()) && w[1].starts_with("metal"))
    })
}

/// A material's own stitch mask among its references, the region map its lacing is painted with
/// (`kratos00_int9_waist_m5_stitches`, `kratos00_int9_shoulder_m5_stitch`: `_m<digits>_stitch...`).
pub fn stitch_mask(refs: &[String]) -> Option<&String> {
    refs.iter().filter(|r| r.starts_with("TX_")).find(|t| {
        let words: Vec<&str> = stem(t).split('_').collect();
        words.windows(2).any(|w| w[0].len() >= 2 && w[0].starts_with('m') && w[0][1..].bytes().all(|c| c.is_ascii_digit()) && w[1].starts_with("stitch"))
    })
}

/// The colour of the Black Bear Set's lacing (the zigzag stitching along the harness, shoulder and belt edges in the
/// set's art) at the leather's full brightness: an amber gold, hue 40 degrees (the art's stitching measures 33-39;
/// 0.1.66-0.1.74 used a red orange, hue 25, [205, 105, 35]: "the orange on the armor is too much its supposed to be
/// yellow").
pub const LACING_GOLD: [f32; 3] = [225.0, 170.0, 60.0];

/// Ragnarök's black dye as far as a colour map shows it: the leather, cloth and fur (everything outside `metal`, the
/// material's own metal mask: its red, stretched over the colour map, the side that covers less of it counting as
/// metal) go to a near-black grey that keeps their shading; the metal keeps its colour. Returns the share counted as
/// metal.
pub fn blacken(img: &mut Image, metal: Option<&Image>) -> f32 {
    blacken_laced(img, metal, None).0
}

/// [`blacken`], and where `lacing` (the material's own stitch mask, see [`stitch_mask`]; red, stretched over the
/// colour map) marks the stitching, [`LACING_GOLD`] at the brightness the stitch had before the dye. Returns the
/// shares counted as metal and as lacing.
pub fn blacken_laced(img: &mut Image, metal: Option<&Image>, lacing: Option<&Image>) -> (f32, f32) {
    let at = |m: &Image, x: u32, y: u32, w: u32, h: u32| -> f32 {
        let (mx, my) = ((x as u64 * m.w as u64 / w as u64) as u32, (y as u64 * m.h as u64 / h as u64) as u32);
        m.rgba.get(((my * m.w + mx) * 4) as usize).copied().unwrap_or(0) as f32 / 255.0
    };
    let mask = metal.filter(|m| m.w > 0 && m.h > 0).map(|m| {
        let lit = m.rgba.chunks_exact(4).filter(|p| p[0] >= 128).count() as f32 / (m.w * m.h) as f32;
        (m, lit > 0.5)
    });
    let lacing = lacing.filter(|m| m.w > 0 && m.h > 0);
    let (mut kept, mut laced) = (0usize, 0usize);
    for y in 0..img.h {
        for x in 0..img.w {
            let i = ((y * img.w + x) * 4) as usize;
            let w = mask.map_or(0.0, |(m, flip)| {
                let v = at(m, x, y, img.w, img.h);
                if flip { 1.0 - v } else { v }
            });
            kept += (w >= 0.5) as usize;
            let (r, g, b) = (img.rgba[i] as f32, img.rgba[i + 1] as f32, img.rgba[i + 2] as f32);
            let lum = 0.299 * r + 0.587 * g + 0.114 * b;
            let dark = 6.0 + 0.28 * lum;
            // the lacing goes gold where it is not metal
            let s = lacing.map_or(0.0, |m| at(m, x, y, img.w, img.h)) * (1.0 - w);
            laced += (s >= 0.5) as usize;
            let shade = (lum / 170.0).clamp(0.35, 1.2);
            for (k, c) in [r, g, b].into_iter().enumerate() {
                let dyed = dark + (c - dark) * w;
                img.rgba[i + k] = (dyed + (LACING_GOLD[k] * shade - dyed) * s).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    let n = (img.w * img.h).max(1) as f32;
    (kept as f32 / n, laced as f32 / n)
}

/// The coverage map among a cut-out material's references (hair cards, brows, lashes): an opacity map, named
/// `*_o`, `*_op`, `*_gen_<digit>o`, or with `opacity` or `alpha` in it (Ragnarök's own, e.g. `son00_eyelash_o`).
pub fn coverage_map(refs: &[String]) -> Option<&String> {
    refs.iter().filter(|r| r.starts_with("TX_")).find(|t| {
        let s = stem(t);
        s.ends_with("_o") || s.ends_with("_op") || baked(t, 'o') || s.contains("opacity") || s.contains("alpha")
    })
}

/// Could this texture be a strand mask, by its name: not a normal, occlusion or gloss map, not one of the shared
/// layering maps (`dynamicmaterial*`, region ids, wrinkles, `death*`), and not laid out on the second UV set.
pub fn coverage_candidate(t: &str) -> bool {
    let s = stem(t);
    let kind = s.rsplit("_gen_").next().filter(|_| s.contains("_gen_")).unwrap_or("");
    t.starts_with("TX_")
        && !(s.ends_with("_n") || s.ends_with("_ao") || s.ends_with("_nrm") || s.contains("normal") || kind.ends_with('n') || kind.ends_with("ao") || kind.ends_with('g'))
        && !["dynamicmaterial", "regionid", "wrinkle", "death", "uv2"].iter().any(|k| s.contains(k))
}

/// How mask-like an image is: the channel a coverage would come from (its alpha where that varies, else red) and the
/// shares of its pixels that are empty (under 32) and solid (over 192).
pub fn mask_stats(img: &Image) -> (char, f32, f32) {
    let alpha = (0.02..0.98).contains(&clear_share(img));
    let k = if alpha { 3 } else { 0 };
    let n = (img.w * img.h).max(1) as f32;
    let (mut zero, mut full) = (0usize, 0usize);
    for p in img.rgba.chunks_exact(4) {
        zero += (p[k] < 32) as usize;
        full += (p[k] > 192) as usize;
    }
    (if alpha { 'a' } else { 'r' }, zero as f32 / n, full as f32 / n)
}

/// The most mask-like of `seen` (named images): a strand mask is mostly empty with some solid strands (empty 20-97 %,
/// solid 2 % or more); the one with the most pixels at either end wins, hair-like names first among near equals.
/// With a line of every candidate's numbers.
pub fn best_coverage(seen: &[(String, Image)]) -> (Option<usize>, String) {
    let mut best: Option<(usize, f32)> = None;
    let mut line = Vec::new();
    for (i, (name, img)) in seen.iter().enumerate() {
        let (ch, zero, full) = mask_stats(img);
        line.push(format!("{name} {}x{} {ch}: {:.0} % empty, {:.0} % solid", img.w, img.h, zero * 100.0, full * 100.0));
        if (0.2..=0.97).contains(&zero) && full >= 0.02 {
            let hairy = ["hair", "beard", "brow", "lash", "strand", "fur"].iter().any(|k| name.contains(k));
            let score = zero + full + if hairy { 0.1 } else { 0.0 };
            if best.is_none_or(|b| score > b.1) {
                best = Some((i, score));
            }
        }
    }
    (best.map(|b| b.0), line.join("; "))
}

/// Share of an image's pixels that are see-through (alpha under 128).
fn clear_share(img: &Image) -> f32 {
    img.rgba.chunks_exact(4).filter(|p| p[3] < 128).count() as f32 / (img.w * img.h).max(1) as f32
}

/// A cut-out material's colour map: colour from `colour` (its image, and whether it is laid out on the second UV set,
/// `*uv2*`, which the part does not carry: then its average colour is used throughout), coverage in alpha from
/// `coverage` (its alpha where that varies, else its red) or else from the colour map's own alpha. At the coverage's
/// size, at most `max_side`. None when nothing gives a coverage (the material would draw as solid cards).
pub fn cut_out_image(colour: Option<(&Image, bool)>, coverage: Option<&Image>, max_side: u32) -> Option<(Image, String)> {
    let varies = |img: &Image| (0.02..0.98).contains(&clear_share(img));
    let (base, from) = match (coverage, colour) {
        (Some(c), _) => (crate::dds::downscale(c, max_side), if varies(c) { "the coverage map's alpha" } else { "the coverage map's red" }),
        (None, Some((img, false))) if varies(img) => (crate::dds::downscale(img, max_side), "the colour map's alpha"),
        _ => return None,
    };
    let use_alpha = coverage.is_none() || varies(&base);
    let average = |img: &Image| -> [u8; 3] {
        let n = (img.w * img.h).max(1) as u64;
        std::array::from_fn(|k| (img.rgba.chunks_exact(4).map(|p| p[k] as u64).sum::<u64>() / n) as u8)
    };
    let flat = match colour {
        Some((img, true)) => Some(average(img)),
        None => Some([70, 52, 40]),
        _ => None,
    };
    let (w, h) = (base.w, base.h);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let at = ((y * w + x) * 4) as usize;
            let a = if use_alpha { base.rgba[at + 3] } else { base.rgba[at] };
            let c = match (flat, colour) {
                (Some(f), _) => f,
                (None, Some((img, _))) => {
                    let (cx, cy) = ((x as u64 * img.w as u64 / w as u64) as u32, (y as u64 * img.h as u64 / h as u64) as u32);
                    let ct = ((cy * img.w + cx) * 4) as usize;
                    [img.rgba[ct], img.rgba[ct + 1], img.rgba[ct + 2]]
                }
                (None, None) => unreachable!(),
            };
            rgba.extend_from_slice(&[c[0], c[1], c[2], a]);
        }
    }
    let how = format!(
        "coverage from {from}, colour {}",
        match (flat, colour) {
            (Some(f), Some(_)) => format!("the colour map's average {f:?} (it is laid out on the second UV set)"),
            (Some(f), None) => format!("a dark brown {f:?} (no colour map)"),
            _ => "from the colour map".into(),
        }
    );
    let img = Image { w, h, rgba, format: base.format };
    let share = 1.0 - clear_share(&img);
    Some((img, format!("{how}, {:.0} % drawn", share * 100.0)))
}

/// A baked map of `kind` ('n' normal, 'g' gloss): `*_gen_<digit><kind>`.
fn baked(t: &str, kind: char) -> bool {
    stem(t).split("_gen_").nth(1).is_some_and(|g| g.len() == 2 && g.ends_with(kind) && g.as_bytes()[0].is_ascii_digit())
}

/// The normal map and the gloss map among a material's texture references.
pub fn surface_maps(refs: &[String]) -> (Option<&String>, Option<&String>) {
    let tx: Vec<&String> = refs.iter().filter(|r| r.starts_with("TX_")).collect();
    let normal = tx.iter().copied().find(|t| baked(t, 'n'));
    let painted: Vec<&String> = tx.iter().copied().filter(|t| stem(t).contains("_m_paint_cine")).collect();
    let gloss = tx.iter().copied().find(|t| baked(t, 'g')).or_else(|| painted.get(1).copied().filter(|_| painted.len() == 2 && !tx.iter().any(|t| baked(t, 'g'))));
    (normal, gloss)
}

/// Elden Ring's normal map from Ragnarök's normal map (X, Y in red, green) and gloss map (red), at the normal map's
/// size: red and green as they are, blue the shininess (gloss x `gloss_scale`, nearest gloss pixel; [`MATTE`] when
/// there is none).
/// The blue (shininess) of a surface with no gloss map: nearly matte.
pub const MATTE: u8 = 40;

pub fn surface_image(normal: &Image, gloss: Option<&Image>, gloss_scale: f32) -> Image {
    let (w, h) = (normal.w, normal.h);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let at = ((y * w + x) * 4) as usize;
            let shine = match gloss {
                Some(g) => {
                    let (gx, gy) = ((x as u64 * g.w as u64 / w as u64) as u32, (y as u64 * g.h as u64 / h as u64) as u32);
                    (g.rgba[((gy * g.w + gx) * 4) as usize] as f32 * gloss_scale.clamp(0.0, 1.0)).round() as u8
                }
                None => MATTE,
            };
            rgba.extend_from_slice(&[normal.rgba[at], normal.rgba[at + 1], shine, 255]);
        }
    }
    Image { w, h, rgba, format: normal.format }
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

/// Every texture `materials` of `MESH_heroa00_0` reference, as PNG files (`<material>_<n>_<texture>.png`, at most
/// `max_side`, alpha kept), with a line per texture: for looking at a material's maps away from the player's PC (which
/// of them holds the strands of a fur, say).
pub fn material_textures_png(buf: &[u8], es: &[Entry], wad_dir: &Path, materials: &[u16], max_side: u32) -> (Vec<(String, Vec<u8>)>, String) {
    let mut files = Vec::new();
    let mut lines = Vec::new();
    let bytes = |e: &Entry| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize);
    let Some(mesh_at) = es.iter().position(|e| e.name == "MESH_heroa00_0" && e.id == 1) else { return (files, "no MESH_heroa00_0".into()) };
    let Some(table) = es.get(mesh_at + 1).filter(|e| e.id == 60).and_then(bytes) else { return (files, "no material table".into()) };
    let names = crate::kratos::material_names(table);
    let mut packs = texture_packs(wad_dir, false);
    packs.extend(texture_packs(wad_dir, true));
    for &m in materials {
        let Some(name) = names.get(m as usize) else { continue };
        let at = es.iter().position(|e| &e.name == name && e.id == 1);
        let refs = at.and_then(|a| es.get(a + 1)).filter(|e| e.id == 60).and_then(bytes).map(crate::kratos::references).unwrap_or_default();
        for (n, (id, tex)) in refs.iter().enumerate().filter(|(_, r)| r.1.starts_with("TX_")) {
            let key = u64::from_le_bytes(id[8..16].try_into().unwrap_or([0; 8]));
            match decode_texture(tex, key, buf, es, &packs) {
                Ok(img) => {
                    let small = crate::dds::downscale(&img, max_side);
                    let alpha = small.rgba.chunks_exact(4).filter(|p| p[3] < 128).count() as f32 / (small.w * small.h).max(1) as f32;
                    lines.push(format!("{m} {n} {tex}: {}x{} {:?}, {:.0} % clear", img.w, img.h, img.format, 100.0 * alpha));
                    if let Ok(png) = crate::dds::to_png(&small) {
                        files.push((format!("{m:02}_{n:02}_{}.png", stem(tex)), png));
                    }
                }
                Err(e) => lines.push(format!("{m} {n} {tex}: {e:#}")),
            }
        }
    }
    (files, lines.join("\n"))
}

/// How strand materials are cut out (see [`cut_out_image`]): which materials are strands, which of them are drawn solid
/// when no coverage is found (a whole cloak rather than none), and which take their coverage from their own colour
/// map's alpha (strengthened by [`COLOUR_ALPHA_GAIN`]) instead of searching their other maps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Strands<'a> {
    pub materials: &'a [u16],
    pub solid: &'a [u16],
    pub colour_alpha: &'a [u16],
}

/// How much a colour map's own alpha is strengthened when it is the coverage: Ragnarök layers its fur cards and blends
/// them, and Elden Ring's cut-out keeps only what is over half, so the thin strands would mostly vanish.
pub const COLOUR_ALPHA_GAIN: f32 = 2.0;

/// A material's look for the part: its colour map, and its surface (an Elden Ring normal map, see [`surface_image`]).
#[derive(Default)]
pub struct Look {
    pub colour: Option<Image>,
    pub surface: Option<Image>,
    /// The colour map carries cut-out coverage in its alpha (see [`cut_out_image`]).
    pub cut_out: bool,
}

/// For each of `materials` (indices into the mesh's material table), its colour map scaled to at most `max_side` and
/// its surface scaled to at most `max_surface`, and a line per material saying what was found. The `strands` (hair
/// cards and the like) get their coverage in the colour map's alpha ([`cut_out_image`], [`Strands`]) and list their
/// textures in the line. Those in `dye` are dyed black ([`blacken`]). `buf` holds the start of r_heroa00.wad (its
/// chunk table `es`), `wad_dir` the folder with the texture packs. Gloss is scaled by [`BODY_GLOSS`].
#[allow(clippy::too_many_arguments)]
pub fn outfit_looks(buf: &[u8], es: &[Entry], wad_dir: &Path, materials: &[u16], strands: Strands, max_side: u32, max_surface: u32, dye: &[u16]) -> (BTreeMap<u16, Look>, String) {
    model_looks(buf, es, wad_dir, "MESH_heroa00_0", materials, strands, max_side, max_surface, BODY_GLOSS, &[], dye)
}

/// How much of Ragnarök's gloss Kratos's body keeps. The 0.1.62 to 0.1.64 cuts made him shinier, not less, because
/// blue was written as 255 - gloss; with blue the gloss itself, a little under full keeps skin and leather dry.
pub const BODY_GLOSS: f32 = 0.6;

/// [`outfit_looks`] for any model: the materials are indices into the table after its `mesh_chunk` (`MESH_<m>_0`).
/// Textures whose names hold one of `skip` (lower case, e.g. a weapon's frost or gold variants, see
/// [`crate::weapon_part::PartRules::reject`]) are passed over when a map is chosen, so a material layered with a
/// variant's maps draws with its own.
#[allow(clippy::too_many_arguments)]
pub fn model_looks(buf: &[u8], es: &[Entry], wad_dir: &Path, mesh_chunk: &str, materials: &[u16], strands: Strands, max_side: u32, max_surface: u32, gloss_scale: f32, skip: &[&str], dye: &[u16]) -> (BTreeMap<u16, Look>, String) {
    let mut out = BTreeMap::new();
    let mut note = String::new();
    let bytes = |e: &Entry| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize);
    let Some(mesh_at) = es.iter().position(|e| e.name == mesh_chunk && e.id == 1) else { return (out, format!("no {mesh_chunk}")) };
    let Some(table) = es.get(mesh_at + 1).filter(|e| e.id == 60).and_then(bytes) else { return (out, format!("no material table after {mesh_chunk}")) };
    let names = crate::kratos::material_names(table);
    let mut packs = texture_packs(wad_dir, false);
    let mut all_read = false;
    note.push_str(&format!("{} texture pack(s) first; ", packs.len()));
    for &m in materials {
        let Some(name) = names.get(m as usize) else { continue };
        let at = es.iter().position(|e| &e.name == name && e.id == 1);
        let refs = at.and_then(|a| es.get(a + 1)).filter(|e| e.id == 60).and_then(bytes).map(crate::kratos::references).unwrap_or_default();
        let all_names: Vec<String> = refs.iter().map(|r| r.1.clone()).collect();
        // the names maps are chosen from: skipped ones blanked, so each keeps its place beside its id (a marker counts at
        // the start of a word, as `weapon_part` matches it: "frost" skips `frost00`, not `bifrost00`)
        let ref_names: Vec<String> = all_names.iter().map(|r| if skip.iter().any(|k| crate::weapon_part::starts_a_word(r, k)) { String::new() } else { r.clone() }).collect();
        // the n-th reference of that name: names cut at 60 characters can repeat, the ids tell them apart
        let mut get = |tex: &String| -> anyhow::Result<Image> {
            let nth = ref_names.iter().take_while(|r| !std::ptr::eq(*r, tex)).filter(|r| *r == tex).count();
            let id_key = refs.iter().filter(|r| &r.1 == tex).nth(nth).map(|r| r.0[8..16].try_into().map(u64::from_le_bytes).unwrap_or(0)).unwrap_or(0);
            let mut decoded = decode_texture(tex, id_key, buf, es, &packs);
            if decoded.is_err() && !all_read {
                // not in the shared pack: read the others' tables once
                packs.extend(texture_packs(wad_dir, true));
                all_read = true;
                decoded = decode_texture(tex, id_key, buf, es, &packs);
            }
            decoded
        };
        let mut look = Look::default();
        // the black dye, sparing what the material's own metal mask marks, with its lacing gold
        let mut named_map = |find: fn(&[String]) -> Option<&String>| -> Option<(String, Image)> { find(&ref_names).and_then(|t| get(t).ok().map(|i| (stem(t).to_string(), crate::dds::downscale(&i, max_side)))) };
        let (mask, lacing) = if dye.contains(&m) { (named_map(metal_mask), named_map(stitch_mask)) } else { (None, None) };
        let dyed = |img: &mut Image, note: &mut String| {
            let (kept, laced) = blacken_laced(img, mask.as_ref().map(|m| &m.1), lacing.as_ref().map(|m| &m.1));
            match &mask {
                Some((name, _)) => note.push_str(&format!(", dyed black (metal from {name}: {:.0} % kept)", 100.0 * kept)),
                None => note.push_str(", dyed black"),
            }
            if let Some((name, _)) = &lacing {
                note.push_str(&format!(" (lacing gold from {name}: {:.0} %)", 100.0 * laced));
            }
        };
        if strands.materials.contains(&m) {
            let stems: Vec<&str> = all_names.iter().filter(|r| r.starts_with("TX_")).map(|r| stem(r)).collect();
            note.push_str(&format!("{m} (cut-out; textures {stems:?}): "));
            let mut colour = colour_map(&ref_names).and_then(|t| get(t).ok().map(|img| (img, stem(t).contains("uv2"))));
            if dye.contains(&m) {
                if let Some((img, _)) = colour.as_mut() {
                    dyed(img, &mut note);
                    note.push_str(": ");
                }
            }
            // the colour map's own alpha, where the material is known to carry its strands there
            let own_alpha = strands.colour_alpha.contains(&m) && colour.as_ref().is_some_and(|(img, uv2)| !uv2 && (0.02..0.98).contains(&clear_share(img)));
            let coverage = match coverage_map(&ref_names) {
                _ if own_alpha => {
                    note.push_str(&format!("coverage from its own colour map's alpha (x{COLOUR_ALPHA_GAIN}), "));
                    None
                }
                Some(t) => get(t).ok(),
                None => {
                    // no opacity map by name: the strand-like textures' pixels tell
                    let mut seen: Vec<(String, Image)> = Vec::new();
                    for t in ref_names.iter().filter(|r| coverage_candidate(r)) {
                        if let Ok(img) = get(t) {
                            seen.push((stem(t).to_string(), crate::dds::downscale(&img, max_side)));
                        }
                    }
                    let (best, line) = best_coverage(&seen);
                    note.push_str(&format!("candidates [{line}], "));
                    best.map(|i| {
                        note.push_str(&format!("coverage {}, ", seen[i].0));
                        seen.swap_remove(i).1
                    })
                }
            };
            match cut_out_image(colour.as_ref().map(|(img, uv2)| (img, *uv2)), coverage.as_ref(), max_side) {
                Some((mut img, how)) => {
                    if own_alpha {
                        for p in img.rgba.chunks_exact_mut(4) {
                            p[3] = (p[3] as f32 * COLOUR_ALPHA_GAIN).min(255.0) as u8;
                        }
                        let drawn = img.rgba.chunks_exact(4).filter(|p| p[3] >= 128).count() as f32 / (img.w * img.h).max(1) as f32;
                        note.push_str(&format!("{:.0} % drawn after the gain, ", 100.0 * drawn));
                    }
                    note.push_str(&format!("{}x{}, {how}", img.w, img.h));
                    look.colour = Some(img);
                    look.cut_out = true;
                }
                None => match colour.as_ref().filter(|_| strands.solid.contains(&m)) {
                    Some((img, _)) => {
                        let img = crate::dds::downscale(img, max_side);
                        note.push_str(&format!("no coverage found, drawn solid ({}x{})", img.w, img.h));
                        look.colour = Some(img);
                    }
                    None => note.push_str("no coverage found, left out"),
                },
            }
        } else {
            match colour_map(&ref_names) {
                Some(tex) => match get(tex) {
                    Ok(img) => {
                        note.push_str(&format!("{m}: {} {}x{}", stem(tex), img.w, img.h));
                        let mut img = crate::dds::downscale(&img, max_side);
                        if dye.contains(&m) {
                            dyed(&mut img, &mut note);
                        }
                        look.colour = Some(img);
                    }
                    Err(e) => note.push_str(&format!("{m}: {} failed ({e:#})", stem(tex))),
                },
                None => note.push_str(&format!("{m}: no colour map")),
            }
        }
        let (normal, gloss) = surface_maps(&ref_names);
        if let Some(n) = normal {
            match get(n) {
                Ok(img) => {
                    let n_img = crate::dds::downscale(&img, max_surface);
                    let g_img = gloss.and_then(|g| get(g).ok()).map(|g| crate::dds::downscale(&g, max_surface));
                    note.push_str(&format!(", normal {}x{}{}", img.w, img.h, if g_img.is_some() { " and gloss" } else { "" }));
                    look.surface = Some(surface_image(&n_img, g_img.as_ref(), gloss_scale));
                }
                Err(e) => note.push_str(&format!(", normal failed ({e:#})")),
            }
        }
        note.push_str("; ");
        out.insert(m, look);
    }
    note.insert_str(0, &format!("{} texture packs read; ", packs.len()));
    (out, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_take_the_baked_normal_and_gloss_maps_or_the_heads_second_painted_map() {
        let v = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let acc = v(&["TX_kratos00_int9_acc_m_gen_0d_111E00F602CC3843", "TX_kratos00_int9_acc_sand_m_gen_0n_4474E39761581525", "TX_kratos00_int9_acc_m_gen_0g_4D54C547F92E74E5", "TX_metal01_n_4198919D2DD536E2"]);
        let (n, g) = surface_maps(&acc);
        assert_eq!((n.unwrap().as_str(), g.unwrap().as_str()), ("TX_kratos00_int9_acc_sand_m_gen_0n_4474E39761581525", "TX_kratos00_int9_acc_m_gen_0g_4D54C547F92E74E5"));
        let head = v(&["TX_kratos00_int9_head_m_paint_cine_", "TX_kratos00_int9_head_sand_m_gen_0n_ABCDEF0123456789", "TX_kratos00_int9_head_m_paint_cine_", "TX_kratos_head_o_0123456789ABCDEF"]);
        let (n, g) = surface_maps(&head);
        assert!(n.unwrap().contains("gen_0n"));
        assert!(std::ptr::eq(g.unwrap(), &head[2]));
        // red and green from the normal, blue = the gloss (Elden Ring's shininess)
        let normal = Image { w: 2, h: 1, rgba: vec![10, 20, 30, 40, 50, 60, 70, 80], format: crate::dds::Format::Bc5 };
        let gloss = Image { w: 1, h: 1, rgba: vec![200, 0, 0, 255], format: crate::dds::Format::Bc4 };
        assert_eq!(surface_image(&normal, Some(&gloss), 1.0).rgba, vec![10, 20, 200, 255, 50, 60, 200, 255]);
        assert_eq!(surface_image(&normal, None, 1.0).rgba[2], MATTE);
        // half the gloss: less shiny
        assert_eq!(surface_image(&normal, Some(&gloss), 0.5).rgba[2], 100);
    }

    #[test]
    fn strands_take_coverage_from_their_opacity_map_or_their_colour_maps_alpha_and_a_uv2_colour_flat() {
        use crate::dds::Format;
        let v = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let hair = v(&["TX_kratosv2_hair_01_uv2_color_0DC3A62E063ED50B", "TX_kratosv2_hair_01_o_0123456789ABCDEF", "TX_kratosv2_hair_01_n_0123456789ABCDEF"]);
        assert_eq!(coverage_map(&hair).unwrap(), &hair[1]);
        assert_eq!(coverage_map(&v(&["TX_kratos00_brow_gen_0o_0123456789ABCDEF"])).unwrap(), "TX_kratos00_brow_gen_0o_0123456789ABCDEF");
        assert!(coverage_map(&v(&["TX_kratos00_int9_head_gen_0ao_0123456789ABCDEF", "TX_kratos00_int9_head_m_gen_0d_0123456789ABCDEF"])).is_none());
        // an opacity map in red (BC4: alpha all 255) with one strand pixel; the colour map on UV2 gives its average
        let coverage = Image { w: 2, h: 1, rgba: vec![255, 0, 0, 255, 0, 0, 0, 255], format: Format::Bc4 };
        let uv2 = Image { w: 2, h: 1, rgba: vec![100, 60, 40, 255, 120, 80, 60, 255], format: Format::Bc1 };
        let (img, how) = cut_out_image(Some((&uv2, true)), Some(&coverage), 1024).unwrap();
        assert_eq!(img.rgba, vec![110, 70, 50, 255, 110, 70, 50, 0], "{how}");
        // no opacity map: the colour map's own alpha, colours as they are
        let own = Image { w: 2, h: 1, rgba: vec![10, 20, 30, 255, 40, 50, 60, 0], format: Format::Bc1 };
        assert_eq!(cut_out_image(Some((&own, false)), None, 1024).unwrap().0.rgba, own.rgba);
        // nothing gives a coverage: left out rather than drawn as solid cards
        assert!(cut_out_image(Some((&uv2, false)), None, 1024).is_none());
        assert!(cut_out_image(None, None, 1024).is_none());
        // no opacity map by name (Kratos's beard): its strand-like textures, as the launcher found them
        let beard = v(&["TX_kratos_newbeard01_screenshadowof", "TX_youngkratos_goatee_gen_0n_0123456789ABCDEF", "TX_kratosv2_hair_01_uv2_color_0DC3A62E063ED50B", "TX_kratosv2_hair_01_beard_whitehair_C9F325357CB4CC61", "TX_kratosv2_hair_01_beard_whitehair_41B21A1DE7E466D9"]);
        assert!(coverage_map(&beard).is_none());
        let kept: Vec<&str> = beard.iter().filter(|t| coverage_candidate(t)).map(|t| stem(t)).collect();
        assert_eq!(kept, ["kratos_newbeard01_screenshadowof", "kratosv2_hair_01_beard_whitehair", "kratosv2_hair_01_beard_whitehair"]);
        let brows = v(&["TX_death01_112E6D2408347E77", "TX_dynamicmaterial_m1_535B93377186405F", "TX_kratos00_int9_eyebrow_sand_m_gen", "TX_youngkratos_eyebrow_gen_0n_0FD63BCD32D73D12", "TX_youngkratos_eyebrow_gen_0ao_5319105F847B2440", "TX_youngkratos_eyebrow_gen_0h_5B34F04C682CB60B", "TX_kratos00_acc_m_gen_0g_0123456789ABCDEF"]);
        let kept: Vec<&str> = brows.iter().filter(|t| coverage_candidate(t)).map(|t| stem(t)).collect();
        assert_eq!(kept, ["kratos00_int9_eyebrow_sand_m_gen", "youngkratos_eyebrow_gen_0h"]);
        // the most mask-like wins: a grey shadow map (no empty pixels) loses to strands on an empty ground
        let img = |f: &dyn Fn(u32) -> [u8; 4]| Image { w: 10, h: 10, rgba: (0..100).flat_map(f).collect(), format: Format::Bc7 };
        let shadow = img(&|i| [90 + (i % 50) as u8, 90, 90, 255]);
        let strands = img(&|i| if i % 4 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] });
        let solid = img(&|_| [250, 250, 250, 255]);
        let seen = vec![("kratos_newbeard01_screenshadowof".to_string(), shadow), ("kratosv2_hair_01_beard_whitehair".to_string(), strands), ("kratosv2_hair_01_beard_whitehair".to_string(), solid)];
        let (best, line) = best_coverage(&seen);
        assert_eq!(best, Some(1), "{line}");
        assert!(line.contains("75 % empty, 25 % solid"), "{line}");
        assert_eq!(best_coverage(&seen[2..]).0, None);
    }

    #[test]
    fn any_models_table_is_read_and_skipped_textures_are_passed_over() {
        // a mesh, its table of one material, the material and its references: an ice layer's map before its own
        let list = |names: &[&str]| {
            let mut b = vec![0u8; 4 + 76 * names.len()];
            b[0..4].copy_from_slice(&(names.len() as u32).to_le_bytes());
            for (i, n) in names.iter().enumerate() {
                let end = (8 + 76 * i + 16 + n.len()).min(b.len());
                b[8 + 76 * i + 16..end].copy_from_slice(&n.as_bytes()[..end - (8 + 76 * i + 16)]);
            }
            b
        };
        let chunks = [("MESH_axe00_0", 1u16, vec![0u8; 16]), ("MAT_15394922D6E57ABC", 60, list(&["MAT_15394922D6E57ABC"])), ("MAT_15394922D6E57ABC", 1, vec![0; 8]), ("TX_axe02_base_ice_gen_0d_0123456789ABCDEF", 60, list(&["TX_axe02_base_ice_gen_0d_0123456789ABCDEF", "TX_axe02_base_metal_gen_0d_0123456789ABCDEF"]))];
        let (mut buf, mut es) = (Vec::new(), Vec::new());
        for (name, id, b) in chunks {
            es.push(Entry { id, version: 0, length: b.len() as u32, name: name.into(), debug_size: 0, temp_size: 0, align: 16, client: 0, server: 0, mem_type: 0, bits: 0, mem_off: 0, file_off: buf.len() as u64, block: 0 });
            buf.extend(b);
        }
        let dir = std::env::temp_dir().join(format!("gowring-looks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // no texture pack and no texture in the WAD: the note names the map that was chosen
        let (_, plain) = model_looks(&buf, &es, &dir, "MESH_axe00_0", &[0], Strands::default(), 64, 64, 1.0, &[], &[]);
        assert!(plain.contains("0: axe02_base_ice_gen_0d failed"), "{plain}");
        let (_, skipped) = model_looks(&buf, &es, &dir, "MESH_axe00_0", &[0], Strands::default(), 64, 64, 1.0, &["ice"], &[]);
        assert!(skipped.contains("0: axe02_base_metal_gen_0d failed"), "{skipped}");
        assert_eq!(model_looks(&buf, &es, &dir, "MESH_kratosspear00_0", &[0], Strands::default(), 64, 64, 1.0, &[], &[]).1, "no MESH_kratosspear00_0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_baked_colour_map_comes_first_then_the_painted_one_then_a_plain_one() {
        let v = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let acc = v(&["TX_dynamicmaterial_m1_535B93377186405F", "TX_kratos00_int9_acc_sand_m_gen_0n_4474E39761581525", "TX_kratos00_int9_acc_m_gen_0d_111E00F602CC3843", "TX_fabric07_d_BA8EC0E140118F71"]);
        assert_eq!(colour_map(&acc).unwrap(), "TX_kratos00_int9_acc_m_gen_0d_111E00F602CC3843");
        let head = v(&["TX_kratos00_wrinklemap_maxregions_1", "TX_kratos00_int9_head_m_paint_cine_", "TX_kratos00_int9_head_sand_m_gen_0n_ABCDEF0123456789", "TX_kratosashsmall_d_0123456789ABCDEF"]);
        assert_eq!(colour_map(&head).unwrap(), "TX_kratos00_int9_head_m_paint_cine_");
        // a material's own metal mask, not the shared tiling maps
        let legs = v(&["TX_metal01_ao_5947030F9951E009", "TX_metal01_n_4198919D2DD536E2", "TX_kratos00_int9_legs_m2_metals_0123456789ABCDEF", "TX_kratos00_int9_legs_m3_cloth_0123456789ABCDEF"]);
        assert_eq!(metal_mask(&legs).unwrap(), "TX_kratos00_int9_legs_m2_metals_0123456789ABCDEF");
        assert_eq!(metal_mask(&v(&["TX_youngkratos_boots_m1_metal_0123456789ABCDEF"])).unwrap(), "TX_youngkratos_boots_m1_metal_0123456789ABCDEF");
        assert!(metal_mask(&v(&["TX_metal01b_flt_0g_63C0E0D7082B66F2", "TX_kratos00_int9_cloakfur_m_gen_0d_B5F3A2AB7A2F0D04"])).is_none());
        let crystal = v(&["TX_bifrost00_m_gen_2n_0123456789ABCDEF", "TX_bifrost00_m_gen_2d_0123456789ABCDEF"]);
        assert_eq!(colour_map(&crystal).unwrap(), "TX_bifrost00_m_gen_2d_0123456789ABCDEF");
        // names the game cut short: the first of the set, not the tiling wood listed after it
        let handle = v(&["TX_death01_112E6D2408347E77", "TX_kratosspear00_set_handle_e_gen_0_749EB72789E3C418", "TX_kratosspear00_set_handle_e_gen_0_336A801EEA061E86", "TX_wood01_d_1205A98C557129F9"]);
        assert_eq!(colour_map(&handle).unwrap(), "TX_kratosspear00_set_handle_e_gen_0_749EB72789E3C418");
        let eye = v(&["TX_death01_112E6D2408347E77", "TX_kratos00_int9_eye_d_0123456789ABCDEF"]);
        assert_eq!(colour_map(&eye).unwrap(), "TX_kratos00_int9_eye_d_0123456789ABCDEF");
        assert!(colour_map(&v(&["TX_death01_112E6D2408347E77", "TX_standardeye00_int9_tearline_n_0123456789ABCDEF"])).is_none());
    }

    #[test]
    fn the_black_dye_darkens_leather_and_keeps_the_metal_its_mask_marks() {
        let v = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let img = |px: &[[u8; 4]]| Image { w: px.len() as u32, h: 1, rgba: px.concat(), format: crate::dds::Format::Rgba8 };
        // four pixels of brown leather; the mask marks the last as metal
        let mut leather = img(&[[120, 80, 40, 255]; 4]);
        let mask = img(&[[0, 0, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255], [255, 255, 255, 255]]);
        let kept = blacken(&mut leather, Some(&mask));
        assert_eq!(kept, 0.25);
        let dark = (6.0f32 + 0.28 * (0.299 * 120.0 + 0.587 * 80.0 + 0.114 * 40.0)).round() as u8;
        assert_eq!(&leather.rgba[..4], &[dark, dark, dark, 255]);
        assert_eq!(&leather.rgba[12..], &[120, 80, 40, 255]);
        // a mask lit over most of the map marks its unlit side as the metal
        let mut flipped = img(&[[120, 80, 40, 255]; 4]);
        let wide = img(&[[255, 0, 0, 255], [255, 0, 0, 255], [255, 0, 0, 255], [0, 0, 0, 255]]);
        assert_eq!(blacken(&mut flipped, Some(&wide)), 0.25);
        assert_eq!(&flipped.rgba[12..], &[120, 80, 40, 255]);
        // no mask: all of it
        let mut fur = img(&[[90, 70, 50, 255]]);
        assert_eq!(blacken(&mut fur, None), 0.0);
        assert!(fur.rgba[0] < 40 && fur.rgba[0] == fur.rgba[1]);
        // the lacing goes gold at its own brightness, the metal stays, the rest goes black
        let mut laced = img(&[[170, 170, 170, 255], [120, 80, 40, 255], [170, 170, 170, 255]]);
        let stitches = img(&[[255, 0, 0, 255], [0, 0, 0, 255], [255, 0, 0, 255]]);
        let metal = img(&[[0, 0, 0, 255], [0, 0, 0, 255], [255, 0, 0, 255]]);
        assert_eq!(blacken_laced(&mut laced, Some(&metal), Some(&stitches)), (1.0 / 3.0, 1.0 / 3.0));
        assert_eq!(&laced.rgba[..4], &[225, 170, 60, 255]);
        assert_eq!(&laced.rgba[4..7], &[dark; 3]);
        assert_eq!(&laced.rgba[8..12], &[170, 170, 170, 255]);
        let refs = v(&["TX_kratos00_int9_waist_m2_metals_D95589B9CC7975E6", "TX_kratos00_int9_waist_m5_stitches_E520609B02B3CF1B"]);
        assert_eq!(stitch_mask(&refs).unwrap(), "TX_kratos00_int9_waist_m5_stitches_E520609B02B3CF1B");
        assert_eq!(stitch_mask(&v(&["TX_kratos00_int9_shoulder_m5_stitch_B75719B642551478"])).unwrap(), "TX_kratos00_int9_shoulder_m5_stitch_B75719B642551478");
        assert!(stitch_mask(&v(&["TX_leathersm_nm_EB42D11A4169A615"])).is_none());
    }
}

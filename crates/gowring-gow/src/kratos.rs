//! Kratos's skeleton and animation data from the player's own Ragnarök install, gathered for study (roadmap R3:
//! Kratos's own animations in Elden Ring). Nothing here is used in game: the export writes what it finds into a
//! folder next to the diagnose report so the formats can be worked out from real files.
//!
//! Format facts used (from public notes on the PC files; no code taken from any tool):
//!  * `goProto<name>` (WTOC id 1, server 1) holds the skeleton: a u16 joint count at +0x10, then from +0x18 one
//!    record per joint ending in an i16 parent index. The notes disagree on the record size (8 or 10 bytes), so both
//!    readings are checked against the rule that a parent comes before its child.
//!  * Streamed animation sets live in a `.as` file next to the WAD: "anmFileSet" blobs on 64 KiB boundaries, each with
//!    a 0x98-byte header: +0x38 stream flags (3 in the `.as`, 5 in the WAD's stub copy), +0x40 / +0x44 offset and size
//!    in the `.as`, +0x48 a 64-bit name hash, +0x50 the name (56 bytes), +0x90 a type hash, +0x94 an id (u16), +0x96
//!    the file count (u16), then one u64 offset per file. The stubs are WAD chunks on server 2.

use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Where streamed animation sets start in a `.as` file.
pub const SET_ALIGN: usize = 64 * 1024;
const SET_HEADER: usize = 0x98;

#[derive(Clone, Debug, PartialEq)]
pub struct AnimSet {
    pub flags: u32,
    pub off: u32,
    pub size: u32,
    pub hash: u64,
    pub name: String,
    pub type_hash: u32,
    pub id: u16,
    pub files: u16,
    /// The first file offsets (as many as the bytes given hold, at most 64).
    pub file_offs: Vec<u64>,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    Some(i16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// An animation-set header at the start of `b`, when its name reads as a name.
pub fn parse_set(b: &[u8]) -> Option<AnimSet> {
    if b.len() < SET_HEADER {
        return None;
    }
    let raw = &b[0x50..0x88];
    let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    let name = &raw[..end];
    if name.len() < 2 || !name.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-' || *c == b'.') {
        return None;
    }
    let files = u16_at(b, 0x96)?;
    let file_offs = (0..files.min(64) as usize).map_while(|i| u64_at(b, SET_HEADER + i * 8)).collect();
    Some(AnimSet {
        flags: u32_at(b, 0x38)?,
        off: u32_at(b, 0x40)?,
        size: u32_at(b, 0x44)?,
        hash: u64_at(b, 0x48)?,
        name: String::from_utf8_lossy(name).into_owned(),
        type_hash: u32_at(b, 0x90)?,
        id: u16_at(b, 0x94)?,
        files,
        file_offs,
    })
}

/// Every set header found on a 64 KiB boundary of a `.as` file's bytes, with where it starts.
pub fn scan_sets(b: &[u8]) -> Vec<(usize, AnimSet)> {
    (0..b.len()).step_by(SET_ALIGN).filter_map(|o| parse_set(&b[o..]).map(|s| (o, s))).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub parent: i16,
    /// The record's bytes before the parent index (flags and fields of unknown use).
    pub fields: Vec<u16>,
}

/// The joint table of a `goProto` chunk read with records of `stride` bytes (parent last), and how many joints obey
/// "a parent comes before its child" (the root's parent is -1).
pub fn joints(b: &[u8], stride: usize) -> Option<(Vec<Joint>, usize)> {
    let n = u16_at(b, 0x10)? as usize;
    if n == 0 || n > 4096 || stride < 4 {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    let mut good = 0;
    for i in 0..n {
        let o = 0x18 + i * stride;
        let parent = i16_at(b, o + stride - 2)?;
        let fields = (0..(stride - 2) / 2).filter_map(|k| u16_at(b, o + 2 * k)).collect();
        if (parent == -1 && i == 0) || (parent >= 0 && (parent as usize) < i) {
            good += 1;
        }
        out.push(Joint { parent, fields });
    }
    Some((out, good))
}

fn safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Study Kratos's WAD bytes `wad` (decompressed) and his `.as` files `sets` ((name, bytes) each), writing into `out`
/// (`focus`: the character's name in chunk names, e.g. `heroa00`, whose skeleton is saved first):
/// `entries.tsv` (the whole chunk table), `goProto*.bin` and `skeleton.json`, the animation chunks under `chunks/`
/// (at most 48 MB) and the smallest streamed sets under `as_sets/`. Returns the report's text.
pub fn export_from(wad: &[u8], sets: &[(String, Vec<u8>)], focus: &str, out: &Path, mut more: impl FnMut() -> bool) -> String {
    let mut o = String::new();
    let _ = std::fs::create_dir_all(out);
    let Some(es) = crate::wtoc::read(wad) else {
        return "Kratos's WAD: no readable chunk table\n".into();
    };
    // the whole table, for working out where things are
    let mut tsv = String::from("index\tname\tid\tversion\tclient\tserver\tarena\tlength\talign\tblock\tfile_off\n");
    for (i, e) in es.iter().enumerate() {
        let _ = writeln!(tsv, "{i}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", e.name, e.id, e.version, e.client, e.server, e.mem_type, e.length, e.align, e.block, e.file_off);
    }
    let _ = std::fs::write(out.join("entries.tsv"), tsv);
    let bytes_of = |e: &crate::wtoc::Entry| wad.get(e.file_off as usize..(e.file_off + e.length as u64) as usize);
    let _ = writeln!(o, "chunk table: {} entries -> entries.tsv ({} decompressed bytes read)", es.len(), wad.len());

    // the skeleton: the character's own first (a WAD also holds props with one-joint skeletons), then a few others
    let mut protos: Vec<&crate::wtoc::Entry> = es.iter().filter(|e| e.name.starts_with("goProto") && e.id == 1).collect();
    protos.sort_by_key(|e| (!e.name.to_ascii_lowercase().contains(&focus.to_ascii_lowercase()), std::cmp::Reverse(e.length)));
    for (k, e) in protos.iter().take(4).enumerate() {
        let Some(b) = bytes_of(e) else {
            let _ = writeln!(o, "{}: beyond the bytes read (at {:#x})", e.name, e.file_off);
            continue;
        };
        let _ = std::fs::write(out.join(format!("{}.bin", safe(&e.name))), b);
        let (r8, r10) = (joints(b, 8), joints(b, 10));
        let fit = |r: &Option<(Vec<Joint>, usize)>| r.as_ref().map(|(j, g)| (*g, j.len())).unwrap_or((0, 0));
        let _ = writeln!(o, "{} ({} bytes -> {}.bin): {} joints; parent-before-child holds for {}/{} with 8-byte records, {}/{} with 10-byte records", e.name, b.len(), safe(&e.name), u16_at(b, 0x10).unwrap_or(0), fit(&r8).0, fit(&r8).1, fit(&r10).0, fit(&r10).1);
        let best = if fit(&r10).0 > fit(&r8).0 { (10, r10) } else { (8, r8) };
        if let (stride, Some((js, _))) = best {
            let mut j = format!("{{\n  \"source\": \"{}\",\n  \"record_bytes\": {stride},\n  \"joints\": [\n", e.name);
            for (i, jt) in js.iter().enumerate() {
                let _ = write!(j, "    {{\"index\": {i}, \"parent\": {}, \"fields\": {:?}}}{}\n", jt.parent, jt.fields, if i + 1 < js.len() { "," } else { "" });
            }
            j.push_str("  ]\n}\n");
            let _ = std::fs::write(out.join(if k == 0 { "skeleton.json".to_string() } else { format!("skeleton_{}.json", safe(&e.name)) }), j);
            let roots = js.iter().filter(|j| j.parent < 0).count();
            let _ = writeln!(o, "  skeleton.json written with {stride}-byte records: {} joints, {roots} root(s); first joints: {:?}", js.len(), js.iter().take(8).map(|j| j.parent).collect::<Vec<_>>());
        }
        let _ = writeln!(o, "  first 256 bytes: {}", hex(&b[..b.len().min(256)]));
    }

    // animation chunks: ANM_* and everything on server 2 (the streamed sets' stubs)
    let anim: Vec<&crate::wtoc::Entry> = es.iter().filter(|e| e.name.starts_with("ANM_") || e.server == 2).collect();
    let _ = writeln!(o, "animation chunks: {} (ANM_ named: {}, on server 2: {})", anim.len(), anim.iter().filter(|e| e.name.starts_with("ANM_")).count(), anim.iter().filter(|e| e.server == 2).count());
    let _ = std::fs::create_dir_all(out.join("chunks"));
    let mut saved = 0usize;
    for (k, e) in anim.iter().enumerate() {
        let b = bytes_of(e);
        let stub = b.and_then(parse_set);
        if k < 200 {
            let head = b.map(|b| hex(&b[..b.len().min(64)])).unwrap_or_else(|| "(beyond the bytes read)".into());
            let st = stub.as_ref().map(|s| format!(" set `{}` flags {} in .as at {:#x} size {:#x}, {} files, id {}", s.name, s.flags, s.off, s.size, s.files, s.id)).unwrap_or_default();
            let _ = writeln!(o, "  {:<48} id={} server={} arena={} len={:#x} at {:#x}{st}\n    {head}", e.name, e.id, e.server, e.mem_type, e.length, e.file_off);
        }
        if let Some(b) = b {
            if saved + b.len() <= 48 << 20 && more() {
                let _ = std::fs::write(out.join("chunks").join(format!("{k:04}_{}_id{}_s{}.bin", safe(&e.name), e.id, e.server)), b);
                saved += b.len();
            }
        }
    }
    let _ = writeln!(o, "  saved {saved} bytes of animation chunks under chunks/");

    // streamed sets
    let _ = std::fs::create_dir_all(out.join("as_sets"));
    for (file, b) in sets {
        let found = scan_sets(b);
        let _ = writeln!(o, "{file}: {} bytes read, {} set headers on 64 KiB boundaries", b.len(), found.len());
        for (at, s) in found.iter().take(300) {
            let _ = writeln!(o, "  at {at:#x}: `{}` id {} flags {} files {} size {:#x} first offsets {:?}", s.name, s.id, s.flags, s.files, s.size, &s.file_offs[..s.file_offs.len().min(6)]);
        }
        // the smallest sets whole (each up to 2 MB), for working out the encoding
        let mut by_size: Vec<&(usize, AnimSet)> = found.iter().filter(|(_, s)| s.size > 0 && (s.size as usize) <= 2 << 20).collect();
        by_size.sort_by_key(|(_, s)| s.size);
        for (at, s) in by_size.iter().take(8) {
            if !more() {
                break;
            }
            if let Some(blob) = b.get(*at..at + s.size as usize).or_else(|| b.get(*at..(at + SET_HEADER.max(s.size as usize)).min(b.len()))) {
                let _ = std::fs::write(out.join("as_sets").join(format!("{}_{}.bin", safe(file), safe(&s.name))), blob);
            }
        }
        if found.is_empty() {
            let _ = writeln!(o, "  first 256 bytes: {}", hex(&b[..b.len().min(256)]));
        }
    }
    o
}

/// The `.as` files next to `wad` that belong to it (same base name), largest last.
pub fn sibling_sets(wad: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (wad.parent(), wad.file_stem().and_then(|s| s.to_str())) else { return Vec::new() };
    let stem = stem.to_ascii_lowercase();
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("as")) && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.to_ascii_lowercase().starts_with(&stem)))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));
    v
}

/// The bytes of a `.as` file: decompressed when it is an LZ4 frame, otherwise read as is (at most `max` bytes).
pub fn read_set_file(path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    if crate::lz4::is_frame(path) {
        return crate::lz4::read_file_prefix(path, max).map(|(b, _)| b).map_err(|e| std::io::Error::other(format!("{e:#}")));
    }
    use std::io::Read;
    let mut b = Vec::new();
    std::fs::File::open(path)?.take(max as u64).read_to_end(&mut b)?;
    Ok(b)
}

/// Every `.as` file in `dir` with its size, and the names that look like Kratos's or his weapons'.
pub fn set_inventory(dir: &Path) -> String {
    let mut o = String::new();
    let mut all: Vec<(String, u64)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| {
            let p = e.path();
            p.extension().and_then(|x| x.to_str()).filter(|x| x.eq_ignore_ascii_case("as"))?;
            Some((p.file_name()?.to_string_lossy().into_owned(), e.metadata().ok()?.len()))
        }).collect())
        .unwrap_or_default();
    all.sort();
    let total: u64 = all.iter().map(|(_, s)| s).sum();
    let _ = writeln!(o, ".as files in {}: {} ({:.1} MB)", dir.display(), all.len(), total as f64 / 1e6);
    let words = ["hero", "kratos", "axe", "blade", "spear", "draupnir", "shield", "add_"];
    for (n, s) in all.iter().filter(|(n, _)| words.iter().any(|w| n.to_ascii_lowercase().contains(w))).take(60) {
        let _ = writeln!(o, "  {n:<48} {s}");
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_header(name: &str, flags: u32, off: u32, size: u32, id: u16, offs: &[u64]) -> Vec<u8> {
        let mut b = vec![0u8; SET_HEADER];
        b[0x38..0x3C].copy_from_slice(&flags.to_le_bytes());
        b[0x40..0x44].copy_from_slice(&off.to_le_bytes());
        b[0x44..0x48].copy_from_slice(&size.to_le_bytes());
        b[0x48..0x50].copy_from_slice(&0x1122334455667788u64.to_le_bytes());
        b[0x50..0x50 + name.len()].copy_from_slice(name.as_bytes());
        b[0x94..0x96].copy_from_slice(&id.to_le_bytes());
        b[0x96..0x98].copy_from_slice(&(offs.len() as u16).to_le_bytes());
        for f in offs {
            b.extend_from_slice(&f.to_le_bytes());
        }
        b
    }

    #[test]
    fn set_headers_are_read_and_found_on_their_boundaries() {
        let mut file = set_header("navcombatidle", 3, 0, 0x200, 7, &[0x100, 0x180]);
        file.resize(SET_ALIGN, 0);
        file.extend(set_header("axe_throw_01", 3, SET_ALIGN as u32, 0x300, 8, &[0x100]));
        file.resize(SET_ALIGN * 2 + 16, 0);
        let found = scan_sets(&file);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].0, found[0].1.name.as_str(), found[0].1.files, found[0].1.file_offs.clone()), (0, "navcombatidle", 2, vec![0x100, 0x180]));
        assert_eq!((found[1].0, found[1].1.id, found[1].1.size), (SET_ALIGN, 8, 0x300));
        assert!(parse_set(&[0u8; SET_HEADER]).is_none(), "an empty name is not a set");
    }

    #[test]
    fn the_record_size_that_keeps_parents_before_children_wins() {
        // five joints, 10-byte records: root, then a chain
        let mut b = vec![0u8; 0x18];
        b[0x10..0x12].copy_from_slice(&5u16.to_le_bytes());
        for (i, p) in [-1i16, 0, 1, 1, 3].iter().enumerate() {
            let mut r = vec![0u8; 10];
            r[0..2].copy_from_slice(&(i as u16 * 3).to_le_bytes());
            r[8..10].copy_from_slice(&p.to_le_bytes());
            b.extend(r);
        }
        let (j10, good10) = joints(&b, 10).unwrap();
        let (_, good8) = joints(&b, 8).unwrap();
        assert_eq!(good10, 5);
        assert!(good8 < 5);
        assert_eq!(j10.iter().map(|j| j.parent).collect::<Vec<_>>(), vec![-1, 0, 1, 1, 3]);
        assert_eq!(j10[2].fields.len(), 4);
    }

    #[test]
    fn a_wad_and_a_set_file_are_written_out_for_study() {
        use crate::wtoc::{ENTRY_LEN, HEADER_LEN, MAGIC};
        let mut proto = vec![0u8; 0x18];
        proto[0x10..0x12].copy_from_slice(&2u16.to_le_bytes());
        proto.extend([0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]); // 8-byte records: -1, 0
        let stub = set_header("heroa00_locomotion", 5, 0, SET_ALIGN as u32, 1, &[]);
        let entry = |id: u16, name: &str, server: u8, len: usize, bits: u8| {
            let mut e = vec![0u8; ENTRY_LEN];
            e[0..2].copy_from_slice(&id.to_le_bytes());
            e[4..8].copy_from_slice(&(len as u32).to_le_bytes());
            e[0x18..0x18 + name.len()].copy_from_slice(name.as_bytes());
            e[0x6D] = server;
            e[0x72] = bits;
            e
        };
        let mut wad = vec![0u8; HEADER_LEN];
        wad[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        wad[4..8].copy_from_slice(&2u32.to_le_bytes());
        wad[8..12].copy_from_slice(&2u32.to_le_bytes());
        wad.extend(entry(1, "goProtoheroa00", 1, proto.len(), 0));
        let mut e2 = entry(7, "ANM_heroa00_locomotion", 2, stub.len(), 1);
        e2[0x78..0x80].copy_from_slice(&(proto.len() as u64).to_le_bytes());
        wad.extend(e2);
        wad.extend(&proto);
        wad.extend(&stub);
        let mut set_file = set_header("heroa00_locomotion", 3, 0, 0x400, 1, &[0x100]);
        set_file.resize(0x400, 0xAB);
        let dir = std::env::temp_dir().join(format!("gowring-kratos-{}", std::process::id()));
        let report = export_from(&wad, &[("r_heroa00.as".into(), set_file)], "heroa00", &dir, || true);
        assert!(report.contains("2 joints; parent-before-child holds for 2/2 with 8-byte records"), "{report}");
        assert!(report.contains("set `heroa00_locomotion` flags 5"), "{report}");
        assert!(report.contains("`heroa00_locomotion` id 1 flags 3 files 1"), "{report}");
        assert!(std::fs::read_to_string(dir.join("entries.tsv")).unwrap().contains("ANM_heroa00_locomotion"));
        assert!(std::fs::read_to_string(dir.join("skeleton.json")).unwrap().contains("\"parent\": -1"));
        assert_eq!(std::fs::read(dir.join("goProtoheroa00.bin")).unwrap(), proto);
        assert_eq!(std::fs::read(dir.join("as_sets").join("r_heroa00_as_heroa00_locomotion.bin")).unwrap().len(), 0x400);
        assert_eq!(std::fs::read_dir(dir.join("chunks")).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

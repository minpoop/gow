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

    // the character's model: mesh group, GPU buffers, mesh, model and game-object chunks named for it, and every
    // material (they are named by hash, so which are its own is worked out from the mesh)
    let _ = std::fs::create_dir_all(out.join("model"));
    let lc = focus.to_ascii_lowercase();
    let mut model_bytes = 0usize;
    for (k, e) in es.iter().enumerate() {
        let name = e.name.to_ascii_lowercase();
        let own = name.contains(&lc) && ["mg_", "mesh_", "mdl_", "go"].iter().any(|p| name.starts_with(p)) && !name.starts_with("goproto");
        if !(own || name.starts_with("mat_")) {
            continue;
        }
        if let Some(b) = bytes_of(e) {
            let sub = if own { "model" } else { "model/mat" };
            let _ = std::fs::create_dir_all(out.join(sub));
            let _ = std::fs::write(out.join(sub).join(format!("{k:05}_{}_id{}.bin", safe(&e.name), e.id)), b);
            model_bytes += b.len();
            if own {
                let _ = writeln!(o, "model chunk {k} {} id={} server={} len={:#x}: {}", e.name, e.id, e.server, e.length, hex(&b[..b.len().min(64)]));
            }
        }
    }
    let _ = writeln!(o, "  saved {model_bytes} bytes of model and material chunks under model/");

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

/// A character skeleton read from its `goProto` chunk (layout worked out from Kratos's `goProtoheroa00`, 1458 joints):
/// a u16 joint count at +0x10; from +0x18 one 8-byte record per joint (flags, first child, next sibling, parent: i16,
/// -1 for none); then a u64 name hash per joint, then 16 bytes per joint; then a table header H (u64 0, the count
/// twice, a u64 offset R): the inverse bind matrices start at H + 8 + R and the joints' rest transforms (relative to
/// their parent) fill the `count` x 64 bytes just before them. Matrices are 4x4 floats stored column by column (the
/// translation in the last 16 bytes), Y up, in metres.
#[derive(Clone, Debug)]
pub struct Skeleton {
    pub parent: Vec<i16>,
    pub flags: Vec<u16>,
    pub name_hash: Vec<u64>,
    /// Rest transform relative to the parent, and inverse bind matrix, each as 16 floats column by column.
    pub local: Vec<[f32; 16]>,
    pub inverse_bind: Vec<[f32; 16]>,
}

impl Skeleton {
    pub fn parse(b: &[u8]) -> Option<Skeleton> {
        let n = u16_at(b, 0x10)? as usize;
        if n == 0 || n > 8192 {
            return None;
        }
        let mut parent = Vec::with_capacity(n);
        let mut flags = Vec::with_capacity(n);
        for i in 0..n {
            let o = 0x18 + i * 8;
            flags.push(u16_at(b, o)?);
            let p = i16_at(b, o + 6)?;
            // every parent comes before its child
            if !((p == -1 && i == 0) || (p >= 0 && (p as usize) < i)) {
                return None;
            }
            parent.push(p);
        }
        let hashes = 0x18 + n * 8;
        let name_hash = (0..n).map(|i| u64_at(b, hashes + i * 8)).collect::<Option<Vec<_>>>()?;
        let head = hashes + n * 8 + n * 16;
        if u32_at(b, head + 8)? as usize != n {
            return None;
        }
        let ib = head + 8 + u64_at(b, head + 0x10)? as usize;
        let local_at = ib.checked_sub(n * 64)?;
        if local_at < head + 0x18 || ib + n * 64 > b.len() {
            return None;
        }
        let mat = |at: usize| -> [f32; 16] { std::array::from_fn(|k| f32::from_le_bytes(b[at + 4 * k..at + 4 * k + 4].try_into().unwrap())) };
        Some(Skeleton {
            parent,
            flags,
            name_hash,
            local: (0..n).map(|i| mat(local_at + i * 64)).collect(),
            inverse_bind: (0..n).map(|i| mat(ib + i * 64)).collect(),
        })
    }

    /// Each joint's rest position in model space, from the rest transforms composed down the hierarchy.
    pub fn rest_positions(&self) -> Vec<[f32; 3]> {
        let mul = |a: &[f32; 16], c: &[f32; 16]| -> [f32; 16] { std::array::from_fn(|k| (0..4).map(|j| a[j * 4 + k % 4] * c[(k / 4) * 4 + j]).sum()) };
        let mut world: Vec<[f32; 16]> = Vec::with_capacity(self.local.len());
        for (i, l) in self.local.iter().enumerate() {
            let w = if self.parent[i] < 0 { *l } else { mul(&world[self.parent[i] as usize], l) };
            world.push(w);
        }
        world.iter().map(|w| [w[12], w[13], w[14]]).collect()
    }
}

/// One draw of a character mesh, from its `MESH_<name>` chunk (layout worked out from Kratos's `MESH_heroa00_0`): a
/// u32 record count at +0x10 and from +0x40 one self-relative u32 offset per record. A record holds a level-of-detail
/// distance (+0x00), a bounding radius (+0x0C), half extents (+0x10) and centre (+0x1C), then from +0x30: vertex
/// bytes, index offset (+0x38), vertex offset (+0x40), vertex count, triangle count, a format code, the record's
/// index, index count (+0x5C); at +0x80 the stream count (+0x84); at the offset in +0x60 one 8-byte entry per stream
/// (semantic, format, components, byte offset within a vertex, u32 buffer slot) and at the offset in +0x64 each
/// slot's start; +0x68 the hash of the resource holding the draw (0 when it is in the WAD). Most draws give each
/// stream its own slot (offset 0, stored one after another); a few (Kratos's skin: torso, head, arms) put every stream
/// in slot 0 at its own offset, interleaved, with a stride where the last of them ends (76 or 80 bytes). A draw's bytes
/// sit in a resource: the WAD's `MG_<name>_gpu` for the coarsest level of a part, a level-of-detail pack's resource
/// (see [`crate::lodpack`]) for the others; one resource can hold several draws back to back. In its resource, the
/// stream starts are offsets from the resource's start, and the u16 indices start at the vertex-bytes field (+0x30),
/// so for a draw alone in its resource that field is its vertex bytes. The index and vertex offsets (+0x38, +0x40)
/// address a whole-character buffer that is never stored as such. Stream formats (bytes per component): 0 f32, 1 16-bit, 2 32-bit,
/// 3 packed 10:10:10:2 (one per vertex), 4 u16, 6 unorm16, 7 snorm16 (UVs read best that way), 8 u8. Semantics: 0 position, 1 normal, 2 tangent,
/// 3/4/5 UV sets, 9 bone indices (global joint numbers), 10 bone weights.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshRecord {
    pub lod_distance: f32,
    pub centre: [f32; 3],
    pub half_extent: [f32; 3],
    pub vertex_bytes: u32,
    pub index_offset: u64,
    pub vertex_offset: u64,
    pub vertices: u32,
    pub triangles: u32,
    pub format: u32,
    pub indices: u32,
    /// (semantic, format, components, where its first vertex's value starts in the resource) for each stream.
    pub streams: Vec<(u8, u8, u8, u32)>,
    /// Bytes from one vertex's value to the next's, for each stream.
    pub strides: Vec<u32>,
    /// The resource holding the draw in a level-of-detail pack; 0 when it is in the WAD's GPU chunk.
    pub hash: u64,
    /// Index into the mesh's material table (+0x28).
    pub material: u16,
}

/// Bytes of one vertex's value in a stream of `format` with `components`.
fn element_bytes(format: u8, components: u8) -> u32 {
    let c = components as u32;
    match format {
        0 | 2 => 4 * c,
        3 => 4,
        8 => c,
        _ => 2 * c,
    }
}

/// The draws in a `MESH_<name>` chunk.
pub fn mesh_records(b: &[u8]) -> Vec<MeshRecord> {
    let n = u32_at(b, 0x10).unwrap_or(0) as usize;
    let f32_at = |o: usize| u32_at(b, o).map(f32::from_bits);
    let mut out = Vec::new();
    for i in 0..n.min(1 << 16) {
        let at = 0x40 + 4 * i;
        let Some(rel) = u32_at(b, at) else { break };
        let o = at + rel as usize;
        let rec = (|| {
            let streams_n = *b.get(o + 0x84)? as usize;
            let (lay, starts) = (o + u32_at(b, o + 0x60)? as usize, o + u32_at(b, o + 0x64)? as usize);
            // (semantic, format, components, offset in the vertex, slot)
            let layout = (0..streams_n)
                .map(|s| Some((*b.get(lay + 8 * s)?, *b.get(lay + 8 * s + 1)?, *b.get(lay + 8 * s + 2)?, *b.get(lay + 8 * s + 3)? as u32, u32_at(b, lay + 8 * s + 4)?)))
                .collect::<Option<Vec<_>>>()?;
            let stride = |slot: u32| layout.iter().filter(|e| e.4 == slot).map(|e| e.3 + element_bytes(e.1, e.2)).max().unwrap_or(0);
            let streams = layout.iter().map(|e| Some((e.0, e.1, e.2, u32_at(b, starts + 4 * e.4 as usize)? + e.3))).collect::<Option<Vec<_>>>()?;
            let strides = layout.iter().map(|e| stride(e.4)).collect();
            Some(MeshRecord {
                lod_distance: f32_at(o)?,
                centre: [f32_at(o + 0x1C)?, f32_at(o + 0x20)?, f32_at(o + 0x24)?],
                half_extent: [f32_at(o + 0x10)?, f32_at(o + 0x14)?, f32_at(o + 0x18)?],
                vertex_bytes: u32_at(b, o + 0x30)?,
                index_offset: u32_at(b, o + 0x38)? as u64,
                vertex_offset: u32_at(b, o + 0x40)? as u64,
                vertices: u32_at(b, o + 0x44)?,
                triangles: u32_at(b, o + 0x48)?,
                format: u32_at(b, o + 0x4C)?,
                indices: u32_at(b, o + 0x5C)?,
                streams,
                strides,
                hash: u64_at(b, o + 0x68)?,
                material: u16_at(b, o + 0x28)?,
            })
        })();
        match rec {
            Some(r) => out.push(r),
            None => break,
        }
    }
    out
}

impl MeshRecord {
    /// How many of the positions at the start of `bytes` (the record's vertex bytes) fall inside its bounds.
    pub fn positions_inside(&self, bytes: &[u8]) -> usize {
        let slack = 0.005;
        (0..self.vertices as usize)
            .filter(|v| {
                (0..3).all(|c| {
                    let o = v * 12 + c * 4;
                    bytes.get(o..o + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap())).is_some_and(|x| (x - self.centre[c]).abs() <= self.half_extent[c] + slack)
                })
            })
            .count()
    }
}

/// Where in `bytes` a run of `rec.vertices` positions inside the record's bounds starts (4-byte aligned), if anywhere.
pub fn find_positions(bytes: &[u8], rec: &MeshRecord) -> Option<usize> {
    let need = rec.vertices as usize;
    if need == 0 {
        return None;
    }
    let slack = 0.005;
    let inside = |o: usize| (0..3).all(|c| bytes.get(o + 4 * c..o + 4 * c + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap())).is_some_and(|x| (x - rec.centre[c]).abs() <= rec.half_extent[c] + slack));
    // three interleaved runs, one per alignment of a 12-byte position against 4-byte steps
    let mut run = [0usize; 3];
    let mut o = 0;
    while o + 12 <= bytes.len() {
        let lane = (o / 4) % 3;
        if inside(o) {
            run[lane] += 1;
            if run[lane] == need {
                return Some(o + 12 - need * 12);
            }
        } else {
            run[lane] = 0;
        }
        o += 4;
    }
    None
}

/// Kratos's default look, as parts of `MG_heroa00_0` (worked out from the player's install, 0.1.41): head, eyes and
/// beard (0-8, 238-241, 243, 263-265, 269), skin (230: torso, head, arms), trousers, skirt, boots and knee guard (200,
/// 201, 267, 268), the unnamed "tier1" chest strap and pauldron with its fur, strap and cape (213-223, 231, 234, 236),
/// wrapped bracers (203, 204), leather waist and buckled belt (194-198), and belt items (251, 270-274, 277, 279, 280).
/// The other parts are the armour sets the game swaps in (chest, wrist and waist for riverdelta, crystal,
/// Svartalfheim, Muspelheim, defense, runic, Tyr, strength, cooldown, Alfheim and New Game+ variants), alternatives
/// of the unnamed set, and the stand-ins in [`STAND_INS`].
pub const DEFAULT_OUTFIT: &[u16] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 194, 195, 196, 197, 198, 200, 201, 203, 204, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223, 230, 231, 234, 236, 238, 239,
    240, 241, 243, 251, 263, 264, 265, 267, 268, 269, 270, 271, 272, 273, 274, 277, 279, 280,
];

/// The Black Bear Set (Ragnarök's New Game+ update): a black Survival Set, the set Kratos wears at the start of the
/// game, with the bear-fur cloak (235, material 24 `kratos00_int9_cloakfur`, shoulders to knees) and the fur under it
/// (224, material 69 `kratos_underfur`). The set is Ragnarök's own "int9" armour, not the pieces of
/// [`DEFAULT_OUTFIT`] that carry the `youngkratos` waist: the chest harness from the right shoulder across the chest,
/// with its shoulder pad (228, materials 70 `kratos00_int9_shoulder` and 71 `kratos00_int9_chest`), the broad belt
/// with its silver plates and knot buckle (207-209, material 66 `kratos00_int9_waist`) and the riveted leather apron
/// with its fur hem (205, 206, material 21 `kratos00_int9_skirt`), as in the set's art (rendered from the player's
/// outfit probe, 0.1.65). Out with them go the `youngkratos` belt and the red cloth with a gold key pattern that hung
/// in front of it (194-198, materials 60 and 61; 0.1.60-0.1.65 wore them, which left his belly bare from the belt to
/// the chest, "a beer gut"), the leather cloak and the fur along its strap (236, 213, 215-223), and the strap itself (231,
/// under the harness). The harness's fur (229, material 29: tufts around the shoulder pad, front to back) stays, and so
/// does the pad's second panel (227, material 70 as the pad: the laced leather band over the back of the shoulder, laid
/// out on the half of `kratos00_int9_shoulder` the pad leaves empty, so the same pauldron, not an alternative; left out
/// in 0.1.66-0.1.71). His arms are the set's too (0.1.74, "his arms are meant to be armored"): the guards over his
/// upper arms (225, 226) and the bracers (210-212), materials 67 `kratos00_int9_leftarm` and 68 `kratos00_int9_rightarm`,
/// the same family as the pad, harness, belt and apron (0.1.66 judged the guards another set's from a render and kept
/// the default outfit's `youngkratos_armbracer` bracers, 203 and 204). The black is a dye
/// ([`crate::kratos_part::Outfit::dyed`]).
pub const BLACK_BEAR_OUTFIT: &[u16] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 200, 201, 205, 206, 207, 208, 209, 210, 211, 212, 214, 224, 225, 226, 227, 228, 229, 230, 234, 235, 238, 239, 240, 241, 243,
    251, 263, 264, 265, 267, 268, 269, 270, 271, 272, 273, 274, 277, 279, 280,
];

/// Parts that are not drawn as Kratos: a capsule and three ellipsoids per body region (244-258, likely shadow or
/// collision shapes), four ground rings (259-262), coarse shells (67, 237, 266) and two objects at the origin (242, 252).
pub const STAND_INS: &[u16] = &[67, 237, 242, 244, 245, 246, 247, 248, 249, 250, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 266];

/// The names in a mesh's material table (the id-60 `MAT_` chunk beside its `MESH_` chunk): u32 count, then 76-byte
/// entries of a 16-byte id and a 60-byte name. A draw's material is the u16 at record +0x28.
pub fn material_names(b: &[u8]) -> Vec<String> {
    references(b).into_iter().map(|(_, name)| name).collect()
}

/// The entries of an id-60 reference list (a mesh's material table, or a material's list of textures, parent
/// materials and shaders): (16-byte id, name). For textures the id is 8 bytes of tag and then the texture's key.
/// The chunk is 4 + 76 n bytes, so the last entry's name slot runs 4 bytes past its end: the name is read up to the
/// end of the chunk (taking whole slots dropped every list's last entry, among them Kratos's belt material 97).
pub fn references(b: &[u8]) -> Vec<([u8; 16], String)> {
    let n = u32_at(b, 0).unwrap_or(0) as usize;
    (0..n.min(4096))
        .map_while(|i| {
            let at = 8 + 76 * i;
            let id: [u8; 16] = b.get(at..at + 16)?.try_into().ok()?;
            let raw = b.get(at + 16..(at + 76).min(b.len())).filter(|r| !r.is_empty())?;
            let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
            Some((id, String::from_utf8_lossy(&raw[..end]).into_owned()))
        })
        .collect()
}

/// The bytes of the chunk `name` with `id` in a WAD (`buf` holds its decompressed start, `es` its table), when they
/// were read; the first of that name, as the body's lookups take it.
pub fn chunk<'a>(buf: &'a [u8], es: &[crate::wtoc::Entry], name: &str, id: u16) -> Option<&'a [u8]> {
    chunk_at(es, name, id).and_then(|i| entry_bytes(buf, &es[i]))
}

/// Where in the table the chunk `name` with `id` is: the first spelled exactly so, else the first that differs only
/// in case (the skeleton is `goProtokratosShield00` beside `MESH_kratosshield00_0`).
pub fn chunk_at(es: &[crate::wtoc::Entry], name: &str, id: u16) -> Option<usize> {
    es.iter().position(|e| e.name == name && e.id == id).or_else(|| es.iter().position(|e| e.id == id && e.name.eq_ignore_ascii_case(name)))
}

/// A chunk's bytes in the decompressed WAD, when they were read.
pub fn entry_bytes<'a>(buf: &'a [u8], e: &crate::wtoc::Entry) -> Option<&'a [u8]> {
    buf.get(e.file_off as usize..(e.file_off as usize).checked_add(e.length as usize)?)
}

/// A model's chunks in its WAD, as the body's are found for `heroa00` (every model's sit in the same order: GPU
/// chunk, `MESH_<m>_0`, its material table, `MG_<m>_0`, `MDL_<m>`).
#[derive(Clone, Debug)]
pub struct ModelChunks<'a> {
    pub model: String,
    /// `MESH_<m>_0` (id 1): the draws ([`mesh_records`]).
    pub mesh: &'a [u8],
    /// The material table: the id-60 chunk right after the mesh ([`material_names`]). None when the mesh has a
    /// single material (then a zero-length `MAT_` marker sits there instead).
    pub materials: Option<&'a [u8]>,
    /// `MG_<m>_0` (id 1): the parts and their levels of detail ([`mg_parts`]).
    pub mg: &'a [u8],
    /// `MG_<m>_0_gpu` (id 29): the resident (coarsest) draws' vertex data.
    pub gpu: &'a [u8],
    /// `goProto<m>` (id 1, any case): the skeleton ([`Skeleton::parse`]).
    pub proto: Option<&'a [u8]>,
}

/// The chunks of `model` (e.g. `axe00`) in the WAD, or which one is missing from the bytes read.
pub fn model_chunks<'a>(buf: &'a [u8], es: &[crate::wtoc::Entry], model: &str) -> Result<ModelChunks<'a>, String> {
    let need = |name: String, id: u16| chunk(buf, es, &name, id).ok_or(format!("{name} (id {id}) is not in the bytes read"));
    let mesh_name = format!("MESH_{model}_0");
    let mesh = need(mesh_name.clone(), 1)?;
    let materials = chunk_at(es, &mesh_name, 1).and_then(|i| es.get(i + 1)).filter(|e| e.id == 60 && e.name.starts_with("MAT_")).and_then(|e| entry_bytes(buf, e));
    Ok(ModelChunks {
        model: model.to_string(),
        mesh,
        materials,
        mg: need(format!("MG_{model}_0"), 1)?,
        gpu: need(format!("MG_{model}_0_gpu"), 29)?,
        proto: chunk(buf, es, &format!("goProto{model}"), 1),
    })
}

/// The records of the most detailed level of each of `parts` (every part when None), in part order.
pub fn level0_records(mg: &[u8], parts: Option<&[u16]>) -> Vec<usize> {
    let all = mg_parts(mg);
    let pick: Vec<usize> = match parts {
        Some(list) => list.iter().map(|p| *p as usize).collect(),
        None => (0..all.len()).collect(),
    };
    pick.iter().filter_map(|p| all.get(*p)).filter_map(|l| l.first()).flat_map(|l| l.first as usize..l.first as usize + l.draws as usize).collect()
}

/// A material's own reference list (its textures, parent materials and shaders, [`references`]): the id-60 chunk
/// right after the first id-1 chunk of that name (the first copy always carries it). Empty when it is not there.
pub fn material_refs(buf: &[u8], es: &[crate::wtoc::Entry], name: &str) -> Vec<([u8; 16], String)> {
    material_refs_chunk(buf, es, name).map(references).unwrap_or_default()
}

/// The bytes of a material's reference list (see [`material_refs`]).
pub fn material_refs_chunk<'a>(buf: &'a [u8], es: &[crate::wtoc::Entry], name: &str) -> Option<&'a [u8]> {
    let at = es.iter().position(|e| e.name == name && e.id == 1)?;
    es.get(at + 1).filter(|e| e.id == 60).and_then(|e| entry_bytes(buf, e))
}

/// A part of a model's `MG_` chunk with what [`mg_parts`] leaves out: the joint it is attached to (u16 at +0x00) and
/// its kind (+0x03: 2 skinned, 0 rigid, as Kratos's eyes and belt items are).
#[derive(Clone, Debug, PartialEq)]
pub struct PartInfo {
    pub joint: u16,
    pub kind: u8,
    pub levels: Vec<Level>,
}

/// Every part of an `MG_` chunk with its joint, kind and levels (the same parts [`mg_parts`] reads).
pub fn mg_part_info(b: &[u8]) -> Vec<PartInfo> {
    let levels = mg_parts(b);
    levels
        .into_iter()
        .enumerate()
        .map_while(|(p, levels)| {
            let a = u32_at(b, 0x44 + 4 * p)? as usize;
            Some(PartInfo { joint: u16_at(b, a)?, kind: *b.get(a + 3)?, levels })
        })
        .collect()
}

/// One level of detail of a part: `draws` records of the `MESH_` chunk from `first`, shown up to `distance`.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub draws: u16,
    pub distance: f32,
    pub first: u16,
}

/// The parts of a character mesh, from its `MG_<name>` chunk (worked out from Kratos's `MG_heroa00_0`): a u16 part
/// count at +0x30 and from +0x44 one u32 offset per part. A part holds its level count at +2 and from +56 one u32
/// offset (from the part's start) per level; a level is a u32 draw count, the f32 distance it is shown to, and at +10
/// a u16 first record. Levels run from most to least detailed; the last has no draws.
pub fn mg_parts(b: &[u8]) -> Vec<Vec<Level>> {
    let n = u16_at(b, 0x30).unwrap_or(0) as usize;
    (0..n.min(1 << 14))
        .map_while(|p| {
            let a = u32_at(b, 0x44 + 4 * p)? as usize;
            let levels = *b.get(a + 2)? as usize;
            (0..levels)
                .map(|i| {
                    let e = a + u32_at(b, a + 56 + 4 * i)? as usize;
                    Some(Level { draws: u32_at(b, e)?.min(u16::MAX as u32) as u16, distance: f32::from_bits(u32_at(b, e + 4)?), first: u16_at(b, e + 10)? })
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

/// A draw's positions and triangles, from the resource holding it; `None` unless every position is inside the
/// record's bounds and every index names a vertex.
pub fn draw_geometry(res: &[u8], rec: &MeshRecord) -> Option<(Vec<[f32; 3]>, Vec<[u16; 3]>)> {
    let k = rec.streams.iter().position(|s| s.0 == 0)?;
    let (_, fmt, comps, start) = rec.streams[k];
    let stride = rec.strides.get(k).copied().unwrap_or(12).max(12) as usize;
    if fmt != 0 || comps != 3 || rec.indices % 3 != 0 {
        return None;
    }
    let nv = rec.vertices as usize;
    let at = start as usize;
    let pos: Vec<u8> = (0..nv).map(|v| res.get(at + v * stride..at + v * stride + 12)).collect::<Option<Vec<_>>>()?.concat();
    if rec.positions_inside(&pos) != nv {
        return None;
    }
    let p = pos.chunks_exact(12).map(|c| [0, 4, 8].map(|k| f32::from_le_bytes(c[k..k + 4].try_into().unwrap()))).collect();
    let ia = rec.vertex_bytes as usize;
    let idx = res.get(ia..ia + rec.indices as usize * 2)?;
    let t: Vec<[u16; 3]> = idx.chunks_exact(6).map(|c| [0, 2, 4].map(|k| u16::from_le_bytes([c[k], c[k + 1]]))).collect();
    t.iter().flatten().all(|i| (*i as usize) < nv).then_some((p, t))
}

/// A draw's vertices with everything a skinned model needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vertices {
    pub position: Vec<[f32; 3]>,
    pub normal: Vec<[f32; 3]>,
    /// UV sets 0 and 1 (empty when the draw has none).
    pub uv: [Vec<[f32; 2]>; 2],
    /// Up to ten (joint, weight) influences per vertex, largest first, weights summing to 1; empty for a rigid draw.
    pub influences: Vec<Vec<(u16, f32)>>,
    pub triangles: Vec<[u16; 3]>,
}

fn unorm10(x: u32, k: u32) -> f32 {
    ((x >> (10 * k)) & 1023) as f32 / 1023.0
}

/// Every vertex of a draw from the resource holding it (see [`draw_geometry`] for the checks). Normals are packed
/// 10:10:10:2 (unorm to [-1, 1]); UVs are f32, unorm16 or snorm16 pairs; joints are four u16 or u8 global joint numbers,
/// or four u32 words holding seven u16 joints (low half first) or ten 11-bit joints packed from the top of the 128-bit
/// value the words make when read first-word-highest (Kratos's skin; record +0x87 & 3 says 4, 7 or 10); weights are 10:10:10:2 words holding three weights each (one, two or three words, so 3, 6 or
/// 9 stored), largest first, with one more weight making the sum 1.
pub fn draw_vertices(res: &[u8], rec: &MeshRecord) -> Option<Vertices> {
    let (position, triangles) = draw_geometry(res, rec)?;
    let nv = rec.vertices as usize;
    let stream = |sem: u8| rec.streams.iter().position(|s| s.0 == sem).map(|k| (rec.streams[k], rec.strides.get(k).copied().unwrap_or(0) as usize));
    let at = |start: u32, stride: usize, v: usize, len: usize| res.get(start as usize + v * stride..start as usize + v * stride + len);
    let word = |b: &[u8], k: usize| u32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap());
    let normal = match stream(1) {
        Some(((_, 3, _, start), stride)) => (0..nv)
            .map(|v| {
                let x = word(at(start, stride.max(4), v, 4)?, 0);
                Some([0, 1, 2].map(|k| unorm10(x, k) * 2.0 - 1.0))
            })
            .collect::<Option<Vec<_>>>()?,
        _ => vec![],
    };
    let uv_set = |sem: u8| -> Option<Vec<[f32; 2]>> {
        let Some(((_, fmt, 2, start), stride)) = stream(sem) else { return Some(vec![]) };
        (0..nv)
            .map(|v| {
                let b = at(start, stride.max(element_bytes(fmt, 2) as usize), v, element_bytes(fmt, 2) as usize)?;
                Some(match fmt {
                    0 => [f32::from_le_bytes(b[0..4].try_into().unwrap()), f32::from_le_bytes(b[4..8].try_into().unwrap())],
                    6 => [0, 2].map(|k| u16::from_le_bytes([b[k], b[k + 1]]) as f32 / 65535.0),
                    7 => [0, 2].map(|k| (i16::from_le_bytes([b[k], b[k + 1]]) as f32 / 32767.0).max(-1.0)),
                    _ => return None,
                })
            })
            .collect()
    };
    let uv = [uv_set(3)?, uv_set(4)?];
    // rigid draws (eyes, stand-ins) carry no joints: they follow whatever they are attached to
    let (Some(((_, jfmt, jn, jstart), jstride)), Some(((_, wfmt, wn, wstart), wstride))) = (stream(9), stream(10)) else {
        return Some(Vertices { position, normal, uv, influences: vec![], triangles });
    };
    let influences = (0..nv)
        .map(|v| {
            let joints: Vec<u16> = match (jfmt, jn) {
                (4, 4) => at(jstart, jstride.max(8), v, 8)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
                (8, 4) => at(jstart, jstride.max(4), v, 4)?.iter().map(|x| *x as u16).collect(),
                // seven influences (two weight words): u16 pairs, low half first
                (2, 4) if (wfmt, wn) == (2, 2) => at(jstart, jstride.max(16), v, 16)?.chunks_exact(2).take(7).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
                (2, 4) => {
                    let b = at(jstart, jstride.max(16), v, 16)?;
                    let x = (0..4).fold(0u128, |acc, k| (acc << 32) | word(b, k) as u128);
                    (0..10).map(|i| ((x >> (128 - 11 * (i + 1))) & 2047) as u16).collect()
                }
                _ => return None,
            };
            let mut weights: Vec<f32> = match (wfmt, wn) {
                (3, 1) | (2, 1..=3) => {
                    let words = if wfmt == 3 { 1 } else { wn as usize };
                    let b = at(wstart, wstride.max(4 * words), v, 4 * words)?;
                    (0..words).flat_map(|k| [0, 1, 2].map(|j| unorm10(word(b, k), j))).collect()
                }
                _ => return None,
            };
            weights.push((1.0 - weights.iter().sum::<f32>()).max(0.0));
            let mut inf: Vec<(u16, f32)> = joints.into_iter().zip(weights).filter(|(_, w)| *w > 0.0).collect();
            let sum: f32 = inf.iter().map(|x| x.1).sum();
            if sum <= 0.0 {
                return None;
            }
            inf.iter_mut().for_each(|x| x.1 /= sum);
            Some(inf)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Vertices { position, normal, uv, influences, triangles })
}

/// A compact copy of some draws' geometry, for a look at a model away from the player's PC: "GRKM", u32 version 1,
/// u32 draws; per draw u32 record, u32 part, u32 vertices, u32 triangles, f32 centre[3], f32 half extents[3], then
/// i16 positions scaled to the bounds (x, y, z per vertex), then u16 indices.
pub fn compact_geometry(draws: &[(u32, u32, &MeshRecord, &[[f32; 3]], &[[u16; 3]])]) -> Vec<u8> {
    let mut o = b"GRKM".to_vec();
    o.extend(1u32.to_le_bytes());
    o.extend((draws.len() as u32).to_le_bytes());
    for (record, part, r, p, t) in draws {
        for v in [*record, *part, p.len() as u32, t.len() as u32] {
            o.extend(v.to_le_bytes());
        }
        let half = r.half_extent.map(|h| h + 0.01);
        for v in r.centre.iter().chain(half.iter()) {
            o.extend(v.to_le_bytes());
        }
        for v in p.iter() {
            for c in 0..3 {
                o.extend(((((v[c] - r.centre[c]) / half[c]).clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
            }
        }
        for tri in t.iter() {
            for i in tri {
                o.extend(i.to_le_bytes());
            }
        }
    }
    o
}

/// Where `needle` first occurs in the file at `path` (its decompressed bytes when it is an LZ4 frame), read start to
/// end with little memory; `more` is asked between reads and stops the search when false.
pub fn find_bytes_in_file(path: &Path, needle: &[u8], mut more: impl FnMut() -> bool) -> anyhow::Result<Option<u64>> {
    use std::io::Read;
    if needle.is_empty() {
        return Ok(None);
    }
    let file = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let mut lz = if crate::lz4::is_frame(path) { Some(crate::lz4::FrameReader::open(file)?) } else { None };
    let mut raw = if lz.is_none() { Some(std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?)) } else { None };
    let (mut buf, mut base, mut scanned) = (Vec::<u8>::new(), 0u64, 0usize);
    let keep = needle.len().max(64 << 10);
    let first = needle[0];
    loop {
        let got = match (&mut lz, &mut raw) {
            (Some(f), _) => f.next_block(&mut buf)?,
            (_, Some(r)) => {
                let at = buf.len();
                buf.resize(at + (4 << 20), 0);
                let n = r.read(&mut buf[at..])?;
                buf.truncate(at + n);
                n > 0
            }
            _ => false,
        };
        // only what is new since the last pass (less a needle's length, for a match straddling two reads)
        let from = scanned.saturating_sub(needle.len() - 1);
        let mut i = from;
        while i + needle.len() <= buf.len() {
            match buf[i..buf.len() + 1 - needle.len()].iter().position(|b| *b == first) {
                Some(j) => {
                    i += j;
                    if &buf[i..i + needle.len()] == needle {
                        return Ok(Some(base + i as u64));
                    }
                    i += 1;
                }
                None => break,
            }
        }
        scanned = buf.len();
        if !got || !more() {
            return Ok(None);
        }
        // keep the tail (a match can straddle reads, and LZ4 matches reach 64 KiB back)
        if buf.len() > keep + (8 << 20) {
            let cut = buf.len() - keep;
            buf.drain(..cut);
            base += cut as u64;
            scanned -= cut;
        }
    }
}

/// Words in a set's name that mark Kratos's everyday moves: such sets are saved whole for working out how bone motion
/// is stored (the smallest sets are single curves, not bones).
pub const STUDY_WORDS: &[&str] = &["Idle", "attAxe", "attBlade", "attSpear", "attFist", "Rage", "Dodge", "Evade", "Roll", "Block", "Parry", "Walk", "Run", "Turn", "Throw", "Recall", "Hit"];

/// What a walk through a whole `.as` file found.
#[derive(Debug, Default)]
pub struct SetIndex {
    pub lz4: bool,
    /// Bytes walked (decompressed) and whether the end was reached.
    pub bytes: u64,
    pub complete: bool,
    /// Every set header found on a 64 KiB boundary: where it starts, the header, and whether the header's own offset
    /// field names that place.
    pub sets: Vec<(u64, AnimSet, bool)>,
}

/// Header bytes looked at for a set (the header and up to 64 file offsets).
const SET_HEAD_NEED: u64 = (SET_HEADER + 64 * 8) as u64;

/// Walk the `.as` file at `path` from start to end (an LZ4 frame or plain bytes) with little memory, noting every
/// set header; `keep` is asked about each set, and the bytes of those it wants are handed to `save`. `more` is
/// asked between reads and stops the walk when false.
pub fn walk_set_file(path: &Path, mut keep: impl FnMut(&AnimSet) -> bool, mut save: impl FnMut(&AnimSet, &[u8]), mut more: impl FnMut() -> bool) -> anyhow::Result<SetIndex> {
    use std::io::Read;
    let file = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let lz4 = crate::lz4::is_frame(path);
    let mut idx = SetIndex { lz4, ..Default::default() };
    enum Src<R: Read> {
        Lz(crate::lz4::FrameReader<R>),
        Raw(R),
    }
    let mut src = if lz4 { Src::Lz(crate::lz4::FrameReader::open(file)?) } else { Src::Raw(file) };
    let mut fill = |buf: &mut Vec<u8>| -> anyhow::Result<bool> {
        match &mut src {
            Src::Lz(f) => f.next_block(buf),
            Src::Raw(r) => {
                let at = buf.len();
                buf.resize(at + (1 << 20), 0);
                let n = r.read(&mut buf[at..])?;
                buf.truncate(at + n);
                Ok(n > 0)
            }
        }
    };
    let (mut buf, mut base, mut next) = (Vec::<u8>::new(), 0u64, 0u64);
    let mut pending: Option<(u64, u64, AnimSet)> = None;
    let mut eof = false;
    loop {
        loop {
            let end = base + buf.len() as u64;
            if let Some((st, en, _)) = &pending {
                if end < *en && !eof {
                    break;
                }
                let (a, b) = ((st - base) as usize, (en.min(&end) - base) as usize);
                let (_, _, set) = pending.take().unwrap();
                save(&set, &buf[a..b]);
                continue;
            }
            if next >= end || (next + SET_HEAD_NEED > end && !eof) {
                break;
            }
            let at = (next - base) as usize;
            match parse_set(&buf[at..]) {
                Some(s) => {
                    let own = s.off as u64 == next && s.size as usize >= SET_HEADER;
                    if own && keep(&s) {
                        pending = Some((next, next + s.size as u64, s.clone()));
                    }
                    // a header that names its own place says how far its set reaches; anything else steps one boundary
                    let step = if own { (s.size as u64).div_ceil(SET_ALIGN as u64).max(1) * SET_ALIGN as u64 } else { SET_ALIGN as u64 };
                    idx.sets.push((next, s, own));
                    next += step;
                }
                None => next += SET_ALIGN as u64,
            }
        }
        if eof {
            idx.complete = true;
            break;
        }
        if !more() {
            break;
        }
        // keep what is still needed, and 64 KiB behind it for an LZ4 block's matches (a big set can put `next` past
        // what has been read so far: then nothing read is needed; 0.1.38 cut beyond the buffer there and panicked)
        let keep_from = pending.as_ref().map(|p| p.0).unwrap_or(next).min(next).saturating_sub(64 << 10).clamp(base, base + buf.len() as u64);
        let cut = (keep_from - base) as usize;
        if cut >= 8 << 20 {
            buf.drain(..cut);
            base = keep_from;
        }
        if !fill(&mut buf)? {
            eof = true;
        }
        idx.bytes = base + buf.len() as u64;
    }
    Ok(idx)
}

/// The study picks for `walk_set_file`: sets with one of `STUDY_WORDS` in the name, between 16 KiB and 2 MiB, at most
/// `per_word` for each word and `budget` bytes in all. Returns the chooser (call it once per set, in file order).
pub fn study_picker(per_word: usize, budget: usize) -> impl FnMut(&AnimSet) -> bool {
    let mut taken = std::collections::HashMap::<&'static str, usize>::new();
    let mut used = 0usize;
    move |s: &AnimSet| {
        let size = s.size as usize;
        if !(16 << 10..=2 << 20).contains(&size) || used + size > budget {
            return false;
        }
        let Some(w) = STUDY_WORDS.iter().find(|w| s.name.contains(*w)) else { return false };
        let n = taken.entry(w).or_default();
        if *n >= per_word {
            return false;
        }
        *n += 1;
        used += size;
        true
    }
}

/// The index as a table: one row per set header found.
pub fn index_tsv(file: &str, file_len: u64, idx: &SetIndex) -> String {
    let mut o = format!("# {file}\t{file_len} bytes\tlz4={}\twalked={}\tcomplete={}\theaders={}\n", idx.lz4, idx.bytes, idx.complete, idx.sets.len());
    o.push_str("at\town\tname\tid\tflags\tfiles\tsize\thash\ttype_hash\tfirst_file_offsets\n");
    for (at, s, own) in &idx.sets {
        let _ = writeln!(o, "{at:#x}\t{}\t{}\t{}\t{}\t{}\t{:#x}\t{:016x}\t{:08x}\t{:?}", *own as u8, s.name, s.id, s.flags, s.files, s.size, s.hash, s.type_hash, &s.file_offs[..s.file_offs.len().min(8)]);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_lists_keep_their_last_entry_whose_name_slot_runs_past_the_chunk() {
        // u32 count, 4 bytes, then 76-byte entries (16-byte id, 60-byte name); the chunk is 4 + 76 n bytes long
        let names = ["MAT_15394922D6E57ABC", "b9fdf91541174798_ps_20003207"];
        let mut b = vec![0u8; 4 + 76 * names.len()];
        b[0..4].copy_from_slice(&(names.len() as u32).to_le_bytes());
        for (i, n) in names.iter().enumerate() {
            let at = 8 + 76 * i;
            b[at] = i as u8 + 1;
            b[at + 16..at + 16 + n.len()].copy_from_slice(n.as_bytes());
        }
        let refs = references(&b);
        assert_eq!(refs.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), names);
        assert_eq!(refs[1].0[0], 2);
        assert_eq!(material_names(&b), names);
    }

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
        wad[8..12].copy_from_slice(&4u32.to_le_bytes());
        wad.extend(entry(1, "goProtoheroa00", 1, proto.len(), 0));
        let mut e2 = entry(7, "ANM_heroa00_locomotion", 2, stub.len(), 1);
        e2[0x78..0x80].copy_from_slice(&(proto.len() as u64).to_le_bytes());
        wad.extend(e2);
        wad.extend(entry(1, "MESH_heroa00_0", 12, 8, 0));
        wad.extend(entry(1, "MAT_B9FDF91541174798", 12, 8, 0));
        wad.extend(&proto);
        wad.extend(&stub);
        wad.extend([7u8; 16]); // the mesh's and the material's 8 bytes each, in the next block
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
        // the model's own chunks, and every material
        assert!(report.contains("model chunk 2 MESH_heroa00_0"), "{report}");
        assert!(dir.join("model").join("00002_MESH_heroa00_0_id1.bin").exists());
        assert!(dir.join("model").join("mat").join("00003_MAT_B9FDF91541174798_id1.bin").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_whole_set_file_is_walked_plain_or_lz4_and_chosen_sets_are_saved() {
        // three sets: a small curve, an "Idle" clip of 80 KiB (crossing a boundary whose bytes look like nothing),
        // and an attack; then a stray name at a boundary that does not name its place
        let mut f = Vec::new();
        let put = |f: &mut Vec<u8>, name: &str, size: usize, fill: u8| {
            let at = f.len();
            let mut b = set_header(name, 3, at as u32, size as u32, 1, &[0xA0]);
            b.resize(size, fill);
            f.extend(b);
            f.resize(f.len().div_ceil(SET_ALIGN) * SET_ALIGN, 0);
        };
        put(&mut f, "curveOnly", 0x180, 1);
        put(&mut f, "BladeMeltIdle01", 80 << 10, 2);
        put(&mut f, "attAxeLight01", 20 << 10, 3);
        let stray = set_header("strayName", 3, 0x1234, 0x400, 1, &[]);
        let at = f.len();
        f.extend(stray);
        f.resize(at + SET_ALIGN, 0);
        let dir = std::env::temp_dir().join(format!("gowring-walk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // plain, then as an LZ4 frame of stored (raw) 64 KiB blocks
        let plain = dir.join("r_heroa00.as");
        std::fs::write(&plain, &f).unwrap();
        let mut lz = crate::lz4::MAGIC.to_vec();
        lz.extend_from_slice(&[0x60, 0x40, 0x82]);
        for c in f.chunks(64 << 10) {
            lz.extend_from_slice(&(c.len() as u32 | 0x8000_0000).to_le_bytes());
            lz.extend_from_slice(c);
        }
        lz.extend_from_slice(&0u32.to_le_bytes());
        let packed = dir.join("r_heroa00_lz.as");
        std::fs::write(&packed, &lz).unwrap();
        for (path, lz4) in [(&plain, false), (&packed, true)] {
            let mut saved: Vec<(String, usize, u8)> = Vec::new();
            let idx = walk_set_file(path, study_picker(3, 64 << 20), |s, b| saved.push((s.name.clone(), b.len(), b[b.len() - 1])), || true).unwrap();
            assert_eq!(idx.lz4, lz4);
            assert!(idx.complete);
            let names: Vec<(&str, bool)> = idx.sets.iter().map(|(_, s, own)| (s.name.as_str(), *own)).collect();
            assert_eq!(names, vec![("curveOnly", true), ("BladeMeltIdle01", true), ("attAxeLight01", true), ("strayName", false)]);
            assert_eq!(saved, vec![("BladeMeltIdle01".to_string(), 80 << 10, 2), ("attAxeLight01".to_string(), 20 << 10, 3)]);
            let tsv = index_tsv("r_heroa00.as", f.len() as u64, &idx);
            assert!(tsv.contains("BladeMeltIdle01\t1\t3\t1\t0x14000"), "{tsv}");
        }
        // the picker keeps to its limits
        let mut pick = study_picker(1, 64 << 20);
        let s = |name: &str, size: u32| AnimSet { flags: 3, off: 0, size, hash: 0, name: name.into(), type_hash: 0, id: 0, files: 0, file_offs: vec![] };
        assert!(pick(&s("attAxeA", 20 << 10)));
        assert!(!pick(&s("attAxeB", 20 << 10)), "one per word");
        assert!(!pick(&s("RageStart", 4 << 20)), "too big");
        assert!(!pick(&s("narSomething", 20 << 10)), "not a study word");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_set_bigger_than_what_was_read_is_stepped_over_without_a_panic() {
        // a 20 MiB set: its header sends the walk 20 MiB ahead while 1 MiB has been read (0.1.38 panicked here)
        let mut f = set_header("BigCinematic", 3, 0, 20 << 20, 1, &[]);
        f.resize(20 << 20, 0);
        let at = f.len();
        f.extend(set_header("attAxeAfter", 3, at as u32, 20 << 10, 1, &[]));
        f.resize(at + (20 << 10), 9);
        let dir = std::env::temp_dir().join(format!("gowring-walk-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r_heroa00.as");
        std::fs::write(&path, &f).unwrap();
        let mut saved = Vec::new();
        let idx = walk_set_file(&path, study_picker(3, 64 << 20), |s, b| saved.push((s.name.clone(), b.len())), || true).unwrap();
        assert!(idx.complete);
        assert_eq!(idx.sets.iter().map(|(_, s, own)| (s.name.as_str(), *own)).collect::<Vec<_>>(), vec![("BigCinematic", true), ("attAxeAfter", true)]);
        assert_eq!(saved, vec![("attAxeAfter".to_string(), 20 << 10)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_goproto_skeleton_reads_with_its_rest_pose_and_inverse_binds() {
        // root, a hip 1 m up, a knee 0.5 m below the hip (local), laid out as in Kratos's goProto
        let n = 3usize;
        let mut b = vec![0u8; 0x18];
        b[0x10..0x12].copy_from_slice(&(n as u16).to_le_bytes());
        for (f, fc, ns, p) in [(0u16, 1i16, -1i16, -1i16), (97, 2, -1, 0), (65, -1, -1, 1)] {
            b.extend(f.to_le_bytes());
            b.extend(fc.to_le_bytes());
            b.extend(ns.to_le_bytes());
            b.extend(p.to_le_bytes());
        }
        for i in 0..n {
            b.extend((0xABC0 + i as u64).to_le_bytes());
        }
        b.extend(vec![0x5A; n * 16]);
        let head = b.len();
        let gap = 0x58usize; // header and one spare matrix before the rest transforms, as in the real file
        let local_at = head + gap;
        let ib = local_at + n * 64;
        b.extend(0u64.to_le_bytes());
        b.extend((n as u32).to_le_bytes());
        b.extend((n as u32).to_le_bytes());
        b.extend(((ib - head - 8) as u64).to_le_bytes());
        b.resize(local_at, 0);
        let m = |t: [f32; 3]| -> Vec<u8> {
            let v: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., t[0], t[1], t[2], 1.];
            v.iter().flat_map(|x| x.to_le_bytes()).collect()
        };
        for t in [[0., 0., 0.], [0., 1., 0.], [0., -0.5, 0.]] {
            b.extend(m(t));
        }
        for t in [[0., 0., 0.], [0., -1., 0.], [0., -0.5, 0.]] {
            b.extend(m(t));
        }
        let sk = Skeleton::parse(&b).expect("parses");
        assert_eq!(sk.parent, vec![-1, 0, 1]);
        assert_eq!(sk.flags, vec![0, 97, 65]);
        assert_eq!(sk.name_hash[2], 0xABC2);
        assert_eq!(sk.rest_positions(), vec![[0., 0., 0.], [0., 1., 0.], [0., 0.5, 0.]]);
        assert_eq!(sk.inverse_bind[1][13], -1.0);
        // a child listed before its parent is not a skeleton
        let mut bad = b.clone();
        bad[0x18 + 8 + 6..0x18 + 8 + 8].copy_from_slice(&2i16.to_le_bytes());
        assert!(Skeleton::parse(&bad).is_none());
    }

    /// Run against a player's own probe file: `GOWRING_GOPROTO=path cargo test -p gowring-gow real_goproto -- --ignored`.
    #[test]
    #[ignore]
    fn real_goproto_reads_as_a_standing_figure() {
        let Ok(path) = std::env::var("GOWRING_GOPROTO") else { return };
        let sk = Skeleton::parse(&std::fs::read(path).unwrap()).expect("parses");
        let pos = sk.rest_positions();
        let (lo, hi) = pos.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[1]), h.max(p[1])));
        println!("{} joints, rest height {lo:.3}..{hi:.3}", sk.parent.len());
        // the composed rest pose matches the inverse binds where they are set
        let (mut checked, mut off) = (0, 0);
        for (i, ib) in sk.inverse_bind.iter().enumerate() {
            let identity = ib.iter().enumerate().all(|(k, v)| (v - if k % 5 == 0 { 1.0 } else { 0.0 }).abs() < 1e-5);
            if identity {
                continue;
            }
            // inverse bind maps the joint's own position to the origin
            let p = pos[i];
            let x: Vec<f32> = (0..3).map(|r| ib[r] * p[0] + ib[4 + r] * p[1] + ib[8 + r] * p[2] + ib[12 + r]).collect();
            if x.iter().all(|v| v.abs() < 1e-2) {
                checked += 1;
            } else {
                println!("joint {i}: rest pose and bind differ by {x:?}");
                off += 1;
            }
        }
        println!("{checked} inverse binds agree with the rest pose, {off} differ");
        // Kratos: 9 of 826, mirrored pairs of corrective joints 1-4 cm from their bind
        assert!(off * 50 <= checked, "the rest pose should match the binds for nearly every joint");
        // the skinned joints stand between the floor and head height (helpers that skin nothing may sit anywhere)
        let skinned: Vec<f32> = sk.inverse_bind.iter().zip(&pos).filter(|(ib, _)| ib.iter().enumerate().any(|(k, v)| (v - if k % 5 == 0 { 1.0 } else { 0.0 }).abs() >= 1e-5)).map(|(_, p)| p[1]).collect();
        let (slo, shi) = skinned.iter().fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
        println!("skinned joints stand {slo:.3}..{shi:.3}");
        assert!(shi > 1.5 && shi < 2.5 && slo > -0.1, "{slo}..{shi}");
    }

    #[test]
    fn mesh_records_read_their_bounds_buffers_and_streams() {
        // two records laid out as in Kratos's MESH chunk (self-relative offsets, streams after the record)
        let rec = |lod: f32, nv: u32, vb: u32| {
            let mut r = vec![0u8; 0x98];
            r[0..4].copy_from_slice(&lod.to_le_bytes());
            for (k, v) in [0.1f32, 0.2, 0.3, -0.2, 1.1, 0.05].iter().enumerate() {
                r[0x10 + 4 * k..0x14 + 4 * k].copy_from_slice(&v.to_le_bytes());
            }
            for (o, v) in [(0x30, nv * 12), (0x38, vb + nv * 12), (0x40, vb), (0x44, nv), (0x48, nv / 3), (0x4C, 7170), (0x5C, nv), (0x60, 0x88), (0x64, 0x90)] {
                r[o..o + 4].copy_from_slice(&(v as u32).to_le_bytes());
            }
            r[0x84] = 1;
            r[0x88..0x8C].copy_from_slice(&[0, 0, 3, 0]);
            r
        };
        let mut b = vec![0u8; 0x48];
        b[0x10..0x14].copy_from_slice(&2u32.to_le_bytes());
        let r0 = rec(1.5, 3, 0);
        let r1 = rec(8.0, 6, 64);
        b[0x40..0x44].copy_from_slice(&((0x48 - 0x40) as u32).to_le_bytes());
        b[0x44..0x48].copy_from_slice(&((0x48 + r0.len() - 0x44) as u32).to_le_bytes());
        b.extend(&r0);
        b.extend(&r1);
        let rs = mesh_records(&b);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[1].vertex_offset, 64);
        assert_eq!(rs[1].vertices, 6);
        assert_eq!(rs[0].centre, [-0.2, 1.1, 0.05]);
        assert_eq!(rs[0].streams, vec![(0, 0, 3, 0)]);
        assert_eq!(rs[0].strides, vec![12]);
        // positions inside the bounds are counted, others not
        let pos: Vec<u8> = [[-0.25f32, 1.0, 0.1], [-0.2, 1.1, 0.05], [5.0, 0.0, 0.0]].iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        assert_eq!(rs[0].positions_inside(&pos), 2);
    }

    /// Run against a player's own probe files: `GOWRING_MESH=MESH.bin GOWRING_MG_GPU=gpu.bin cargo test -p gowring-gow
    /// real_mesh -- --ignored`.
    #[test]
    #[ignore]
    fn real_mesh_records_and_a_resident_level_of_detail() {
        let (Ok(mesh), Ok(gpu)) = (std::env::var("GOWRING_MESH"), std::env::var("GOWRING_MG_GPU")) else { return };
        let rs = mesh_records(&std::fs::read(mesh).unwrap());
        let gpu = std::fs::read(gpu).unwrap();
        println!("{} records; first: {:?}", rs.len(), rs[0]);
        // Kratos: record 1 (219 vertices, a belt piece's lowest level) is the first thing in the WAD's GPU chunk
        assert_eq!(rs[1].positions_inside(&gpu), rs[1].vertices as usize);
        let far = rs.iter().map(|r| r.vertex_offset + r.vertex_bytes as u64).max().unwrap();
        println!("the whole buffer spans {far} bytes; the WAD holds {}", gpu.len());
        // resident draws, as the launcher's probe finds them, and a fingerprint search in a planted stream file
        let mut resident: Vec<usize> = (0..rs.len())
            .filter(|k| {
                let r = &rs[*k];
                r.vertices >= 64 && r.streams.first().is_some_and(|s| s.0 == 0) && gpu.get(r.streams[0].3 as usize..).is_some_and(|b| r.positions_inside(b) == r.vertices as usize)
            })
            .collect();
        resident.sort_by_key(|k| std::cmp::Reverse(rs[*k].vertices));
        println!("{} resident draws; largest {:?}", resident.len(), resident.iter().take(3).map(|k| (k, rs[*k].vertices)).collect::<Vec<_>>());
        assert!(resident.len() >= 50);
        let a = resident[0];
        let at = rs[a].streams[0].3 as usize;
        let sig = &gpu[at..at + (rs[a].vertices as usize * 12).min(192)];
        // planted a little way into a small file (the real one sits at the base plus the draw's buffer offset)
        let base = 12345u64;
        let mut fake = vec![0u8; base as usize];
        fake.extend_from_slice(&gpu[at..at + rs[a].vertices as usize * 12]);
        let dir = std::env::temp_dir().join(format!("gowring-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stream.bin"), &fake).unwrap();
        let found = find_bytes_in_file(&dir.join("stream.bin"), sig, || true).unwrap();
        assert_eq!(found, Some(base));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interleaved_draws_read_each_stream_at_its_offset_in_the_vertex() {
        // one record whose position (f32 x3 at 0) and normal (packed at 12) share slot 0: a 16-byte stride
        let mut r = vec![0u8; 0xB0];
        for (k, v) in [0.5f32, 0.5, 0.5, 0.0, 1.0, 0.0].iter().enumerate() {
            r[0x10 + 4 * k..0x14 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        for (o, v) in [(0x30, 48u32), (0x44, 3), (0x48, 1), (0x5C, 3), (0x60, 0x90), (0x64, 0xA0)] {
            r[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        r[0x84] = 2;
        r[0x90..0x98].copy_from_slice(&[0, 0, 3, 0, 0, 0, 0, 0]);
        r[0x98..0xA0].copy_from_slice(&[1, 3, 1, 12, 0, 0, 0, 0]);
        r[0xA0..0xA4].copy_from_slice(&0u32.to_le_bytes());
        let mut b = vec![0u8; 0x44];
        b[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
        b[0x40..0x44].copy_from_slice(&4u32.to_le_bytes());
        b.extend(&r);
        let rs = mesh_records(&b);
        assert_eq!(rs[0].streams, vec![(0, 0, 3, 0), (1, 3, 1, 12)]);
        assert_eq!(rs[0].strides, vec![16, 16]);
        let mut res = Vec::new();
        for p in [[0.1f32, 1.0, 0.0], [0.2, 1.1, 0.1], [0.0, 0.9, -0.1]] {
            res.extend(p.iter().flat_map(|x| x.to_le_bytes()));
            res.extend(0xC0DEu32.to_le_bytes());
        }
        res.extend([0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()));
        let (p, t) = draw_geometry(&res, &rs[0]).unwrap();
        assert_eq!((p[2], t[0]), ([0.0, 0.9, -0.1], [0, 1, 2]));
    }

    #[test]
    fn skin_joints_unpack_from_the_top_of_the_reversed_words() {
        // ten 11-bit joints 1..=10 from the top of a 128-bit value, stored as its four u32 words highest first
        let x = (0..10u128).fold(0u128, |acc, i| acc | ((i + 1) << (128 - 11 * (i + 1))));
        let words: Vec<u8> = (0..4).flat_map(|k| ((x >> (96 - 32 * k)) as u32).to_le_bytes()).collect();
        // weights: three 10:10:10:2 words with 0.4, 0.2, 0.1 then zeros; the rest goes to the fourth joint
        let w = |a: u32, b: u32, c: u32| (a | b << 10 | c << 20).to_le_bytes();
        let mut res: Vec<u8> = Vec::new();
        for p in [[0.0f32, 1.0, 0.0], [0.1, 1.0, 0.0], [0.0, 1.1, 0.0]] {
            res.extend(p.iter().flat_map(|x| x.to_le_bytes()));
            res.extend(&words);
            res.extend(w(409, 205, 102));
            res.extend(w(0, 0, 0));
            res.extend(w(0, 0, 0));
        }
        let idx_at = res.len() as u32;
        res.extend([0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()));
        let rec = MeshRecord {
            lod_distance: 1.0,
            centre: [0.0, 1.0, 0.0],
            half_extent: [0.5, 0.5, 0.5],
            vertex_bytes: idx_at,
            index_offset: 0,
            vertex_offset: 0,
            vertices: 3,
            triangles: 1,
            format: 0,
            indices: 3,
            streams: vec![(0, 0, 3, 0), (9, 2, 4, 12), (10, 2, 3, 28)],
            strides: vec![40, 40, 40],
            hash: 1,
            material: 0,
        };
        let v = draw_vertices(&res, &rec).unwrap();
        let inf = &v.influences[2];
        assert_eq!(inf.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2, 3, 10]);
        assert!((inf[0].1 - 0.4).abs() < 0.01 && (inf[3].1 - 0.3).abs() < 0.01);
    }

    fn entry(name: &str, id: u16, file_off: u64, length: u32) -> crate::wtoc::Entry {
        crate::wtoc::Entry { id, version: 0, length, name: name.into(), debug_size: 0, temp_size: 0, align: 16, client: 0, server: 0, mem_type: 0, bits: 0, mem_off: 0, file_off, block: 0 }
    }

    #[test]
    fn a_models_chunks_are_found_by_name_its_skeleton_in_any_case_and_its_table_after_the_mesh() {
        let buf: Vec<u8> = (0..200u8).collect();
        let es = vec![
            entry("MG_axe00_0_gpu", 29, 0, 10),
            entry("MESH_axe00_0", 1, 10, 20),
            entry("MAT_15394922D6E57ABC", 60, 30, 8),
            entry("MG_axe00_0", 1, 40, 12),
            entry("goProtoAxe00", 1, 60, 4),
            entry("MAT_15394922D6E57ABC", 1, 70, 6),
            entry("TX_gem_blue_base_96B139E57CE66328", 60, 80, 5),
            entry("beyond", 1, 190, 50),
        ];
        let c = model_chunks(&buf, &es, "axe00").unwrap();
        assert_eq!((c.mesh, c.mg, c.gpu), (&buf[10..30], &buf[40..52], &buf[0..10]));
        assert_eq!(c.materials, Some(&buf[30..38]));
        assert_eq!(c.proto, Some(&buf[60..64]));
        // exact spelling first, then any case
        assert_eq!(chunk_at(&es, "goprotoaxe00", 1), Some(4));
        assert_eq!(chunk_at(&es, "goProtoAxe00", 60), None);
        // a chunk past the bytes read is not there
        assert_eq!(chunk(&buf, &es, "beyond", 1), None);
        // a material's references follow its first id-1 chunk
        assert_eq!(material_refs_chunk(&buf, &es, "MAT_15394922D6E57ABC"), Some(&buf[80..85]));
        assert_eq!(model_chunks(&buf, &es, "kratosspear00").unwrap_err(), "MESH_kratosspear00_0 (id 1) is not in the bytes read");
        let no_table = vec![es[0].clone(), es[1].clone(), entry("MAT_0000000000000000", 1, 30, 0), es[3].clone()];
        let c = model_chunks(&buf, &no_table, "axe00").unwrap();
        assert!(c.materials.is_none() && c.proto.is_none());
    }

    #[test]
    fn parts_list_their_levels_most_detailed_first() {
        // two parts: one with levels (1 draw from 0, then 1 from 1, then the empty last), one with 2 draws from 2
        let mut b = vec![0u8; 0x44 + 8];
        b[0x30..0x32].copy_from_slice(&2u16.to_le_bytes());
        let part = |levels: &[(u32, f32, u16)]| {
            let mut p = vec![0u8; 56 + 4 * levels.len()];
            p[2] = levels.len() as u8;
            let head = p.len();
            for i in 0..levels.len() {
                p[56 + 4 * i..60 + 4 * i].copy_from_slice(&((head + 16 * i) as u32).to_le_bytes());
            }
            for (n, d, first) in levels {
                let mut e = vec![0u8; 16];
                e[0..4].copy_from_slice(&n.to_le_bytes());
                e[4..8].copy_from_slice(&d.to_le_bytes());
                e[10..12].copy_from_slice(&first.to_le_bytes());
                p.extend(e);
            }
            p
        };
        for (k, levels) in [vec![(1, 1280.0, 0), (1, 2560.0, 1), (0, 32767.0, 0)], vec![(2, 8.0, 2), (0, 32767.0, 0)]].iter().enumerate() {
            let at = b.len() as u32;
            b[0x44 + 4 * k..0x48 + 4 * k].copy_from_slice(&at.to_le_bytes());
            b.extend(part(levels));
        }
        // part 1 attached to joint 457, rigid; part 0 skinned
        let a1 = u32::from_le_bytes(b[0x48..0x4C].try_into().unwrap()) as usize;
        b[a1..a1 + 2].copy_from_slice(&457u16.to_le_bytes());
        let a0 = u32::from_le_bytes(b[0x44..0x48].try_into().unwrap()) as usize;
        b[a0 + 3] = 2;
        let info = mg_part_info(&b);
        assert_eq!(info.iter().map(|p| (p.joint, p.kind, p.levels.len())).collect::<Vec<_>>(), [(0, 2, 3), (457, 0, 2)]);
        assert_eq!(level0_records(&b, None), vec![0, 2, 3]);
        assert_eq!(level0_records(&b, Some(&[1, 7])), vec![2, 3]);
        let ps = mg_parts(&b);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0][1], Level { draws: 1, distance: 2560.0, first: 1 });
        assert_eq!(ps[1][0], Level { draws: 2, distance: 8.0, first: 2 });
        assert_eq!(ps[1][1].draws, 0);
    }

    #[test]
    fn draws_sharing_a_resource_decode_from_their_own_streams_and_indices() {
        // two triangles' worth of draws back to back, each: positions, a normal stream, then u16 indices
        let rec = |at: u32, nv: u32, idx_at: u32| MeshRecord {
            lod_distance: 1.0,
            centre: [0.0, 1.0, 0.0],
            half_extent: [0.5, 0.5, 0.5],
            vertex_bytes: idx_at,
            index_offset: 0,
            vertex_offset: 0,
            vertices: nv,
            triangles: 1,
            format: 0,
            indices: 3,
            streams: vec![(0, 0, 3, at), (1, 3, 1, at + nv * 12)],
            strides: vec![12, 4],
            hash: 0x1234,
            material: 0,
        };
        let mut res = Vec::new();
        let mut put = |pts: &[[f32; 3]], tri: [u16; 3]| {
            let at = res.len() as u32;
            for p in pts {
                res.extend(p.iter().flat_map(|x| x.to_le_bytes()));
            }
            res.extend(vec![0u8; pts.len() * 4]);
            let idx_at = res.len() as u32;
            res.extend(tri.iter().flat_map(|i| i.to_le_bytes()));
            while res.len() % 16 != 0 {
                res.push(0);
            }
            rec(at, pts.len() as u32, idx_at)
        };
        let a = put(&[[0.1, 1.0, 0.0], [0.2, 1.1, 0.0], [0.0, 1.2, 0.1]], [0, 1, 2]);
        let b = put(&[[-0.1, 0.9, 0.0], [-0.2, 0.8, 0.1], [0.0, 0.7, 0.2], [0.3, 0.6, 0.0]], [3, 1, 0]);
        let (pa, ta) = draw_geometry(&res, &a).unwrap();
        let (pb, tb) = draw_geometry(&res, &b).unwrap();
        assert_eq!((pa[1], ta[0]), ([0.2, 1.1, 0.0], [0, 1, 2]));
        assert_eq!((pb.len(), pb[3], tb[0]), (4, [0.3, 0.6, 0.0], [3, 1, 0]));
        // read from the wrong place, the bounds check refuses it
        let mut off = b.clone();
        off.streams[0].3 += 4;
        assert!(draw_geometry(&res, &off).is_none());
        // a compact copy: header, then the first draw's scaled positions
        let c = compact_geometry(&[(7, 3, &a, &pa, &ta)]);
        assert_eq!(&c[..4], b"GRKM");
        assert_eq!(u32::from_le_bytes(c[8..12].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(c[12..16].try_into().unwrap()), 7);
        assert_eq!(c.len(), 12 + 16 + 24 + 3 * 6 + 3 * 2);
        let x0 = i16::from_le_bytes([c[52], c[53]]) as f32 / 32767.0 * 0.51;
        assert!((x0 - 0.1).abs() < 1e-3);
    }

    /// With a player's `MESH_heroa00_0`, `MG_heroa00_0`, `MG_heroa00_0_gpu` and decompressed `root.lodpack.toc`:
    /// `GOWRING_MESH=... GOWRING_MG=... GOWRING_MG_GPU=... GOWRING_LODPACK_TOC=... cargo test -p gowring-gow
    /// real_parts -- --ignored`.
    #[test]
    #[ignore]
    fn real_parts_and_their_resources() {
        let (Ok(mesh), Ok(mg), Ok(gpu), Ok(toc)) = (std::env::var("GOWRING_MESH"), std::env::var("GOWRING_MG"), std::env::var("GOWRING_MG_GPU"), std::env::var("GOWRING_LODPACK_TOC")) else { return };
        let rs = mesh_records(&std::fs::read(mesh).unwrap());
        let ps = mg_parts(&std::fs::read(mg).unwrap());
        let gpu = std::fs::read(gpu).unwrap();
        let toc = crate::lodpack::Toc::parse(&std::fs::read(toc).unwrap()).unwrap();
        let mut seen = vec![0; rs.len()];
        for l in ps.iter().flatten() {
            for k in l.first..l.first + l.draws {
                seen[k as usize] += 1;
            }
        }
        assert!(seen.iter().all(|n| *n == 1), "every record in exactly one level");
        let streamed = rs.iter().filter(|r| r.hash != 0).count();
        let found = rs.iter().filter(|r| r.hash != 0 && toc.find(r.hash).is_some()).count();
        let detail: Vec<usize> = ps.iter().flat_map(|l| (l[0].first..l[0].first + l[0].draws).map(|k| k as usize)).collect();
        let resident_ok = detail.iter().filter(|k| rs[**k].hash == 0 && draw_geometry(&gpu, &rs[**k]).is_some()).count();
        println!("{} parts, {} records ({streamed} streamed, {found} in the pack), {} most detailed ({resident_ok} decoded from the WAD)", ps.len(), rs.len(), detail.len());
        assert_eq!(found, streamed);
        assert!(resident_ok > 0);
        // the skin's interleaved draws, from the resource a player's probe saved (GOWRING_SKIN_SAMPLE)
        if let Ok(skin) = std::env::var("GOWRING_SKIN_SAMPLE") {
            let b = std::fs::read(skin).unwrap();
            for k in 1135..=1140 {
                let (p, t) = draw_geometry(&b, &rs[k]).unwrap_or_else(|| panic!("record {k}"));
                println!("record {k}: stride {}, {} vertices, {} triangles", rs[k].strides[0], p.len(), t.len());
                let v = draw_vertices(&b, &rs[k]).unwrap_or_else(|| panic!("vertices of record {k}"));
                let most = v.influences.iter().map(|i| i.len()).max().unwrap();
                assert!(v.influences.iter().flatten().all(|(j, w)| (*j as usize) < 1458 && *w > 0.0));
                assert!(v.normal.iter().all(|n| (n.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 0.05));
                assert!(v.uv[0].iter().flatten().all(|x| x.is_finite()));
                println!("  up to {most} influences; first vertex {:?}", v.influences[0]);
            }
        }
        // every resident draw's vertices
        let resident = rs.iter().filter(|r| r.hash == 0 && draw_vertices(&gpu, r).is_some()).count();
        println!("{resident} of {} draws in the WAD decode with all their vertex data", rs.iter().filter(|r| r.hash == 0).count());
        for (k, r) in rs.iter().enumerate().filter(|(_, r)| r.hash == 0 && draw_vertices(&gpu, r).is_none()).take(12) {
            println!("  record {k}: geometry {}, streams {:?}", draw_geometry(&gpu, r).is_some(), r.streams.iter().map(|s| (s.0, s.1, s.2)).collect::<Vec<_>>());
        }
        assert_eq!(resident, rs.iter().filter(|r| r.hash == 0).count(), "every draw in the WAD decodes");
    }

    #[test]
    fn a_records_positions_are_found_after_unrelated_bytes() {
        let rec = MeshRecord { lod_distance: 1.0, centre: [0.0, 1.0, 0.0], half_extent: [0.1, 0.1, 0.1], vertex_bytes: 36, index_offset: 0, vertex_offset: 0, vertices: 3, triangles: 1, format: 0, indices: 3, streams: vec![], strides: vec![], hash: 0, material: 0 };
        let mut b = vec![0xFFu8; 20]; // not floats in range
        for p in [[0.05f32, 1.02, -0.03], [0.0, 0.95, 0.01], [-0.08, 1.07, 0.09]] {
            b.extend(p.iter().flat_map(|x| x.to_le_bytes()));
        }
        b.extend([0xFFu8; 8]);
        assert_eq!(find_positions(&b, &rec), Some(20));
        assert_eq!(find_positions(&b[..40], &rec), None);
    }

    #[test]
    fn bytes_are_found_in_plain_and_lz4_files_across_reads() {
        let dir = std::env::temp_dir().join(format!("gowring-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = vec![7u8; 9 << 20];
        let at = (4 << 20) - 3; // straddles the first 4 MiB read
        f[at..at + 6].copy_from_slice(b"KRATOS");
        let plain = dir.join("a.bin");
        std::fs::write(&plain, &f).unwrap();
        assert_eq!(find_bytes_in_file(&plain, b"KRATOS", || true).unwrap(), Some(at as u64));
        assert_eq!(find_bytes_in_file(&plain, b"ATREUS", || true).unwrap(), None);
        let mut lz = crate::lz4::MAGIC.to_vec();
        lz.extend_from_slice(&[0x60, 0x40, 0x82]);
        for c in f.chunks(64 << 10) {
            lz.extend_from_slice(&(c.len() as u32 | 0x8000_0000).to_le_bytes());
            lz.extend_from_slice(c);
        }
        lz.extend_from_slice(&0u32.to_le_bytes());
        let packed = dir.join("b.bin");
        std::fs::write(&packed, &lz).unwrap();
        assert_eq!(find_bytes_in_file(&packed, b"KRATOS", || true).unwrap(), Some(at as u64));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

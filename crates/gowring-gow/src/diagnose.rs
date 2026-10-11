//! `--diagnose`: a plain-text report of what the reader finds in the player's install, written so a
//! person (or a later session) can finish the asset mapping without access to the game.

use crate::catalog::EntryKind;
use crate::prepare::AssetReq;
use crate::{audiopack, bnk, oodle, scan, wad};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

fn mb(b: u64) -> String {
    format!("{:.1} MB", b as f64 / 1_048_576.0)
}


/// One 0x90-byte record of the decompressed WAD table (`WTOC`), as far as it is understood: the 0xaa-filled
/// id block at +8, a name at +0x18, and a tail of numbers at +0x68 whose meaning is still being worked out.
#[derive(Clone, Debug)]
struct WtocRec {
    a: u16,
    b: u16,
    c: u32,
    name: String,
    tail: Vec<u8>,
}

const WTOC_FIRST: usize = 0x40;
const WTOC_REC: usize = 0x90;

/// The record count is the u32 at +8 of the decompressed `WTOC` header (31 in a tiny WAD, 31568 in `r_ui.wad`).
fn wtoc_count(buf: &[u8]) -> Option<usize> {
    if buf.len() < 0x40 || &buf[..4] != b"WTOC" {
        return None;
    }
    Some(u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as usize)
}

fn wtoc_records(buf: &[u8]) -> Vec<WtocRec> {
    let Some(count) = wtoc_count(buf) else { return Vec::new() };
    let mut v = Vec::new();
    for i in 0..count {
        let at = WTOC_FIRST + i * WTOC_REC;
        let Some(r) = buf.get(at..at + WTOC_REC) else { break };
        let name_raw = &r[0x18..0x68];
        let end = name_raw.iter().position(|b| *b == 0).unwrap_or(name_raw.len());
        let printable = name_raw[..end].iter().all(|b| (0x20..0x7f).contains(b));
        v.push(WtocRec {
            a: u16::from_le_bytes([r[0], r[1]]),
            b: u16::from_le_bytes([r[2], r[3]]),
            c: u32::from_le_bytes([r[4], r[5], r[6], r[7]]),
            name: if printable { String::from_utf8_lossy(&name_raw[..end]).into_owned() } else { String::new() },
            tail: r[0x68..].to_vec(),
        });
    }
    v
}

/// Bytes of decompressed stream the whole table needs (plus a little), read from the header alone.
fn wtoc_prefix_len(head: &[u8]) -> usize {
    wtoc_count(head).map(|c| (WTOC_FIRST + c * WTOC_REC + 0x1000).min(24 << 20)).unwrap_or(64 << 10)
}

/// Decompress the table of a WAD: header, records, and how much was read.
fn wtoc_load(path: &Path) -> anyhow::Result<(Vec<u8>, crate::lz4::FrameInfo)> {
    let (head, _) = crate::lz4::read_file_prefix(path, 64)?;
    crate::lz4::read_file_prefix(path, wtoc_prefix_len(&head))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
}

fn hexdump(b: &[u8], max: usize) -> String {
    let mut o = String::new();
    for (i, row) in b.chunks(16).take(max / 16).enumerate() {
        let _ = writeln!(o, "    {:05x}  {:<47}  {}", i * 16, row.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "), row.iter().map(|x| if (32..127).contains(x) { *x as char } else { '.' }).collect::<String>());
    }
    o
}

pub fn report(gow: &Path, er: Option<&Path>, reqs: &[AssetReq], vgmstream: Option<&Path>) -> String {
    report_until(gow, er, reqs, vgmstream, None)
}

fn hex_head(path: &Path, n: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    let got = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
    buf.truncate(got);
    buf.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("")
}

/// The same report, but each section stops reading at `deadline` (and says so), so it can run while the
/// game is loading without competing with it for long.
pub fn report_until(gow: &Path, er: Option<&Path>, reqs: &[AssetReq], vgmstream: Option<&Path>, deadline: Option<std::time::Instant>) -> String {
    let late = || deadline.map(|d| std::time::Instant::now() >= d).unwrap_or(false);
    let mut o = String::new();
    let _ = writeln!(o, "GowRing diagnose — {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(o, "God of War Ragnarök folder: {}", gow.display());
    if let Some(er) = er {
        let exe = er.join("Game").join("eldenring.exe");
        let _ = writeln!(o, "Elden Ring folder: {}  (eldenring.exe {})", er.display(), match std::fs::metadata(&exe) {
            Ok(m) => format!("{} bytes", m.len()),
            Err(_) => "MISSING".into(),
        });
    }
    let _ = writeln!(o, "vgmstream-cli: {}", vgmstream.map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into()));
    if !gow.is_dir() {
        let _ = writeln!(o, "\nThe Ragnarök folder does not exist; nothing more to report.");
        return o;
    }
    let files = scan::walk(gow, 8);
    let s = scan::summarize(&files);
    let _ = writeln!(o, "\n== Inventory: {} files, {}", s.files, mb(s.bytes));
    for (k, (n, b)) in &s.by_kind {
        let _ = writeln!(o, "  {k:<10} {n:>7} files  {}", mb(*b));
    }
    let _ = writeln!(o, "  largest top-level folders:");
    for (d, n, b) in &s.top_dirs {
        let _ = writeln!(o, "    {d:<32} {n:>7} files  {}", mb(*b));
    }
    let _ = writeln!(o, "  Oodle: {}", oodle::find(gow).map(|p| p.display().to_string()).unwrap_or_else(|| "no oo2core dll found".into()));

    // ---- audio ------------------------------------------------------------------------------------
    let tocs: Vec<_> = files.iter().filter(|f| f.kind == scan::FileKind::AudioToc).collect();
    let _ = writeln!(o, "\n== Audio packs: {} TOC files", tocs.len());
    let mut codecs: BTreeMap<&str, usize> = BTreeMap::new();
    let mut total_ms: u64 = 0;
    for (i, t) in tocs.iter().enumerate() {
        if late() {
            let _ = writeln!(o, "  (time budget reached; {} more TOC files not read)", tocs.len() - i);
            break;
        }
        let Ok(bytes) = std::fs::read(gow.join(&t.path)) else { continue };
        match audiopack::parse_toc(&bytes) {
            Ok(toc) => {
                let pack = gow.join(audiopack::pack_path_for(Path::new(&t.path)));
                let mut here: BTreeMap<&str, usize> = BTreeMap::new();
                let mut ms = 0u64;
                if let Ok(mut f) = std::fs::File::open(&pack) {
                    for e in toc.entries.iter().take(400) {
                        if let Ok(b) = audiopack::read_entry(&mut f, e) {
                            if let Some(info) = audiopack::wem_info(&b) {
                                *here.entry(info.codec_name).or_default() += 1;
                                *codecs.entry(info.codec_name).or_default() += 1;
                                ms += info.approx_ms as u64;
                            }
                        }
                    }
                }
                total_ms += ms;
                if i < 40 {
                    let _ = writeln!(o, "  {}  v{} {} media  sampled codecs {:?}  ~{:.0}s", t.path, toc.version, toc.entries.len(), here, ms as f64 / 1000.0);
                }
            }
            Err(e) => {
                let _ = writeln!(o, "  {}  NOT UNDERSTOOD: {e}  head: {:02x?}", t.path, &bytes[..bytes.len().min(16)]);
            }
        }
    }
    let _ = writeln!(o, "  codec histogram (sampled): {codecs:?}   sampled duration ~{:.0}s", total_ms as f64 / 1000.0);

    // ---- banks -------------------------------------------------------------------------------------
    let banks: Vec<_> = files.iter().filter(|f| f.kind == scan::FileKind::Bnk).collect();
    let _ = writeln!(o, "\n== Wwise banks: {}", banks.len());
    for b in banks.iter().take(25) {
        if let Ok(bytes) = std::fs::read(gow.join(&b.path)) {
            match bnk::parse(&bytes) {
                Ok(i) => {
                    let types: Vec<String> = i.hirc_objects.iter().map(|(t, n)| format!("{}={n}", bnk::hirc_type_name(*t))).collect();
                    let _ = writeln!(o, "  {} v{} id {:08x} media {} events {} [{}]", b.path, i.version, i.bank_id, i.embedded_media.len(), i.event_ids.len(), types.join(", "));
                }
                Err(e) => {
                    let _ = writeln!(o, "  {} NOT UNDERSTOOD: {e}", b.path);
                }
            }
        }
    }

    // ---- WADs / textures ------------------------------------------------------------------------------
    // The Ragnarök PC WADs are not the 2018 layout. Until the container is decoded, the first bytes of the
    // WADs that matter (HUD, Mimir, the player) are the evidence the next version is written from.
    let wads: Vec<_> = files.iter().filter(|f| f.kind == scan::FileKind::Wad).collect();
    let _ = writeln!(o, "\n== WAD files: {}", wads.len());
    let mut parsed = 0usize;
    for w in wads.iter().take(60) {
        if late() {
            break;
        }
        if let Ok(p) = wad::parse_file(&gow.join(&w.path)) {
            parsed += 1;
            let mut types: BTreeMap<u16, usize> = BTreeMap::new();
            for e in &p.entries {
                *types.entry(e.kind).or_default() += 1;
            }
            let names: Vec<_> = p.entries.iter().take(6).map(|e| e.name.as_str()).collect();
            let _ = writeln!(o, "  PARSED {} {} entries{} types {:?} e.g. {:?}", w.path, p.entries.len(), if p.truncated { " (stopped early)" } else { "" }, types, names);
        }
    }
    let _ = writeln!(o, "  parsed as a 2018-layout WAD: {parsed} of the first {}", wads.len().min(60));
    let interesting = ["r_ui", "hud", "mimir", "kratos", "player", "atreus", "weapon", "axe", "blades", "spear", "shield", "menu", "rune", "rage"];
    let mut picked: Vec<&&scan::FileInfo> = wads
        .iter()
        .filter(|w| {
            let n = w.path.to_lowercase();
            interesting.iter().any(|k| n.contains(k))
        })
        .collect();
    picked.truncate(40);
    let _ = writeln!(o, "\n== WAD heads (first 96 bytes, hex) of {} likely WADs", picked.len());
    for w in &picked {
        if late() {
            let _ = writeln!(o, "  (time budget reached)");
            break;
        }
        let _ = writeln!(o, "  {} size {} head {}", w.path, w.size, hex_head(&gow.join(&w.path), 96));
    }
    let mut by_size: Vec<&&scan::FileInfo> = wads.iter().collect();
    by_size.sort_by_key(|w| w.size);
    let _ = writeln!(o, "\n== WAD heads of the 8 smallest and 4 largest");
    for w in by_size.iter().take(8).chain(by_size.iter().rev().take(4)) {
        if late() {
            break;
        }
        let _ = writeln!(o, "  {} size {} head {}", w.path, w.size, hex_head(&gow.join(&w.path), 96));
    }
    // A second look inside one UI and one Mimir WAD: where the 4-byte values that look like counts and offsets sit.
    for key in ["r_ui.wad", "r_mimir00.wad"] {
        if let Some(w) = wads.iter().find(|w| w.path.to_lowercase().ends_with(key)) {
            if !late() {
                let _ = writeln!(o, "\n== {} first 512 bytes\n  {}", w.path, hex_head(&gow.join(&w.path), 512));
            }
        }
    }


    // ---- the LZ4-framed WADs: decompress the table and list what it names ---------------------------------------------------
    {
        let lz: Vec<&&scan::FileInfo> = wads.iter().filter(|w| crate::lz4::is_frame(&gow.join(&w.path))).collect();
        let _ = writeln!(o, "\n== LZ4-framed WADs: {} of {}", lz.len(), wads.len());
        let mut tiny: Vec<&&&scan::FileInfo> = lz.iter().collect();
        tiny.sort_by_key(|w| w.size);
        // A whole tiny WAD: the table, then what follows it (the data the table points at).
        for w in tiny.iter().take(2) {
            if let Ok((buf, info)) = wtoc_load(&gow.join(&w.path)) {
                let count = wtoc_count(&buf).unwrap_or(0);
                let end = (WTOC_FIRST + count * WTOC_REC).min(buf.len());
                let _ = writeln!(o, "  {} ({} bytes on disk, {} decompressed of {:?}): {} records; the {} bytes after the table:\n{}", w.path, w.size, buf.len(), info.content_size, count, buf.len() - end, hexdump(&buf[end..], 2048));
                let recs = wtoc_records(&buf);
                for (i, r) in recs.iter().enumerate() {
                    let _ = writeln!(o, "    #{i:<3} a={:04x} b={:04x} c={:08x} {:<34} tail {}", r.a, r.b, r.c, r.name, hex(&r.tail));
                }
                layout_check(&mut o, &buf);
            }
        }
        for key in ["r_ui.wad", "r_mimir00.wad"] {
            let Some(w) = lz.iter().find(|w| w.path.to_lowercase().ends_with(key)) else { continue };
            if late() {
                break;
            }
            match wtoc_load(&gow.join(&w.path)) {
                Ok((buf, info)) => {
                    let count = wtoc_count(&buf).unwrap_or(0);
                    let recs = wtoc_records(&buf);
                    let _ = writeln!(o, "\n  {} content {:?} bytes; header {}; {} of {count} records read ({} bytes)", w.path, info.content_size, hex(&buf[..buf.len().min(0x40)]), recs.len(), buf.len());
                    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                    for r in &recs {
                        let tag = r.name.split('_').next().unwrap_or("").to_string();
                        *kinds.entry(if tag.len() <= 5 && !tag.is_empty() { tag } else { "(other)".into() }).or_default() += 1;
                    }
                    let _ = writeln!(o, "    record name prefixes: {kinds:?}");
                    for (i, r) in recs.iter().enumerate().take(40) {
                        let _ = writeln!(o, "    #{i:<5} a={:04x} b={:04x} c={:08x} {:<44} tail {}", r.a, r.b, r.c, r.name, hex(&r.tail));
                    }
                    let words = ["hud", "health", "rage", "runic", "stun", "reticle", "crosshair", "btn", "button", "mimir", "leviathan", "chaos", "draupnir", "guardian", "shield", "axe", "spear", "blades", "portrait", "subtitle", "icon", "bar", "gauge", "meter"];
                    let mut shown = 0;
                    let _ = writeln!(o, "    records naming something the design sheets look for (up to 400):");
                    for (i, r) in recs.iter().enumerate() {
                        let n = r.name.to_lowercase();
                        if words.iter().any(|k| n.contains(k)) {
                            let _ = writeln!(o, "    #{i:<5} a={:04x} b={:04x} c={:08x} {:<56} tail {}", r.a, r.b, r.c, r.name, hex(&r.tail));
                            shown += 1;
                            if shown >= 400 {
                                break;
                            }
                        }
                    }
                    // Texture chunks, placed with the arena layout (see wtoc.rs): a parameter chunk (id 1, server 25,
                    // a 0x68-byte header then a GNF header) and a GPU chunk (id 29) of the same name.
                    if key == "r_ui.wad" {
                        if let Ok((big, _)) = crate::lz4::read_file_prefix(&gow.join(&w.path), 64 << 20) {
                            ui_textures(&mut o, &big);
                        }
                    }
                    // the bytes at the end of the table, where the data starts
                    let end = (WTOC_FIRST + count * WTOC_REC).min(buf.len());
                    let _ = writeln!(o, "    the 256 bytes at the end of the table (offset {end:#x}):\n{}", hexdump(&buf[end.saturating_sub(48)..(end + 208).min(buf.len())], 256));
                }
                Err(e) => {
                    let _ = writeln!(o, "  {}: {e:#}", w.path);
                }
            }
        }
        // Kratos himself (roadmap R2): his model is in r_heroa00.wad (MESH_heroa00_*, MG_heroa00_*, goProtoheroa00).
        if let Some(w) = lz.iter().find(|w| w.path.to_lowercase().ends_with("r_heroa00.wad")) {
            if !late() {
                if let Ok((big, _)) = crate::lz4::read_file_prefix(&gow.join(&w.path), 96 << 20) {
                    kratos_probe(&mut o, &w.path, w.size, &big);
                }
            }
        } else {
            let _ = writeln!(o, "\n  r_heroa00.wad (Kratos) not found among the LZ4 WADs");
        }
        // Which WAD names the things the design sheets need? Search every table.
        let words = ["hud", "health", "rage", "runic", "stun", "reticle", "crosshair", "btn", "button", "mimir", "leviathan", "chaos", "draupnir", "guardian"];
        let mut shown = 0usize;
        let mut scanned = 0usize;
        let _ = writeln!(o, "\n  tables that name a word the design sheets look for (first 6 names per WAD):");
        for w in &lz {
            if late() || shown > 1500 {
                let _ = writeln!(o, "  (stopped after {scanned} of {} WADs)", lz.len());
                break;
            }
            scanned += 1;
            let Ok((buf, _)) = wtoc_load(&gow.join(&w.path)) else { continue };
            let recs = wtoc_records(&buf);
            let hits: Vec<&WtocRec> = recs.iter().filter(|r| { let n = r.name.to_lowercase(); words.iter().any(|k| n.contains(k)) }).take(6).collect();
            if !hits.is_empty() {
                shown += hits.len();
                let _ = writeln!(o, "  {} ({} records): {}", w.path, recs.len(), hits.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
    }

    // ---- what the design sheets want ------------------------------------------------------------------------
    let catalog = crate::catalog::build_until(gow, &files, deadline);
    let _ = writeln!(o, "\n== Asset search against the catalog ({} named entries, {} notes)", catalog.entries.len(), catalog.notes.len());
    for n in catalog.notes.iter().take(5) {
        let _ = writeln!(o, "  note: {n}");
    }
    if catalog.notes.len() > 5 {
        let _ = writeln!(o, "  ... and {} more notes of the same kind", catalog.notes.len() - 5);
    }
    for r in reqs {
        let hits: Vec<_> = catalog.search(&r.search).take(3).collect();
        let total = catalog.search(&r.search).count();
        let _ = writeln!(o, "  {:<24} search `{}` -> {} match(es){}", r.id, r.search, total, if hits.is_empty() { String::new() } else { format!(": {}", hits.iter().map(|h| h.locator()).collect::<Vec<_>>().join("  ")) });
    }
    let named = catalog.entries.iter().filter(|e| matches!(e.kind, EntryKind::WadEntry { .. })).count();
    let _ = writeln!(o, "\n(WAD-named entries: {named}. Wwise media carry numeric ids only; map them in assets_map.json.)");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare::Converter;

    #[test]
    fn report_mentions_every_section_for_a_synthetic_install() {
        let root = std::env::temp_dir().join(format!("gowring-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/ui.wad"), wad::build(&[("tex_hud_health_frame", 0x80A1, vec![1; 20])])).unwrap();
        let (toc, pack) = audiopack::build_pack(&[(9, audiopack::fake_pcm_wem(480, 48000))]);
        std::fs::write(root.join("a/vo.audiopack.toc"), toc).unwrap();
        std::fs::write(root.join("a/vo.0.audiopack"), pack).unwrap();
        std::fs::write(root.join("a/Init.bnk"), bnk::build_test_bank()).unwrap();
        std::fs::write(root.join("a/broken.wad"), vec![0xCDu8; 300]).unwrap();
        let reqs = vec![AssetReq { id: "hud_health_frame".into(), search: "hud,health".into(), converter: Converter::DdsToPng, output: "x.png".into() }];
        let text = report(&root, None, &reqs, None);
        for needle in ["== Inventory", "== Audio packs: 1", "pcm", "== Wwise banks: 1", "events 1", "== WAD files: 2", "PARSED a/ui.wad", "== WAD heads", "cdcdcdcd", "hud_health_frame", "1 match"] {
            assert!(text.contains(needle), "missing `{needle}` in:\n{text}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lz4_wads_are_listed_by_record_name() {
        let root = std::env::temp_dir().join(format!("gowring-diag-lz4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("exec")).unwrap();
        let mut content = b"WTOC".to_vec();
        content.resize(WTOC_FIRST, 0);
        content[8] = 2; // two records
        for name in ["WAD_R_UI", "TXRX_HUD_HEALTHBAR"] {
            let mut rec = vec![0u8; WTOC_REC];
            rec[0] = 1;
            rec[8] = 0xaa;
            rec[11] = 0xba;
            rec[0x18..0x18 + name.len()].copy_from_slice(name.as_bytes());
            content.extend(rec);
        }
        std::fs::write(root.join("exec/r_ui.wad"), crate::lz4::frame_for_tests(&content, &[0x00], content.len() as u64)).unwrap();
        let text = report(&root, None, &[], None);
        assert!(text.contains("== LZ4-framed WADs: 1 of 1"), "{text}");
        assert!(text.contains("TXRX_HUD_HEALTHBAR"), "{text}");
        assert!(text.contains("2 records"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_folder_is_reported_not_panicked() {
        assert!(report(Path::new("/nope"), None, &[], None).contains("does not exist"));
    }
}

/// Does the arena layout account for a whole (small) WAD exactly? The strongest check that `wtoc::layout` is right.
fn layout_check(o: &mut String, buf: &[u8]) {
    let Some(mut es) = crate::wtoc::read(buf) else {
        let _ = writeln!(o, "    layout: table unreadable");
        return;
    };
    let start = (crate::wtoc::HEADER_LEN + es.len() * crate::wtoc::ENTRY_LEN) as u64;
    let end = crate::wtoc::layout(&mut es, start);
    let blocks = es.last().map(|e| e.block + 1).unwrap_or(0);
    let within = es.iter().filter(|e| e.id != crate::wtoc::PAD_CHUNK && e.file_off + e.length as u64 <= buf.len() as u64).count();
    let _ = writeln!(
        o,
        "    layout: {} chunks in {blocks} load blocks; data laid out to {end:#x}, file is {:#x} bytes ({}); {within} chunks fit inside the file",
        es.len(),
        buf.len(),
        if end == buf.len() as u64 { "EXACT MATCH" } else { "mismatch" }
    );
}

fn gnf_at(b: &[u8], off: usize) -> bool {
    b.get(off..off + 4).map(|m| m == b"GNF ").unwrap_or(false)
}

/// The HUD textures of r_ui.wad with their laid-out positions and first bytes (is there a GNF header where we expect?).
fn ui_textures(o: &mut String, big: &[u8]) {
    let Some(es) = crate::wtoc::read(big) else {
        let _ = writeln!(o, "    UI textures: table unreadable in the {} bytes read", big.len());
        return;
    };
    let words = ["sonhud_meter", "sonhud_icon_knockeddown", "stunicon_small", "toggleblockicon_", "toggleaimicon_", "ragedrain", "xbox_controller_buttonmasks", "worldinteract_rsbutton"];
    let tx: Vec<&crate::wtoc::Entry> = es.iter().filter(|e| e.name.starts_with("TX_")).collect();
    let gnf_hits = tx.iter().filter(|e| e.id == 1 && (gnf_at(big, e.file_off as usize + 0x68) || gnf_at(big, e.file_off as usize))).count();
    let _ = writeln!(o, "    UI textures: {} TX_ chunks; {} parameter chunks have a GNF header where expected (stream read: {} bytes)", tx.len(), gnf_hits, big.len());
    let mut ids: BTreeMap<(u16, u8, u8), usize> = BTreeMap::new();
    for e in &tx {
        *ids.entry((e.id, e.server, e.mem_type)).or_default() += 1;
    }
    let _ = writeln!(o, "    TX_ chunk kinds (id, server, arena) -> count: {ids:?}");
    let mut n = 0;
    for e in tx.iter().filter(|e| words.iter().any(|k| e.name.to_lowercase().contains(k))) {
        let off = e.file_off as usize;
        let head = big.get(off..off + 0x68 + 64).or_else(|| big.get(off..(off + 96).min(big.len()))).map(hex).unwrap_or_else(|| "(beyond what was read)".into());
        let _ = writeln!(o, "      {} id={} server={} arena={} len={:#x} align={} block={} mem_off={:#x} file_off={off:#x} gnf@0x68={} gnf@0={}\n        {head}", e.name, e.id, e.server, e.mem_type, e.length, e.align, e.block, e.mem_off, gnf_at(big, off + 0x68), gnf_at(big, off));
        n += 1;
        if n >= 16 {
            break;
        }
    }
}

/// Kratos's model chunks in r_heroa00.wad: what they are, where they lie, how they start.
fn kratos_probe(o: &mut String, path: &str, disk: u64, big: &[u8]) {
    let Some(es) = crate::wtoc::read(big) else {
        let _ = writeln!(o, "\n  {path}: table unreadable in the {} bytes read", big.len());
        return;
    };
    let _ = writeln!(o, "\n  {path} (Kratos; {disk} bytes on disk, {} decompressed bytes read): {} chunks", big.len(), es.len());
    let mut ids: BTreeMap<(u16, u8, u8), usize> = BTreeMap::new();
    for e in &es {
        *ids.entry((e.id, e.server, e.mem_type)).or_default() += 1;
    }
    let _ = writeln!(o, "    chunk kinds (id, server, arena) -> count: {ids:?}");
    let mut n = 0;
    for e in es.iter().filter(|e| e.name.contains("heroa00") && (e.name.starts_with("MESH_") || e.name.starts_with("MG_") || e.name.starts_with("goProto") || e.name.starts_with("ANM_"))) {
        let off = e.file_off as usize;
        let head = big.get(off..off + 48).map(hex).unwrap_or_else(|| "(beyond what was read)".into());
        let _ = writeln!(o, "      {:<40} id={} server={} arena={} len={:#x} block={} file_off={off:#x}: {head}", e.name, e.id, e.server, e.mem_type, e.length, e.block);
        n += 1;
        if n >= 40 {
            break;
        }
    }
    let anims = es.iter().filter(|e| e.name.starts_with("ANM_")).count();
    let _ = writeln!(o, "    ANM_ chunks in this WAD: {anims}");
}

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

    // ---- what the design sheets want ------------------------------------------------------------------------
    let catalog = crate::catalog::build_until(gow, &files, deadline);
    let _ = writeln!(o, "\n== Asset search against the catalog ({} named entries, {} notes)", catalog.entries.len(), catalog.notes.len());
    for n in catalog.notes.iter().take(20) {
        let _ = writeln!(o, "  note: {n}");
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
    fn missing_folder_is_reported_not_panicked() {
        assert!(report(Path::new("/nope"), None, &[], None).contains("does not exist"));
    }
}

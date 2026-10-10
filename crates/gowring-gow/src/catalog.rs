//! What can be found in the install by name: WAD entries, loose DDS files and audiopack media.

use crate::audiopack;
use crate::scan::{FileInfo, FileKind};
use crate::wad;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntryKind {
    WadEntry { wad: String, kind: u16, size: u32, offset: u64 },
    Dds { file: String, size: u64 },
    Wem { pack: String, id: u32, size: u32, offset: u32 },
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogEntry {
    /// The string the asset search matches against (entry name or path).
    pub name: String,
    pub kind: EntryKind,
}

impl CatalogEntry {
    /// The locator string used in `assets_map.json` overrides.
    pub fn locator(&self) -> String {
        match &self.kind {
            EntryKind::WadEntry { wad, .. } => format!("wad:{wad}#{}", self.name),
            EntryKind::Dds { file, .. } => format!("dds:{file}"),
            EntryKind::Wem { pack, id, .. } => format!("wem:{pack}#{id}"),
        }
    }
}

#[derive(Default, Debug, Serialize)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
    /// Files that looked like a known container but failed to parse (kept for the diagnose report).
    pub notes: Vec<String>,
}

/// Cap on how many audiopack media entries are listed in the catalog (they have no names).
const MAX_WEM_ENTRIES: usize = 300_000;

pub fn build(root: &Path, files: &[FileInfo]) -> Catalog {
    let mut c = Catalog::default();
    let mut wem_count = 0usize;
    for f in files {
        let full = root.join(&f.path);
        match f.kind {
            FileKind::Wad => match wad::parse_file(&full) {
                Ok(w) => {
                    if w.truncated {
                        c.notes.push(format!("{}: parsed {} WAD entries, then hit data it does not understand", f.path, w.entries.len()));
                    }
                    for e in w.entries {
                        c.entries.push(CatalogEntry { name: e.name, kind: EntryKind::WadEntry { wad: f.path.clone(), kind: e.kind, size: e.size, offset: e.offset } });
                    }
                }
                Err(e) => c.notes.push(format!("{}: not parsed as a GoW WAD ({e})", f.path)),
            },
            FileKind::Dds => c.entries.push(CatalogEntry { name: f.path.clone(), kind: EntryKind::Dds { file: f.path.clone(), size: f.size } }),
            FileKind::AudioToc => match std::fs::read(&full).map_err(anyhow::Error::from).and_then(|b| audiopack::parse_toc(&b)) {
                Ok(t) => {
                    let pack = audiopack::pack_path_for(Path::new(&f.path)).to_string_lossy().replace('\\', "/");
                    for e in t.entries {
                        if wem_count >= MAX_WEM_ENTRIES {
                            break;
                        }
                        wem_count += 1;
                        c.entries.push(CatalogEntry { name: format!("{pack}#{}", e.id), kind: EntryKind::Wem { pack: pack.clone(), id: e.id, size: e.size, offset: e.offset } });
                    }
                }
                Err(e) => c.notes.push(format!("{}: audiopack TOC not understood ({e})", f.path)),
            },
            _ => {}
        }
    }
    c
}

/// `a,b|c` = entries whose name contains both `a` and `b`, or contains `c` (case-insensitive).
pub fn matches(expr: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    expr.split('|').any(|alt| {
        let terms: Vec<_> = alt.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
        !terms.is_empty() && terms.iter().all(|t| name.contains(t))
    })
}

impl Catalog {
    pub fn search<'a>(&'a self, expr: &'a str) -> impl Iterator<Item = &'a CatalogEntry> {
        self.entries.iter().filter(move |e| matches(expr, &e.name))
    }

    pub fn by_locator(&self, loc: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.locator() == loc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_expressions() {
        assert!(matches("hud,health,frame|ui,kratos", "Tex_HUD_Health_Frame_01"));
        assert!(matches("hud,health,frame|ui,kratos", "ui_kratos_icon"));
        assert!(!matches("hud,health,frame", "hud_health"));
        assert!(!matches("", "anything"));
    }

    #[test]
    fn builds_from_a_synthetic_install() {
        let root = std::env::temp_dir().join(format!("gowring-cat-{}", std::process::id()));
        std::fs::create_dir_all(root.join("exec/wad")).unwrap();
        std::fs::create_dir_all(root.join("audio")).unwrap();
        std::fs::write(root.join("exec/wad/ui.wad"), wad::build(&[("tex_hud_health_frame", 0x80A1, crate::dds::build_test_bc1())])).unwrap();
        let (toc, pack) = audiopack::build_pack(&[(5, audiopack::fake_pcm_wem(100, 48000))]);
        std::fs::write(root.join("audio/vo_en.audiopack.toc"), toc).unwrap();
        std::fs::write(root.join("audio/vo_en.0.audiopack"), pack).unwrap();
        let files = crate::scan::walk(&root, 6);
        let c = build(&root, &files);
        assert!(c.search("hud,health").next().is_some());
        let wem = c.entries.iter().find(|e| matches!(e.kind, EntryKind::Wem { id: 5, .. })).unwrap();
        assert_eq!(wem.locator(), "wem:audio/vo_en.0.audiopack#5");
        let _ = std::fs::remove_dir_all(&root);
    }
}

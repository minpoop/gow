//! Belt and braces for the player's real Elden Ring saves: the mod plays on its own save file, and
//! before every Play the launcher copies the real ones (once per change) into a backup folder.

use crate::config::Ctx;
use std::path::{Path, PathBuf};

fn newest(dir: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(dir).ok()?.modified().ok()
}

/// `%APPDATA%/EldenRing/<steam id>/ER0000.sl2` files.
fn real_saves() -> Vec<PathBuf> {
    let Some(appdata) = std::env::var_os("APPDATA") else { return vec![] };
    let root = PathBuf::from(appdata).join("EldenRing");
    let Ok(rd) = std::fs::read_dir(&root) else { return vec![] };
    let mut v = Vec::new();
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            for name in ["ER0000.sl2", "ER0000.sl2.bak"] {
                let f = p.join(name);
                if f.is_file() {
                    v.push(f);
                }
            }
        }
    }
    v
}

pub fn backup(ctx: &Ctx) -> Vec<String> {
    let mut notes = Vec::new();
    let saves = real_saves();
    if saves.is_empty() {
        notes.push("saves: no Elden Ring save found to back up".into());
        return notes;
    }
    let root = ctx.local.join("save-backups");
    let marker = root.join(".last");
    let latest = saves.iter().filter_map(|p| newest(p)).max();
    let last_done = std::fs::metadata(&marker).ok().and_then(|m| m.modified().ok());
    if let (Some(l), Some(d)) = (latest, last_done) {
        if d >= l {
            notes.push("saves: backup already up to date".into());
            return notes;
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let dest = root.join(format!("{stamp}"));
    for s in &saves {
        let rel = s.parent().and_then(|p| p.file_name()).map(PathBuf::from).unwrap_or_default();
        let to = dest.join(rel).join(s.file_name().unwrap());
        if let Some(p) = to.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        match std::fs::copy(s, &to) {
            Ok(n) => notes.push(format!("saves: backed up {} ({n} bytes)", s.display())),
            Err(e) => notes.push(format!("saves: could not back up {}: {e}", s.display())),
        }
    }
    let _ = std::fs::write(&marker, stamp.to_string());
    // keep the three newest backups
    if let Ok(rd) = std::fs::read_dir(&root) {
        let mut dirs: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        while dirs.len() > 3 {
            let old = dirs.remove(0);
            let _ = std::fs::remove_dir_all(old);
        }
    }
    notes
}

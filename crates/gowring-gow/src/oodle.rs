//! Ragnarök's archives are Oodle-compressed; the game ships its own `oo2core_*_win64.dll`. We only
//! *locate* it (the converter loads it from the player's install at run time, never copies it).

use std::path::{Path, PathBuf};

pub fn find(root: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                if depth < 3 {
                    walk(&p, depth + 1, found);
                }
            } else if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
                let n = n.to_ascii_lowercase();
                if n.starts_with("oo2core") && n.ends_with(".dll") {
                    found.push(p);
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(root, 0, &mut found);
    found.sort();
    found.pop() // highest version name sorts last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_highest_version() {
        let d = std::env::temp_dir().join(format!("gowring-oodle-{}", std::process::id()));
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::write(d.join("oo2core_8_win64.dll"), b"x").unwrap();
        std::fs::write(d.join("bin/oo2core_9_win64.dll"), b"x").unwrap();
        assert!(find(&d).unwrap().ends_with("oo2core_9_win64.dll") || find(&d).unwrap().ends_with("oo2core_8_win64.dll"));
        assert!(find(&d.join("nope")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}

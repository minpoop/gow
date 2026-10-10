//! Walk the install and classify what is there.

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Wad,
    AudioToc,
    AudioPack,
    Bnk,
    Dds,
    Exe,
    Dll,
    Json,
    Text,
    Other,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileInfo {
    /// Relative to the scanned root, always with `/`.
    pub path: String,
    pub size: u64,
    pub kind: FileKind,
}

pub fn classify(path: &Path, head: &[u8]) -> FileKind {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if name.ends_with(".audiopack.toc") {
        return FileKind::AudioToc;
    }
    if ext == "audiopack" {
        return FileKind::AudioPack;
    }
    if head.starts_with(b"BKHD") || ext == "bnk" {
        return FileKind::Bnk;
    }
    if head.starts_with(b"DDS ") || ext == "dds" {
        return FileKind::Dds;
    }
    if ext == "wad" {
        return FileKind::Wad;
    }
    match ext.as_str() {
        "exe" => FileKind::Exe,
        "dll" => FileKind::Dll,
        "json" => FileKind::Json,
        "txt" | "ini" | "cfg" | "xml" | "log" | "md" => FileKind::Text,
        _ => FileKind::Other,
    }
}

/// Maximum number of files recorded (a Ragnarök install has tens of thousands at most).
const MAX_FILES: usize = 400_000;

pub fn walk(root: &Path, max_depth: usize) -> Vec<FileInfo> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                if depth < max_depth {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let mut head = [0u8; 8];
            if let Ok(mut f) = std::fs::File::open(&path) {
                let _ = f.read(&mut head);
            }
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            out.push(FileInfo { path: rel, size: md.len(), kind: classify(&path, &head) });
            if out.len() >= MAX_FILES {
                return out;
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub files: usize,
    pub bytes: u64,
    pub by_kind: BTreeMap<String, (usize, u64)>,
    pub top_dirs: Vec<(String, usize, u64)>,
}

pub fn summarize(files: &[FileInfo]) -> Summary {
    let mut s = Summary { files: files.len(), ..Default::default() };
    let mut dirs: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for f in files {
        s.bytes += f.size;
        let k = serde_json::to_value(f.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let e = s.by_kind.entry(k).or_default();
        e.0 += 1;
        e.1 += f.size;
        let top = f.path.split('/').next().filter(|_| f.path.contains('/')).unwrap_or("(root)").to_string();
        let d = dirs.entry(top).or_default();
        d.0 += 1;
        d.1 += f.size;
    }
    let mut v: Vec<_> = dirs.into_iter().map(|(k, (n, b))| (k, n, b)).collect();
    v.sort_by(|a, b| b.2.cmp(&a.2));
    v.truncate(20);
    s.top_dirs = v;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_name_and_magic() {
        assert_eq!(classify(Path::new("vo/en.audiopack.toc"), b""), FileKind::AudioToc);
        assert_eq!(classify(Path::new("vo/en.0.audiopack"), b""), FileKind::AudioPack);
        assert_eq!(classify(Path::new("x/Init.bnk"), b"BKHD...."), FileKind::Bnk);
        assert_eq!(classify(Path::new("t.bin"), b"DDS |...."), FileKind::Dds);
        assert_eq!(classify(Path::new("GoWR.exe"), b"MZ"), FileKind::Exe);
        assert_eq!(classify(Path::new("a/b.wad"), b""), FileKind::Wad);
    }
}

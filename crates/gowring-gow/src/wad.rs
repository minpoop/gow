//! The God of War WAD container (layout per the community GOWTool, which documents the 2018 PC
//! format and notes Ragnarök PC reuses the texture side of it): a run of entries, each
//! `{group:u16, type:u16, size:u32, 0x10 skip, name:[u8;0x38], 0x10 skip, data[size]}` padded to
//! 16 bytes. Entries carry readable names, which is what the asset search matches against.

use anyhow::{bail, Result};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct WadEntry {
    pub name: String,
    pub group: u16,
    pub kind: u16,
    pub size: u32,
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Wad {
    pub entries: Vec<WadEntry>,
    /// True when parsing stopped before the end of the file (unknown layout or trailing data).
    pub truncated: bool,
}

const HEADER: usize = 2 + 2 + 4 + 0x10 + 0x38 + 0x10;

fn clean_name(raw: &[u8]) -> Option<String> {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    let s = &raw[..end];
    if s.is_empty() || !s.iter().all(|b| (0x20..0x7f).contains(b)) {
        return None;
    }
    Some(String::from_utf8_lossy(s).into_owned())
}

/// Parse from a slice.
pub fn parse(bytes: &[u8]) -> Result<Wad> {
    parse_with(bytes.len() as u64, |off, n| bytes.get(off as usize..off as usize + n).map(<[u8]>::to_vec))
}

/// Parse from a file without reading entry data (only the 0x70-byte headers).
pub fn parse_file(path: &std::path::Path) -> Result<Wad> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    parse_with(len, |off, n| {
        f.seek(SeekFrom::Start(off)).ok()?;
        let mut buf = vec![0u8; n];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    })
}

const MAX_ENTRIES: usize = 2_000_000;

pub fn parse_with(len: u64, mut read: impl FnMut(u64, usize) -> Option<Vec<u8>>) -> Result<Wad> {
    let mut at = 0u64;
    let mut entries = Vec::new();
    let mut truncated = false;
    while at + HEADER as u64 <= len && entries.len() < MAX_ENTRIES {
        let Some(h) = read(at, HEADER) else {
            truncated = true;
            break;
        };
        let group = u16::from_le_bytes([h[0], h[1]]);
        let kind = u16::from_le_bytes([h[2], h[3]]);
        let size = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
        let Some(name) = clean_name(&h[8 + 0x10..8 + 0x10 + 0x38]) else {
            if entries.is_empty() {
                bail!("first entry has no readable name: not a GoW WAD");
            }
            truncated = true;
            break;
        };
        let data = at + HEADER as u64;
        if size as u64 > len - data {
            if entries.is_empty() {
                bail!("first entry overruns the file");
            }
            truncated = true;
            break;
        }
        entries.push(WadEntry { name, group, kind, size, offset: data });
        at = (data + size as u64 + 15) & !15;
    }
    if entries.is_empty() {
        bail!("no entries");
    }
    if at < len && at + HEADER as u64 > len && len - at > 15 {
        truncated = true;
    }
    Ok(Wad { entries, truncated })
}

/// Build a WAD from `(name, type, data)` for tests.
pub fn build(items: &[(&str, u16, Vec<u8>)]) -> Vec<u8> {
    let mut v = Vec::new();
    for (name, kind, data) in items {
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&kind.to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&[0u8; 0x10]);
        let mut n = [0u8; 0x38];
        n[..name.len()].copy_from_slice(name.as_bytes());
        v.extend_from_slice(&n);
        v.extend_from_slice(&[0u8; 0x10]);
        v.extend_from_slice(data);
        while v.len() % 16 != 0 {
            v.push(0);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let w = build(&[("tex_hud_health_frame", 0x80A1, vec![1, 2, 3]), ("rig_kratos", 0x3D, vec![9; 40])]);
        let p = parse(&w).unwrap();
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].name, "tex_hud_health_frame");
        assert_eq!(p.entries[0].kind, 0x80A1);
        assert_eq!(&w[p.entries[1].offset as usize..][..2], &[9, 9]);
        assert!(!p.truncated);
    }

    #[test]
    fn garbage_is_not_a_wad() {
        assert!(parse(&vec![0xAAu8; 400]).is_err());
        assert!(parse(b"short").is_err());
    }

    #[test]
    fn stops_cleanly_at_unknown_trailing_data() {
        let mut w = build(&[("a", 1, vec![0; 8])]);
        w.extend_from_slice(&[0xFFu8; 200]);
        let p = parse(&w).unwrap();
        assert_eq!(p.entries.len(), 1);
        assert!(p.truncated);
    }
}

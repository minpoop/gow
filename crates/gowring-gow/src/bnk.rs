//! Wwise soundbank (`.bnk`) chunk inventory: version, embedded media ids, object counts and event
//! ids. Deliberately shallow: object layouts differ between Wwise versions, so the diagnose report
//! records what is there and the mapping from events to voice lines is derived per install.

use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Default)]
pub struct BnkInfo {
    pub version: u32,
    pub bank_id: u32,
    pub embedded_media: Vec<(u32, u32, u32)>, // id, offset, size
    pub hirc_objects: BTreeMap<u8, usize>,
    pub event_ids: Vec<u32>,
}

pub fn hirc_type_name(t: u8) -> &'static str {
    match t {
        1 => "settings",
        2 => "sound",
        3 => "action",
        4 => "event",
        5 => "random/sequence container",
        6 => "switch container",
        7 => "actor-mixer",
        8 => "audio bus",
        9 => "blend container",
        10 => "music segment",
        11 => "music track",
        12 => "music switch",
        13 => "music playlist",
        14 => "attenuation",
        15 => "dialogue event",
        18 => "fx",
        _ => "other",
    }
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn parse(b: &[u8]) -> Result<BnkInfo> {
    if b.len() < 8 || &b[0..4] != b"BKHD" {
        bail!("not a Wwise soundbank");
    }
    let mut info = BnkInfo::default();
    let mut at = 0usize;
    while at + 8 <= b.len() {
        let tag = &b[at..at + 4];
        let len = u32le(b, at + 4).unwrap_or(0) as usize;
        let body = at + 8;
        if body + len > b.len() {
            break;
        }
        match tag {
            b"BKHD" if len >= 8 => {
                info.version = u32le(b, body).unwrap_or(0);
                info.bank_id = u32le(b, body + 4).unwrap_or(0);
            }
            b"DIDX" => {
                for i in 0..len / 12 {
                    let o = body + i * 12;
                    if let (Some(id), Some(off), Some(sz)) = (u32le(b, o), u32le(b, o + 4), u32le(b, o + 8)) {
                        info.embedded_media.push((id, off, sz));
                    }
                }
            }
            b"HIRC" if len >= 4 => {
                let count = u32le(b, body).unwrap_or(0) as usize;
                let mut o = body + 4;
                for _ in 0..count {
                    if o + 9 > body + len {
                        break;
                    }
                    let t = b[o];
                    let olen = u32le(b, o + 1).unwrap_or(0) as usize;
                    let id = u32le(b, o + 5).unwrap_or(0);
                    *info.hirc_objects.entry(t).or_default() += 1;
                    if t == 4 {
                        info.event_ids.push(id);
                    }
                    o += 5 + olen;
                }
            }
            _ => {}
        }
        at = body + len;
    }
    Ok(info)
}

/// A tiny bank for tests: BKHD + DIDX(2 media) + HIRC(1 event, 1 sound).
pub fn build_test_bank() -> Vec<u8> {
    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = tag.to_vec();
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(body);
        v
    }
    let mut bkhd = Vec::new();
    bkhd.extend_from_slice(&145u32.to_le_bytes());
    bkhd.extend_from_slice(&0xDEADu32.to_le_bytes());
    let mut didx = Vec::new();
    for (id, off, sz) in [(10u32, 0u32, 100u32), (11, 100, 50)] {
        didx.extend_from_slice(&id.to_le_bytes());
        didx.extend_from_slice(&off.to_le_bytes());
        didx.extend_from_slice(&sz.to_le_bytes());
    }
    let mut hirc = Vec::new();
    hirc.extend_from_slice(&2u32.to_le_bytes());
    for (t, id) in [(4u8, 0x1234u32), (2u8, 0x77u32)] {
        hirc.push(t);
        hirc.extend_from_slice(&8u32.to_le_bytes());
        hirc.extend_from_slice(&id.to_le_bytes());
        hirc.extend_from_slice(&[0u8; 4]);
    }
    let mut v = chunk(b"BKHD", &bkhd);
    v.extend(chunk(b"DIDX", &didx));
    v.extend(chunk(b"HIRC", &hirc));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_chunks() {
        let i = parse(&build_test_bank()).unwrap();
        assert_eq!((i.version, i.bank_id), (145, 0xDEAD));
        assert_eq!(i.embedded_media.len(), 2);
        assert_eq!(i.event_ids, vec![0x1234]);
        assert_eq!(i.hirc_objects.get(&2), Some(&1));
        assert!(parse(b"nope....").is_err());
    }
}

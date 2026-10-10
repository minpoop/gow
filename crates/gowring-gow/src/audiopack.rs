//! Ragnarök's audio containers: `<name>.audiopack.toc` + `<name>.0.audiopack` hold Wwise `.wem`
//! media (format per the MIT-licensed gowr_soundtool by iArtorias: header
//! `APAK`,version,count,parts; then `{file_id,size,offset}` u32 triples).

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const TOC_MAGIC: u32 = 0x4b41_5041; // "APAK" little-endian

#[derive(Clone, Debug, Serialize)]
pub struct TocEntry {
    pub id: u32,
    pub size: u32,
    pub offset: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Toc {
    pub version: u32,
    pub parts: u32,
    pub entries: Vec<TocEntry>,
}

fn u32le(b: &[u8], at: usize) -> Result<u32> {
    let s = b.get(at..at + 4).context("truncated")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn parse_toc(bytes: &[u8]) -> Result<Toc> {
    if bytes.len() < 16 {
        bail!("TOC too small ({} bytes)", bytes.len());
    }
    if u32le(bytes, 0)? != TOC_MAGIC {
        bail!("not an audiopack TOC (magic {:08x})", u32le(bytes, 0)?);
    }
    let (version, count, parts) = (u32le(bytes, 4)?, u32le(bytes, 8)?, u32le(bytes, 12)?);
    if count == 0 || (count as usize) > (bytes.len() - 16) / 12 {
        bail!("bad entry count {count} for {} bytes", bytes.len());
    }
    let mut entries = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let at = 16 + i * 12;
        entries.push(TocEntry { id: u32le(bytes, at)?, size: u32le(bytes, at + 4)?, offset: u32le(bytes, at + 8)? });
    }
    Ok(Toc { version, parts, entries })
}

/// `dir/X.audiopack.toc` → `dir/X.0.audiopack`.
pub fn pack_path_for(toc: &Path) -> PathBuf {
    let name = toc.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name.strip_suffix(".audiopack.toc").unwrap_or(name);
    toc.with_file_name(format!("{stem}.0.audiopack"))
}

pub fn read_entry(pack: &mut (impl Read + Seek), e: &TocEntry) -> Result<Vec<u8>> {
    pack.seek(SeekFrom::Start(e.offset as u64))?;
    let mut buf = vec![0u8; e.size as usize];
    pack.read_exact(&mut buf).with_context(|| format!("entry {} ({} bytes @ {})", e.id, e.size, e.offset))?;
    Ok(buf)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct WemInfo {
    /// `fmt ` format tag: 0xFFFF Wwise Vorbis, 0x0001 PCM, 0x0002 Wwise ADPCM, 0x3039/0x3040/0x3041 Wwise Opus.
    pub codec: u16,
    pub codec_name: &'static str,
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_sec: u32,
    pub data_len: u32,
    /// Estimated from the data size and byte rate (exact for PCM; indicative for the rest).
    pub approx_ms: u32,
}

pub fn codec_name(tag: u16) -> &'static str {
    match tag {
        0x0001 => "pcm",
        0x0002 => "wwise-adpcm",
        0x0011 => "ima-adpcm",
        0xFFFF => "wwise-vorbis",
        0x3039 | 0x3040 | 0x3041 => "wwise-opus",
        0xFFFE => "extensible",
        _ => "unknown",
    }
}

/// Parse a `.wem` (RIFF/WAVE) header.
pub fn wem_info(b: &[u8]) -> Option<WemInfo> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut at, mut fmt, mut data_len) = (12usize, None, 0u32);
    while at + 8 <= b.len() {
        let tag = &b[at..at + 4];
        let len = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]) as usize;
        let body = at + 8;
        if tag == b"fmt " && body + 16 <= b.len() {
            let r = |o: usize| u16::from_le_bytes([b[body + o], b[body + o + 1]]);
            let d = |o: usize| u32::from_le_bytes([b[body + o], b[body + o + 1], b[body + o + 2], b[body + o + 3]]);
            fmt = Some((r(0), r(2), d(4), d(8)));
        } else if tag == b"data" {
            data_len = len as u32;
            break;
        }
        at = body + len + (len & 1);
    }
    let (codec, channels, sample_rate, avg) = fmt?;
    let approx_ms = if avg > 0 { (data_len as u64 * 1000 / avg as u64) as u32 } else { 0 };
    Some(WemInfo { codec, codec_name: codec_name(codec), channels, sample_rate, avg_bytes_per_sec: avg, data_len, approx_ms })
}

/// Build a TOC+pack pair from media blobs (used by tests and the synthetic fixtures).
pub fn build_pack(items: &[(u32, Vec<u8>)]) -> (Vec<u8>, Vec<u8>) {
    let mut toc = Vec::new();
    toc.extend_from_slice(&TOC_MAGIC.to_le_bytes());
    toc.extend_from_slice(&1u32.to_le_bytes());
    toc.extend_from_slice(&(items.len() as u32).to_le_bytes());
    toc.extend_from_slice(&1u32.to_le_bytes());
    let mut pack = Vec::new();
    for (id, blob) in items {
        toc.extend_from_slice(&id.to_le_bytes());
        toc.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        toc.extend_from_slice(&(pack.len() as u32).to_le_bytes());
        pack.extend_from_slice(blob);
    }
    (toc, pack)
}

/// A minimal valid PCM `.wem` for tests.
pub fn fake_pcm_wem(samples: usize, rate: u32) -> Vec<u8> {
    let data_len = (samples * 2) as u32;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v.extend((0..samples).flat_map(|i| (((i % 50) as i16 - 25) * 300).to_le_bytes()));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn toc_round_trip_and_extract() {
        let a = fake_pcm_wem(4800, 48000);
        let b = fake_pcm_wem(24000, 48000);
        let (toc, pack) = build_pack(&[(111, a.clone()), (222, b.clone())]);
        let t = parse_toc(&toc).unwrap();
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.entries[1].id, 222);
        let mut cur = Cursor::new(pack);
        assert_eq!(read_entry(&mut cur, &t.entries[0]).unwrap(), a);
        let got = read_entry(&mut cur, &t.entries[1]).unwrap();
        let info = wem_info(&got).unwrap();
        assert_eq!((info.codec_name, info.channels, info.sample_rate, info.approx_ms), ("pcm", 1, 48000, 500));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_toc(b"nope").is_err());
        let mut bad = vec![0u8; 32];
        bad[0..4].copy_from_slice(&TOC_MAGIC.to_le_bytes());
        bad[8..12].copy_from_slice(&1000u32.to_le_bytes());
        assert!(parse_toc(&bad).is_err());
        assert!(wem_info(b"RIFFxxxxWAVEjunk").is_none());
    }

    #[test]
    fn pack_path_naming() {
        let p = pack_path_for(Path::new("/g/audio/vo_en.audiopack.toc"));
        assert_eq!(p, PathBuf::from("/g/audio/vo_en.0.audiopack"));
    }
}

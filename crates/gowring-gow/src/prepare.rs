//! `--prepare`: resolve every asset the design sheets ask for against the player's install and
//! convert it into the local cache. Reports per asset what happened; never invents a substitute.

use crate::catalog::{Catalog, CatalogEntry, EntryKind};
use crate::{audiopack, convert, dds, scan};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Converter {
    DdsToPng,
    WemToWav,
    WemToWavSet,
}

#[derive(Clone, Debug)]
pub struct AssetReq {
    pub id: String,
    pub search: String,
    pub converter: Converter,
    /// File (or, for sets, directory ending in `/`) under the cache.
    pub output: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AssetState {
    Converted { files: Vec<String>, source: String },
    NotFound,
    Failed { reason: String },
}

#[derive(Debug, Serialize, Default)]
pub struct PrepareReport {
    pub per_asset: BTreeMap<String, AssetState>,
    pub catalog_entries: usize,
    pub notes: Vec<String>,
    pub oodle: Option<String>,
    pub vgmstream: Option<String>,
}

impl PrepareReport {
    pub fn converted(&self) -> usize {
        self.per_asset.values().filter(|s| matches!(s, AssetState::Converted { .. })).count()
    }
    pub fn missing(&self) -> Vec<&str> {
        self.per_asset.iter().filter(|(_, s)| !matches!(s, AssetState::Converted { .. })).map(|(k, _)| k.as_str()).collect()
    }
}

#[derive(Default, Clone)]
pub struct Tools {
    pub vgmstream: Option<PathBuf>,
    /// Wall-clock budget for scanning and cataloguing the install (None = unlimited).
    pub budget: Option<std::time::Duration>,
}

fn read_range(path: &Path, offset: u64, size: u64) -> Result<Vec<u8>> {
    let mut f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; size as usize];
    f.read_exact(&mut buf).with_context(|| format!("{} bytes at {offset} in {}", size, path.display()))?;
    Ok(buf)
}

/// Bytes of a catalog entry.
pub fn entry_bytes(root: &Path, e: &CatalogEntry) -> Result<Vec<u8>> {
    match &e.kind {
        EntryKind::WadEntry { wad, size, offset, .. } => read_range(&root.join(wad), *offset, *size as u64),
        EntryKind::Dds { file, size } => read_range(&root.join(file), 0, *size),
        EntryKind::Wem { pack, size, offset, .. } => read_range(&root.join(pack), *offset as u64, *size as u64),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().take(16).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn convert_one(root: &Path, e: &CatalogEntry, req: &AssetReq, cache: &Path, tools: &Tools, out_name: &str) -> Result<String> {
    let bytes = entry_bytes(root, e)?;
    let out = cache.join(out_name);
    if let Some(p) = out.parent() {
        std::fs::create_dir_all(p)?;
    }
    match req.converter {
        Converter::DdsToPng => {
            if !bytes.starts_with(b"DDS ") {
                bail!("texture payload is not DDS (first bytes: {}; entry {})", hex(&bytes), e.locator());
            }
            std::fs::write(&out, dds::dds_to_png(&bytes)?)?;
        }
        Converter::WemToWav | Converter::WemToWavSet => {
            let info = audiopack::wem_info(&bytes).ok_or_else(|| anyhow!("media is not a RIFF/WEM (first bytes: {})", hex(&bytes)))?;
            if let Some(v) = &tools.vgmstream {
                convert::wem_to_wav(v, &bytes, &cache.join("tmp"), &out)?;
            } else if let Some(wav) = convert::pcm_wem_to_wav(&bytes) {
                std::fs::write(&out, wav)?;
            } else {
                bail!("no decoder for {} audio and vgmstream-cli is not available", info.codec_name);
            }
        }
    }
    Ok(out_name.to_string())
}

/// Resolve and convert every requested asset.
pub fn prepare(gow: &Path, reqs: &[AssetReq], overrides: &serde_json::Value, cache: &Path, tools: &Tools) -> Result<PrepareReport> {
    if !gow.is_dir() {
        bail!("God of War Ragnarök folder not found: {}", gow.display());
    }
    std::fs::create_dir_all(cache)?;
    let started = std::time::Instant::now();
    let deadline = tools.budget.map(|b| started + b);
    let (files, cut) = scan::walk_until(gow, 8, deadline.map(|d| started + (d - started) / 2));
    let catalog: Catalog = crate::catalog::build_until(gow, &files, deadline);
    let mut report = PrepareReport {
        catalog_entries: catalog.entries.len(),
        notes: {
            let mut n = catalog.notes.clone();
            if n.len() > 40 {
                let more = n.len() - 40;
                n.truncate(40);
                n.push(format!("... and {more} more like these"));
            }
            n.push(format!("scanned {} files in {:.1}s{}", files.len(), started.elapsed().as_secs_f32(), if cut { " (stopped at the time budget)" } else { "" }));
            n
        },
        oodle: crate::oodle::find(gow).map(|p| p.to_string_lossy().into_owned()),
        vgmstream: tools.vgmstream.as_ref().map(|p| p.to_string_lossy().into_owned()),
        ..Default::default()
    };

    for req in reqs {
        // Overrides win; otherwise the sheet's search terms.
        let locators: Vec<String> = overrides
            .get(&req.id)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let mut sources: Vec<&CatalogEntry> = if locators.is_empty() {
            catalog.search(&req.search).collect()
        } else {
            locators.iter().filter_map(|l| catalog.by_locator(l)).collect()
        };
        // A single-file asset takes the first match; a set takes them all.
        if req.converter != Converter::WemToWavSet {
            sources.truncate(1);
        }
        if sources.is_empty() {
            report.per_asset.insert(req.id.clone(), AssetState::NotFound);
            continue;
        }
        let mut done = Vec::new();
        let mut failure = None;
        for (i, e) in sources.iter().enumerate() {
            let out_name = if req.converter == Converter::WemToWavSet {
                let id = match &e.kind {
                    EntryKind::Wem { id, .. } => *id,
                    _ => i as u32,
                };
                format!("{}{id}.wav", req.output)
            } else {
                req.output.clone()
            };
            match convert_one(gow, e, req, cache, tools, &out_name) {
                Ok(f) => done.push(f),
                Err(err) => {
                    failure = Some(format!("{err:#}"));
                    if req.converter != Converter::WemToWavSet {
                        break;
                    }
                }
            }
        }
        let state = if done.is_empty() {
            AssetState::Failed { reason: failure.unwrap_or_else(|| "nothing converted".into()) }
        } else {
            AssetState::Converted { files: done, source: sources[0].locator() }
        };
        report.per_asset.insert(req.id.clone(), state);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn install() -> PathBuf {
        let root = std::env::temp_dir().join(format!("gowring-prep-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("exec")).unwrap();
        std::fs::create_dir_all(root.join("audio")).unwrap();
        std::fs::write(root.join("exec/ui.wad"), crate::wad::build(&[("tex_hud_health_frame", 0x80A1, dds::build_test_bc1()), ("tex_hud_rage_frame", 0x80A1, vec![7u8; 64])])).unwrap();
        let (toc, pack) = audiopack::build_pack(&[(1, audiopack::fake_pcm_wem(480, 48000)), (2, audiopack::fake_pcm_wem(960, 48000))]);
        std::fs::write(root.join("audio/vo.audiopack.toc"), toc).unwrap();
        std::fs::write(root.join("audio/vo.0.audiopack"), pack).unwrap();
        root
    }

    fn req(id: &str, search: &str, c: Converter, out: &str) -> AssetReq {
        AssetReq { id: id.into(), search: search.into(), converter: c, output: out.into() }
    }

    #[test]
    fn converts_found_reports_missing_and_failed() {
        let root = install();
        let cache = root.join("cache");
        let reqs = vec![
            req("hud_health_frame", "hud,health,frame", Converter::DdsToPng, "hud/health_frame.png"),
            req("hud_rage_frame", "hud,rage,frame", Converter::DdsToPng, "hud/rage_frame.png"), // payload is not DDS
            req("hud_runic_ring", "runic,ring", Converter::DdsToPng, "hud/runic_ring.png"), // absent
            req("sfx_parry", "shield,parry", Converter::WemToWav, "sfx/parry.wav"), // overridden below
            req("mimir_kill", "never-matches", Converter::WemToWavSet, "mimir/kill/"),
        ];
        let overrides = json!({
            "sfx_parry": ["wem:audio/vo.0.audiopack#2"],
            "mimir_kill": ["wem:audio/vo.0.audiopack#1", "wem:audio/vo.0.audiopack#2"]
        });
        let r = prepare(&root, &reqs, &overrides, &cache, &Tools::default()).unwrap();
        assert!(matches!(r.per_asset["hud_health_frame"], AssetState::Converted { .. }));
        assert!(cache.join("hud/health_frame.png").exists());
        match &r.per_asset["hud_rage_frame"] {
            AssetState::Failed { reason } => assert!(reason.contains("not DDS"), "{reason}"),
            s => panic!("{s:?}"),
        }
        assert_eq!(r.per_asset["hud_runic_ring"], AssetState::NotFound);
        assert!(cache.join("sfx/parry.wav").exists());
        assert!(cache.join("mimir/kill/1.wav").exists() && cache.join("mimir/kill/2.wav").exists());
        assert_eq!(r.converted(), 3);
        assert_eq!(r.missing(), vec!["hud_rage_frame", "hud_runic_ring"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_install_is_an_error() {
        assert!(prepare(Path::new("/no/such/gow"), &[], &json!({}), Path::new("/tmp/x"), &Tools::default()).is_err());
    }
}

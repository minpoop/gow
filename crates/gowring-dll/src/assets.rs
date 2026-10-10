//! hooks.gow_cache — what `GowRing.exe --prepare` converted from the player's Ragnarök install.

use gowring_core::sheets::{AssetKind, ASSETS};
use std::collections::HashMap;
use std::path::Path;

pub struct Texture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub struct Store {
    pub textures: HashMap<&'static str, Texture>,
    pub total: usize,
    pub converted: usize,
    pub missing: Vec<String>,
    pub channel_alpha: bool,
    pub prepared: bool,
}

fn decode_png(path: &Path) -> Result<Texture, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        other => return Err(format!("unsupported PNG colour type {other:?}")),
    };
    Ok(Texture { w: info.width, h: info.height, rgba })
}

pub fn load(cache: &Path, channel: &str) -> Store {
    let mut s = Store { total: ASSETS.len(), channel_alpha: channel != "release", ..Default::default() };
    s.prepared = cache.join("manifest.json").exists();
    for a in ASSETS {
        let path = cache.join(a.output);
        let ok = match a.kind {
            AssetKind::Texture => match decode_png(&path) {
                Ok(t) => {
                    s.textures.insert(a.id, t);
                    true
                }
                Err(e) => {
                    if path.exists() {
                        crate::glog!("assets", "{}: could not decode {}: {e}", a.id, path.display());
                    }
                    false
                }
            },
            AssetKind::Audio => path.exists() && (!a.output.ends_with('/') || std::fs::read_dir(&path).map(|d| d.count() > 0).unwrap_or(false)),
        };
        if ok {
            s.converted += 1;
        } else {
            s.missing.push(a.id.to_string());
        }
    }
    s
}

//! hooks.gow_cache — what `GowRing.exe --prepare` converted from the player's Ragnarök install.

use gowring_core::sheets::{AssetKind, ASSETS};
use std::collections::HashMap;
use std::path::Path;

pub struct Texture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// A white shape on transparency (Ragnarök's UI masks): the HUD colours it instead of drawing it as is.
    pub mask: bool,
}

/// Ragnarök's UI shapes sit inside textures with empty space around them and often top out below full alpha:
/// crop to the visible shape and stretch its alpha to full, so the shape fills the HUD element and reads as solid.
pub fn trim_mask(t: Texture) -> Texture {
    let (w, h) = (t.w as usize, t.h as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    let mut top = 0u8;
    for y in 0..h {
        for x in 0..w {
            let a = t.rgba[(y * w + x) * 4 + 3];
            top = top.max(a);
            if a > 8 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return t;
    }
    let (nw, nh) = (x1 - x0, y1 - y0);
    let gain = if top > 0 { 255.0 / top as f32 } else { 1.0 };
    let mut rgba = Vec::with_capacity(nw * nh * 4);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 4;
            // stretched to full and boosted, so a faint interior reads as a solid shape; soft edges stay soft
            let a = (t.rgba[i + 3] as f32 * gain * 3.0).round().min(255.0) as u8;
            rgba.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    Texture { w: nw as u32, h: nh as u32, rgba, mask: true }
}

/// Every visible pixel near white and without colour.
pub fn is_mask(rgba: &[u8]) -> bool {
    let mut seen = false;
    for p in rgba.chunks_exact(4) {
        if p[3] < 16 {
            continue;
        }
        seen = true;
        let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        if lo < 200 || hi - lo > 12 {
            return false;
        }
    }
    seen
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
    let mask = is_mask(&rgba);
    let t = Texture { w: info.width, h: info.height, rgba, mask };
    Ok(if mask { trim_mask(t) } else { t })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_is_cropped_to_its_shape_and_made_solid() {
        // 8x4 texture, shape in columns 2..6 of row 1..3 at alpha 128
        let mut rgba = vec![0u8; 8 * 4 * 4];
        for y in 1..3 {
            for x in 2..6 {
                let i = (y * 8 + x) * 4;
                rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 128]);
            }
        }
        assert!(is_mask(&rgba));
        let t = trim_mask(Texture { w: 8, h: 4, rgba, mask: true });
        assert_eq!((t.w, t.h), (4, 2));
        assert!(t.rgba.chunks_exact(4).all(|p| p[3] == 255));
        assert!(!is_mask(&[200, 40, 40, 255]), "a coloured picture is not a mask");
    }
}

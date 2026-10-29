//! Saves a copied back buffer as a PNG: 8-bit and 10-bit formats as they are, half-float (scRGB) clamped and turned
//! to sRGB. Shrunk by a whole factor to at most 1920 pixels wide, so a 4K frame stays a few MB to send.

use crate::d3d::{format_name, Image};
use crate::plog;
use windows::Win32::Graphics::Dxgi::Common::*;

pub fn save(img: Image, n: u32, label: &str) {
    let r = std::panic::catch_unwind(|| write(&img, n, label));
    match r {
        Ok(Ok(name)) => plog!("shot", "#{n} saved as {name}"),
        Ok(Err(e)) => plog!("shot", "#{n} not saved: {e}"),
        Err(_) => plog!("shot", "#{n} not saved: panic while converting"),
    }
}

fn write(img: &Image, n: u32, label: &str) -> Result<String, String> {
    let (w, h) = (img.width as usize, img.height as usize);
    let px = |x: usize, y: usize| -> [f32; 3] {
        let row = &img.data[y * img.row_bytes..];
        match img.format {
            DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => {
                let p = &row[x * 4..];
                [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]
            }
            DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB | DXGI_FORMAT_B8G8R8X8_UNORM => {
                let p = &row[x * 4..];
                [p[2] as f32 / 255.0, p[1] as f32 / 255.0, p[0] as f32 / 255.0]
            }
            DXGI_FORMAT_R10G10B10A2_UNORM => {
                let v = u32::from_le_bytes([row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]]);
                [(v & 1023) as f32 / 1023.0, ((v >> 10) & 1023) as f32 / 1023.0, ((v >> 20) & 1023) as f32 / 1023.0]
            }
            DXGI_FORMAT_R16G16B16A16_FLOAT => {
                let c = |i: usize| to_srgb(half(u16::from_le_bytes([row[x * 8 + i * 2], row[x * 8 + i * 2 + 1]])));
                [c(0), c(1), c(2)]
            }
            _ => [1.0, 0.0, 1.0],
        }
    };
    if !matches!(
        img.format,
        DXGI_FORMAT_R8G8B8A8_UNORM
            | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
            | DXGI_FORMAT_B8G8R8A8_UNORM
            | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
            | DXGI_FORMAT_B8G8R8X8_UNORM
            | DXGI_FORMAT_R10G10B10A2_UNORM
            | DXGI_FORMAT_R16G16B16A16_FLOAT
    ) {
        return Err(format!("format {} ({}) is not handled", img.format.0, format_name(img.format)));
    }
    let k = w.div_ceil(1920).max(1);
    let (ow, oh) = (w / k, h / k);
    let mut out = Vec::with_capacity(ow * oh * 3);
    for oy in 0..oh {
        for ox in 0..ow {
            let mut acc = [0f32; 3];
            for dy in 0..k {
                for dx in 0..k {
                    let p = px(ox * k + dx, oy * k + dy);
                    for c in 0..3 {
                        acc[c] += p[c];
                    }
                }
            }
            for a in acc {
                out.push((a / (k * k) as f32 * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    let safe: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let name = format!("shot_{n:02}_{safe}.png");
    let f = std::fs::File::create(crate::log::dir().join(&name)).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), ow as u32, oh as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(&out).map_err(|e| e.to_string())?;
    Ok(format!("{name} ({ow}x{oh} from {w}x{h})"))
}

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    s * match e {
        0 => m / 1024.0 * 2f32.powi(-14),
        31 => f32::INFINITY,
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

fn to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

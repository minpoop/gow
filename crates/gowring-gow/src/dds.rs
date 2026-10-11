//! DDS → PNG for HUD art (BC1/2/3/4/5/7 and 32-bit uncompressed).

use anyhow::{bail, Result};

fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc7,
    Rgba8,
    Bgra8,
}

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub format: Format,
}

pub fn decode(b: &[u8]) -> Result<Image> {
    if b.len() < 128 || &b[0..4] != b"DDS " {
        bail!("not a DDS file");
    }
    let (h, w) = (u32le(b, 12), u32le(b, 16));
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        bail!("implausible size {w}x{h}");
    }
    let fourcc = &b[84..88];
    let mut data_at = 128usize;
    let format = match fourcc {
        b"DXT1" => Format::Bc1,
        b"DXT3" => Format::Bc2,
        b"DXT5" => Format::Bc3,
        b"ATI1" | b"BC4U" => Format::Bc4,
        b"ATI2" | b"BC5U" => Format::Bc5,
        b"DX10" => {
            data_at = 148;
            if b.len() < 148 {
                bail!("truncated DX10 header");
            }
            match u32le(b, 128) {
                70..=72 => Format::Bc1,
                73..=75 => Format::Bc2,
                76..=78 => Format::Bc3,
                79..=81 => Format::Bc4,
                82..=84 => Format::Bc5,
                97..=99 => Format::Bc7,
                28 | 29 => Format::Rgba8,
                87 | 91 => Format::Bgra8,
                other => bail!("unsupported DXGI format {other}"),
            }
        }
        _ => {
            let bits = u32le(b, 88);
            if bits == 32 {
                if u32le(b, 92) == 0xFF { Format::Rgba8 } else { Format::Bgra8 }
            } else {
                bail!("unsupported pixel format {:?}", fourcc);
            }
        }
    };
    decode_raw(format, w, h, &b[data_at..])
}

/// Decode tightly packed texture data (BC blocks row by row, or 32-bit pixels) of a known format.
pub fn decode_raw(format: Format, w: u32, h: u32, data: &[u8]) -> Result<Image> {
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        bail!("implausible size {w}x{h}");
    }
    let (wz, hz) = (w as usize, h as usize);
    let mut px = vec![0u32; wz * hz];
    match format {
        Format::Bc1 | Format::Bc2 | Format::Bc3 | Format::Bc4 | Format::Bc5 | Format::Bc7 => {
            let blocks = wz.div_ceil(4) * hz.div_ceil(4);
            let size = blocks * if matches!(format, Format::Bc1 | Format::Bc4) { 8 } else { 16 };
            if data.len() < size {
                bail!("texture data truncated: {} < {size}", data.len());
            }
            let r = match format {
                Format::Bc1 => texture2ddecoder::decode_bc1(data, wz, hz, &mut px),
                Format::Bc2 => texture2ddecoder::decode_bc2(data, wz, hz, &mut px),
                Format::Bc3 => texture2ddecoder::decode_bc3(data, wz, hz, &mut px),
                Format::Bc4 => texture2ddecoder::decode_bc4(data, wz, hz, &mut px),
                Format::Bc5 => texture2ddecoder::decode_bc5(data, wz, hz, &mut px),
                _ => texture2ddecoder::decode_bc7(data, wz, hz, &mut px),
            };
            r.map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;
            // texture2ddecoder yields BGRA packed in u32 (little-endian: B,G,R,A bytes)
            let mut rgba = Vec::with_capacity(wz * hz * 4);
            for p in px {
                let [b, g, r, a] = p.to_le_bytes();
                rgba.extend_from_slice(&[r, g, b, a]);
            }
            Ok(Image { w, h, rgba, format })
        }
        Format::Rgba8 | Format::Bgra8 => {
            if data.len() < wz * hz * 4 {
                bail!("texture data truncated");
            }
            let mut rgba = data[..wz * hz * 4].to_vec();
            if format == Format::Bgra8 {
                for c in rgba.chunks_exact_mut(4) {
                    c.swap(0, 2);
                }
            }
            Ok(Image { w, h, rgba, format })
        }
    }
}

pub fn to_png(img: &Image) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.w, img.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        w.write_image_data(&img.rgba)?;
    }
    Ok(out)
}

pub fn dds_to_png(bytes: &[u8]) -> Result<Vec<u8>> {
    to_png(&decode(bytes)?)
}

/// A 4x4 solid-colour BC1 DDS for tests.
pub fn build_test_bc1() -> Vec<u8> {
    let mut v = vec![0u8; 128];
    v[0..4].copy_from_slice(b"DDS ");
    v[4..8].copy_from_slice(&124u32.to_le_bytes());
    v[12..16].copy_from_slice(&4u32.to_le_bytes());
    v[16..20].copy_from_slice(&4u32.to_le_bytes());
    v[76..80].copy_from_slice(&32u32.to_le_bytes());
    v[80..84].copy_from_slice(&4u32.to_le_bytes());
    v[84..88].copy_from_slice(b"DXT1");
    // c0 = pure red (565: 0xF800), c1 = black, all indices 0 => red
    v.extend_from_slice(&[0x00, 0xF8, 0x00, 0x00, 0, 0, 0, 0]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_bc1_and_writes_png() {
        let img = decode(&build_test_bc1()).unwrap();
        assert_eq!((img.w, img.h, img.format), (4, 4, Format::Bc1));
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
        let png = to_png(&img).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn rejects_junk_and_truncation() {
        assert!(decode(b"nope").is_err());
        let mut t = build_test_bc1();
        t.truncate(130);
        assert!(decode(&t).is_err());
    }
}

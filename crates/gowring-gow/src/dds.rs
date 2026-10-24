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
    /// BC4 of signed values (`BC4_SNORM`): decoded as a mask, its positive half.
    Bc4S,
    Bc5,
    /// BC5 of signed values (`BC5_SNORM`): decoded as a normal map, -1..1 as 0..255 with blue worked out from red and
    /// green.
    Bc5S,
    Bc6U,
    Bc6S,
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
        b"BC4S" => Format::Bc4S,
        b"ATI2" | b"BC5U" => Format::Bc5,
        b"BC5S" => Format::Bc5S,
        b"DX10" => {
            data_at = 148;
            if b.len() < 148 {
                bail!("truncated DX10 header");
            }
            match u32le(b, 128) {
                70..=72 => Format::Bc1,
                73..=75 => Format::Bc2,
                76..=78 => Format::Bc3,
                79 | 80 => Format::Bc4,
                81 => Format::Bc4S,
                82 | 83 => Format::Bc5,
                84 => Format::Bc5S,
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
        Format::Bc4S | Format::Bc5S => decode_snorm(format, w, h, data),
        Format::Bc1 | Format::Bc2 | Format::Bc3 | Format::Bc4 | Format::Bc5 | Format::Bc6U | Format::Bc6S | Format::Bc7 => {
            let blocks = wz.div_ceil(4) * hz.div_ceil(4);
            let size = blocks * if matches!(format, Format::Bc1 | Format::Bc4) { 8 } else { 16 };
            if data.len() < size {
                bail!("texture data truncated: {} < {size}", data.len());
            }
            let r = match format {
                // with BC1's one-bit alpha (the three-colour form's fourth index is transparent black), as Direct3D reads it
                Format::Bc1 => texture2ddecoder::decode_bc1a(data, wz, hz, &mut px),
                Format::Bc2 => texture2ddecoder::decode_bc2(data, wz, hz, &mut px),
                Format::Bc3 => texture2ddecoder::decode_bc3(data, wz, hz, &mut px),
                Format::Bc4 => texture2ddecoder::decode_bc4(data, wz, hz, &mut px),
                Format::Bc5 => texture2ddecoder::decode_bc5(data, wz, hz, &mut px),
                Format::Bc6U => texture2ddecoder::decode_bc6(data, wz, hz, &mut px, false),
                Format::Bc6S => texture2ddecoder::decode_bc6(data, wz, hz, &mut px, true),
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

/// The texels of a BC4 block of signed values (BC4_SNORM, and each half of BC5_SNORM), row by row, each in -1..=1.
/// texture2ddecoder reads only unsigned ones: a signed normal map read so has its flat parts (0) at one end and its
/// slightly tilted ones split between the two (0.1.86: "the way the light is hitting the fur looks off").
fn snorm_block(b: &[u8]) -> [f32; 16] {
    let end = |x: u8| (x as i8).max(-127) as f32 / 127.0;
    let (r0, r1) = (end(b[0]), end(b[1]));
    let mut pal = [r0, r1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    if r0 > r1 {
        for i in 1..7 {
            pal[i + 1] = ((7 - i) as f32 * r0 + i as f32 * r1) / 7.0;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = ((5 - i) as f32 * r0 + i as f32 * r1) / 5.0;
        }
        pal[6] = -1.0;
        pal[7] = 1.0;
    }
    let bits = u64::from_le_bytes([b[2], b[3], b[4], b[5], b[6], b[7], 0, 0]);
    std::array::from_fn(|k| pal[((bits >> (3 * k)) & 7) as usize])
}

/// BC4 or BC5 of signed values: BC4 as a mask (its positive half as 0..255 in red), BC5 as a normal map (red and green
/// from -1..1 to 0..255, blue worked out from them).
fn decode_snorm(format: Format, w: u32, h: u32, data: &[u8]) -> Result<Image> {
    let (wz, hz) = (w as usize, h as usize);
    let (bw, bh) = (wz.div_ceil(4), hz.div_ceil(4));
    let step = if format == Format::Bc4S { 8 } else { 16 };
    if data.len() < bw * bh * step {
        bail!("texture data truncated: {} < {}", data.len(), bw * bh * step);
    }
    let byte = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
    let mut rgba = vec![0u8; wz * hz * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) * step..];
            let red = snorm_block(&blk[..8]);
            let green = (format == Format::Bc5S).then(|| snorm_block(&blk[8..16]));
            for k in 0..16 {
                let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
                if x >= wz || y >= hz {
                    continue;
                }
                let px = match green {
                    Some(g) => {
                        let (nx, ny) = (red[k], g[k]);
                        [byte(nx), byte(ny), byte((1.0 - nx * nx - ny * ny).max(0.0).sqrt()), 255]
                    }
                    None => [(red[k].max(0.0) * 255.0).round() as u8, 0, 0, 255],
                };
                rgba[(y * wz + x) * 4..(y * wz + x) * 4 + 4].copy_from_slice(&px);
            }
        }
    }
    Ok(Image { w, h, rgba, format })
}

/// The image shrunk by whole factors of two until its longer side is at most `max_side` (each pixel the average of
/// the ones it covers).
pub fn downscale(img: &Image, max_side: u32) -> Image {
    let (mut w, mut h, mut px) = (img.w, img.h, img.rgba.clone());
    while w.max(h) > max_side && w > 1 && h > 1 {
        let (nw, nh) = (w / 2, h / 2);
        let mut out = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| px[((yy * w + xx) * 4 + c) as usize] as u32;
                    out[((y * nw + x) * 4 + c) as usize] = ((at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1) + 2) / 4) as u8;
                }
            }
        }
        (w, h, px) = (nw, nh, out);
    }
    Image { w, h, rgba: px, format: img.format }
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

    /// The first pixel and the average of each DDS file in `GOWRING_DDS_DIR` (for reading the game's stand-in
    /// textures): `GOWRING_DDS_DIR=... cargo test -p gowring-gow print_dds_pixels -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_dds_pixels() {
        let Ok(dir) = std::env::var("GOWRING_DDS_DIR") else { return };
        let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        names.sort();
        for p in names {
            match decode(&std::fs::read(&p).unwrap()) {
                Ok(img) => {
                    let n = (img.w * img.h) as usize;
                    let avg: Vec<u32> = (0..4).map(|c| (0..n).map(|i| img.rgba[i * 4 + c] as u32).sum::<u32>() / n as u32).collect();
                    println!("{:40} {}x{} {:?} first {:?} average {avg:?}", p.file_name().unwrap().to_string_lossy(), img.w, img.h, img.format, &img.rgba[..4]);
                }
                Err(e) => println!("{:40} {e:#}", p.file_name().unwrap().to_string_lossy()),
            }
        }
    }

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

    #[test]
    fn signed_bc4_and_bc5_read_as_signed() {
        // BC5_SNORM all 0 (a flat normal): the middle, facing out (read as unsigned: 0, 0, fully tilted)
        let flat = decode_raw(Format::Bc5S, 4, 4, &[0u8; 16]).unwrap();
        assert!(flat.rgba.chunks_exact(4).all(|p| p == [128, 128, 255, 255]));
        // red all -1 (both ends 0x81), green all +1 (both ends 0x7f)
        let mut b = [0u8; 16];
        (b[0], b[1], b[8], b[9]) = (0x81, 0x81, 0x7f, 0x7f);
        let tilted = decode_raw(Format::Bc5S, 4, 4, &b).unwrap();
        assert_eq!(&tilted.rgba[..4], &[0, 255, 128, 255]);
        // BC4_SNORM as a mask: its first end (+1) full, its second (0x80, read as -1) empty
        let mut m = [0u8; 8];
        (m[0], m[1], m[2]) = (0x7f, 0x80, 0b001_000);
        let mask = decode_raw(Format::Bc4S, 4, 4, &m).unwrap();
        assert_eq!((mask.rgba[0], mask.rgba[4]), (255, 0));
    }
}

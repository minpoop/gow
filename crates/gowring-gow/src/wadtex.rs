//! Textures kept inside Ragnarök PC WADs (HUD art and other small images), decoded to PNG.
//!
//! Each texture is a pair of chunks with the same `TX_...` name: a parameter chunk (id 1, server 25, client 1) whose
//! first 0x68 bytes describe the texture and which then carries a "GNF " header with the GPU texture descriptor (the
//! 32-byte T#), and a GPU chunk (id 29, server 25) holding the pixels. Inside a WAD the pixels are stored linearly,
//! not in the console's tiled layout: every mip level gets an allocation whose rows are padded to 256 bytes, the
//! smallest mip first and the full-size image last, and the rows inside an allocation are packed tightly.
//!
//! Field positions and the layout rule follow the public community documentation of the format (the smpack
//! project); this is our own implementation of it.

use crate::dds::{self, Format};
use crate::wtoc::Entry;
use anyhow::{bail, Result};
use std::path::Path;

pub const GNF_AT: usize = 0x68;

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The GPU texture descriptor (8 words).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TSharp(pub [u32; 8]);

impl TSharp {
    pub fn format(&self) -> u32 {
        (self.0[1] >> 20) & 0x1FF
    }
    pub fn width(&self) -> u32 {
        (((self.0[1] >> 30) & 3) | ((self.0[2] & 0xFFF) << 2)) + 1
    }
    pub fn height(&self) -> u32 {
        ((self.0[2] >> 14) & 0x3FFF) + 1
    }
    pub fn last_mip(&self) -> u32 {
        (self.0[3] >> 16) & 0xF
    }
    pub fn tile_mode(&self) -> u32 {
        (self.0[3] >> 20) & 0x1F
    }
    pub fn kind(&self) -> u32 {
        (self.0[3] >> 28) & 0xF
    }
    pub fn mips(&self) -> u32 {
        if matches!(self.kind(), 14 | 15) {
            1
        } else {
            self.last_mip() + 1
        }
    }
}

#[derive(Clone, Debug)]
pub struct TexParm {
    pub name: String,
    pub flags: u16,
    pub width: u16,
    pub height: u16,
    pub tsharp: Option<TSharp>,
}

pub fn parse_parm(b: &[u8]) -> Option<TexParm> {
    if b.len() < GNF_AT + 48 {
        return None;
    }
    let raw = &b[0x0C..0x0C + 56];
    let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    let tsharp = (&b[GNF_AT..GNF_AT + 4] == b"GNF ").then(|| {
        let mut w = [0u32; 8];
        for (i, x) in w.iter_mut().enumerate() {
            *x = u32_at(b, GNF_AT + 16 + 4 * i);
        }
        TSharp(w)
    });
    Some(TexParm { name: String::from_utf8_lossy(&raw[..end]).into_owned(), flags: u16_at(b, 0x44), width: u16_at(b, 0x48), height: u16_at(b, 0x4A), tsharp })
}

/// (decoder format, bytes per element, texels per element side) for the GPU format numbers we can decode.
pub fn format_info(f: u32) -> Option<(Format, usize, u32)> {
    Some(match f {
        169 | 170 => (Format::Bc1, 8, 4),
        171 | 172 => (Format::Bc2, 16, 4),
        173 | 174 => (Format::Bc3, 16, 4),
        175 => (Format::Bc4, 8, 4),
        177 => (Format::Bc5, 16, 4),
        181 | 182 => (Format::Bc7, 16, 4),
        56 | 130 => (Format::Rgba8, 4, 1),
        _ => return None,
    })
}

/// Where the full-size image starts in the GPU chunk, how long it is, and the size of the whole chain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub mip0_off: usize,
    pub mip0_len: usize,
    pub total: usize,
}

pub fn linear_layout(w: u32, h: u32, mips: u32, bpe: usize, blk: u32) -> Layout {
    let row_align = (256 / bpe).max(1);
    let (mut lw, mut lh) = (w.div_ceil(blk) as usize, h.div_ceil(blk) as usize);
    let mut sizes = Vec::with_capacity(mips as usize);
    let mip0_len = lw * lh * bpe;
    for _ in 0..mips.max(1) {
        sizes.push(lw.div_ceil(row_align) * row_align * lh * bpe);
        lw = lw.div_ceil(2);
        lh = lh.div_ceil(2);
    }
    // smallest first: the full-size image comes after every smaller level
    let mip0_off = sizes[1..].iter().sum();
    Layout { mip0_off, mip0_len, total: sizes.iter().sum() }
}

/// Decode the full-size image of a texture from its parameter and GPU chunks.
pub fn decode(parm: &TexParm, gpu: &[u8]) -> Result<(dds::Image, Layout)> {
    let Some(t) = parm.tsharp else { bail!("no GNF header") };
    let Some((format, bpe, blk)) = format_info(t.format()) else { bail!("GPU format {} not supported yet", t.format()) };
    if !matches!(t.kind(), 9 | 0) {
        bail!("texture type {} (not a plain 2D image)", t.kind());
    }
    let (w, h) = (t.width(), t.height());
    let l = linear_layout(w, h, t.mips(), bpe, blk);
    let Some(bytes) = gpu.get(l.mip0_off..l.mip0_off + l.mip0_len) else {
        bail!("GPU chunk is {} bytes; the {w}x{h} image would need {} at {:#x}", gpu.len(), l.mip0_len, l.mip0_off)
    };
    Ok((dds::decode_raw(format, w, h, bytes)?, l))
}

/// Every texture pair in a WAD's table: (parameter entry index, GPU entry index).
pub fn pairs(es: &[Entry]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, e) in es.iter().enumerate() {
        if !(e.id == 1 && e.server == 25 && e.client == 1) {
            continue;
        }
        let before = i.checked_sub(1).filter(|&j| es[j].id == 29 && es[j].server == 25 && es[j].name == e.name);
        let gpu = before.or_else(|| es.iter().position(|g| g.id == 29 && g.server == 25 && g.name == e.name));
        if let Some(g) = gpu {
            out.push((i, g));
        }
    }
    out
}

fn chunk<'a>(buf: &'a [u8], e: &Entry) -> Option<&'a [u8]> {
    buf.get(e.file_off as usize..e.file_off as usize + e.length as usize)
}

/// Decode every texture of a (decompressed) WAD into `out_dir` as PNG, up to `max` of them and while `more()` says
/// there is time. Returns a plain-text report (one line per texture, then a summary).
pub fn export(buf: &[u8], out_dir: &Path, max: usize, mut more: impl FnMut() -> bool) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let Some(es) = crate::wtoc::read(buf) else {
        return "not a readable WTOC table\n".into();
    };
    let _ = std::fs::create_dir_all(out_dir);
    let ps = pairs(&es);
    let mut shown: Vec<(String, u32, u32)> = Vec::new();
    let (mut ok, mut failed) = (0usize, 0usize);
    let mut sizes_match = 0usize;
    for &(pi, gi) in ps.iter().take(max) {
        if !more() {
            let _ = writeln!(o, "(stopped for time)");
            break;
        }
        let (pe, ge) = (&es[pi], &es[gi]);
        let (Some(pb), Some(gb)) = (chunk(buf, pe), chunk(buf, ge)) else {
            failed += 1;
            let _ = writeln!(o, "{}: chunk outside the file", pe.name);
            continue;
        };
        let Some(parm) = parse_parm(pb) else {
            failed += 1;
            let _ = writeln!(o, "{}: parameter chunk too short", pe.name);
            continue;
        };
        let t = parm.tsharp.unwrap_or_default();
        match decode(&parm, gb) {
            Ok((img, l)) => {
                if l.total == gb.len() {
                    sizes_match += 1;
                }
                let file = out_dir.join(format!("{}.png", safe(&pe.name)));
                let wrote = dds::to_png(&img).ok().and_then(|png| std::fs::write(&file, png).ok()).is_some();
                ok += wrote as usize;
                if wrote {
                    shown.push((safe(&pe.name), img.w, img.h));
                }
                let _ = writeln!(o, "{} {}x{} fmt {} mips {} tile {} gpu {:#x} layout {:#x}{}", pe.name, img.w, img.h, t.format(), t.mips(), t.tile_mode(), gb.len(), l.total, if wrote { "" } else { " (could not write the PNG)" });
            }
            Err(e) => {
                failed += 1;
                let _ = writeln!(o, "{} fmt {} {}x{} mips {} tile {} gpu {:#x}: {e:#}", pe.name, t.format(), t.width(), t.height(), t.mips(), t.tile_mode(), gb.len());
            }
        }
    }
    let _ = std::fs::write(out_dir.join("index.html"), gallery(&shown));
    let _ = writeln!(o, "summary: {} texture pairs, {ok} written as PNG, {failed} not decoded, {sizes_match} whose GPU chunk is exactly the laid-out size", ps.len());
    o
}

/// A page that shows every exported picture with its name, with a filter box (open it in any browser).
pub fn gallery(items: &[(String, u32, u32)]) -> String {
    use std::fmt::Write;
    let mut h = String::from(
        "<!doctype html><meta charset=utf-8><title>Ragnarok UI textures</title>\n<style>body{background:#1b1b1d;color:#e8e2cf;font:13px system-ui,sans-serif;margin:16px}input{font:inherit;padding:6px 8px;width:min(420px,90vw);margin:8px 0 16px}.g{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:12px}figure{margin:0;background:#2a2a2e;border-radius:8px;padding:8px}img{display:block;max-width:100%;max-height:160px;margin:auto;background:repeating-conic-gradient(#3a3a40 0 25%,#2e2e33 0 50%) 0 0/16px 16px}figcaption{margin-top:6px;word-break:break-all;user-select:all}small{color:#a39e8c}</style>\n<h1>Ragnar&ouml;k UI textures from your install</h1><p>Copy a name into <code>ragnarok_hud.json</code> next to the HUD piece it should replace, then start the game again.</p>\n<input id=f placeholder='filter by name, e.g. sonhud, rage, icon'>\n<div class=g id=g>\n",
    );
    for (name, w, hh) in items {
        let _ = writeln!(h, "<figure data-n='{}'><img loading=lazy src='{name}.png'><figcaption>{name}<br><small>{w} x {hh}</small></figcaption></figure>", name.to_lowercase());
    }
    h.push_str("</div>\n<script>f.oninput=()=>{const q=f.value.toLowerCase();for(const e of g.children)e.style.display=e.dataset.n.includes(q)?'':'none'}</script>\n");
    h
}

fn safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tsharp(format: u32, w: u32, h: u32, last_mip: u32) -> [u32; 8] {
        let mut t = [0u32; 8];
        t[1] = (format << 20) | (((w - 1) & 3) << 30);
        t[2] = ((w - 1) >> 2) | ((h - 1) << 14);
        t[3] = (last_mip << 16) | (9 << 28);
        t
    }

    fn parm(name: &str, t: [u32; 8]) -> Vec<u8> {
        let mut b = vec![0u8; GNF_AT + 64];
        b[0x0C..0x0C + name.len()].copy_from_slice(name.as_bytes());
        b[GNF_AT..GNF_AT + 4].copy_from_slice(b"GNF ");
        for (i, x) in t.iter().enumerate() {
            b[GNF_AT + 16 + 4 * i..GNF_AT + 20 + 4 * i].copy_from_slice(&x.to_le_bytes());
        }
        b
    }

    /// A solid BC1 block: colour0 = colour1 = `rgb565`, all indices 0.
    fn bc1(rgb565: u16) -> [u8; 8] {
        let c = rgb565.to_le_bytes();
        [c[0], c[1], c[0], c[1], 0, 0, 0, 0]
    }

    #[test]
    fn descriptor_fields_round_trip() {
        let t = TSharp(tsharp(181, 300, 77, 4));
        assert_eq!((t.format(), t.width(), t.height(), t.mips(), t.kind()), (181, 300, 77, 5, 9));
    }

    #[test]
    fn the_full_size_image_comes_after_the_smaller_ones() {
        // 8x8 BC1 with 2 mips: mip 1 (1x1 blocks) takes one 256-byte row, mip 0 (2x2 blocks) two rows after it
        let l = linear_layout(8, 8, 2, 8, 4);
        assert_eq!(l, Layout { mip0_off: 256, mip0_len: 32, total: 768 });
        let p = parse_parm(&parm("TX_test", tsharp(169, 8, 8, 1))).unwrap();
        let mut gpu = vec![0u8; 768];
        for b in 0..4 {
            gpu[256 + 8 * b..256 + 8 * b + 8].copy_from_slice(&bc1(0xF800)); // red
        }
        gpu[0..8].copy_from_slice(&bc1(0x001F)); // blue small mip, must not be picked
        let (img, l2) = decode(&p, &gpu).unwrap();
        assert_eq!((img.w, img.h, l2.total), (8, 8, 768));
        assert!(img.rgba.chunks_exact(4).all(|px| px[0] > 200 && px[2] < 30 && px[3] == 255), "{:?}", &img.rgba[..8]);
    }

    #[test]
    fn unsupported_and_short_chunks_are_errors_not_panics() {
        let p = parse_parm(&parm("TX_x", tsharp(179, 8, 8, 0))).unwrap(); // BC6H
        assert!(decode(&p, &[0u8; 64]).is_err());
        let p = parse_parm(&parm("TX_x", tsharp(169, 64, 64, 0))).unwrap();
        assert!(decode(&p, &[0u8; 16]).is_err());
        assert!(parse_parm(&[0u8; 10]).is_none());
    }

    #[test]
    fn a_wad_with_one_texture_exports_one_png() {
        use crate::wtoc::{ENTRY_LEN, HEADER_LEN, MAGIC};
        let parm_bytes = parm("TX_hud_test_0123", tsharp(169, 8, 8, 1));
        let mut gpu = vec![0u8; 768];
        for b in 0..4 {
            gpu[256 + 8 * b..256 + 8 * b + 8].copy_from_slice(&bc1(0x07E0)); // green
        }
        let entry = |id: u16, client: u8, len: usize, arena: u8, bits: u8| {
            let mut e = vec![0u8; ENTRY_LEN];
            e[0..2].copy_from_slice(&id.to_le_bytes());
            e[4..8].copy_from_slice(&(len as u32).to_le_bytes());
            e[0x18..0x18 + 16].copy_from_slice(b"TX_hud_test_0123");
            e[0x6C] = client;
            e[0x6D] = 25;
            e[0x6F] = arena;
            e[0x72] = bits;
            e
        };
        let mut buf = vec![0u8; HEADER_LEN];
        buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&2u32.to_le_bytes());
        buf[8..12].copy_from_slice(&2u32.to_le_bytes());
        buf.extend(entry(29, 0, gpu.len(), 1, 0)); // GPU chunk first, in the gpu arena
        buf.extend(entry(1, 1, parm_bytes.len(), 0, 1)); // then its parameter chunk, ending the block
        buf.extend(&parm_bytes); // arena 0 (cpu)
        buf.extend(&gpu); // arena 1 (gpu)
        let dir = std::env::temp_dir().join(format!("gowring-wadtex-{}", std::process::id()));
        let report = export(&buf, &dir, 10, || true);
        assert!(report.contains("1 written as PNG"), "{report}");
        assert!(report.contains("1 whose GPU chunk is exactly the laid-out size"), "{report}");
        let png = std::fs::read(dir.join("TX_hud_test_0123.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let page = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(page.contains("src='TX_hud_test_0123.png'") && page.contains("8 x 8"), "{page}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

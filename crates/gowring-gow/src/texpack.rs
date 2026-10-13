//! Ragnarök's texture packs (`exec/wad/pc_le/<area>.texpack`), which hold the full mip chains of textures whose WAD
//! entries (`TX_<name>_<HEX16>`) carry only small mips.
//!
//! The table of contents (`<area>.texpack.toc`, an LZ4 frame; the pack starts with a plain copy) decompresses to: 32
//! zero bytes, then u32 table size, u32 block count, u32 offset of the block table, u32 texture count, u32 version 6,
//! u32 size of an optional JSON blob; then one 24-byte entry per texture (u64 key = the `HEX16` of its WAD name, u64
//! content hash, u64 table offset of its smallest-mip block); then 32-byte blocks (u32 offset in the pack in 16-byte
//! units, u32 texel bytes, u32 bytes on disk, u32 0, u8 largest mip index, u8 smallest mip index (index 0 = the 1x1
//! level), u16 0, u16 width and u16 height of the block's largest mip, u64 table offset of the next larger block or
//! all ones). A texture's blocks chain from its smallest mips to its largest; shipped packs keep the mips up to 512
//! pixels in one block and give each larger mip its own. Entries are not in key order. Layout as documented by the
//! community's smpack (our own reading of it), checked against a player's `root.texpack.toc` (12,433 textures, 21,767
//! blocks, 9,334 chain links).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// Where the block's bytes start in the pack.
    pub offset: u64,
    /// Texel bytes, and bytes on disk (a command stream, then the texels).
    pub bytes: u32,
    pub bytes_on_disk: u32,
    /// Mip indices counted from the 1x1 level.
    pub largest_mip: u8,
    pub smallest_mip: u8,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub key: u64,
    pub key2: u64,
    pub records: Vec<Record>,
}

#[derive(Clone, Debug, Default)]
pub struct Toc {
    pub textures: Vec<Texture>,
    by_key: std::collections::HashMap<u64, usize>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

impl Toc {
    /// A decompressed table of contents.
    pub fn parse(b: &[u8]) -> Option<Toc> {
        if b.get(..32)?.iter().any(|x| *x != 0) {
            return None;
        }
        let (size, blocks, table2, textures) = (u32_at(b, 32)? as usize, u32_at(b, 36)? as usize, u32_at(b, 40)? as usize, u32_at(b, 44)? as usize);
        if size > b.len() || u32_at(b, 48)? != 6 || table2 < 56 + 24 * textures || table2 + 32 * blocks > b.len() {
            return None;
        }
        let record = |o: usize| -> Option<Record> {
            if o < table2 || (o - table2) % 32 != 0 || o + 32 > table2 + 32 * blocks {
                return None;
            }
            Some(Record {
                offset: u32_at(b, o)? as u64 * 16,
                bytes: u32_at(b, o + 4)?,
                bytes_on_disk: u32_at(b, o + 8)?,
                largest_mip: *b.get(o + 16)?,
                smallest_mip: *b.get(o + 17)?,
                width: u16::from_le_bytes(b.get(o + 20..o + 22)?.try_into().ok()?),
                height: u16::from_le_bytes(b.get(o + 22..o + 24)?.try_into().ok()?),
            })
        };
        let mut out = Vec::with_capacity(textures);
        for i in 0..textures {
            let e = 56 + 24 * i;
            let mut records = Vec::new();
            let mut at = u64_at(b, e + 16)?;
            while at != u64::MAX {
                if records.len() > 32 {
                    return None;
                }
                records.push(record(at as usize)?);
                at = u64_at(b, at as usize + 24)?;
            }
            out.push(Texture { key: u64_at(b, e)?, key2: u64_at(b, e + 8)?, records });
        }
        let by_key = out.iter().enumerate().map(|(i, t)| (t.key, i)).collect();
        Some(Toc { textures: out, by_key })
    }

    /// The table beside a pack (`<pack>.toc`), decompressed.
    pub fn read(toc_path: &Path) -> Result<Toc> {
        let (b, _) = crate::lz4::read_file_prefix(toc_path, 256 << 20)?;
        match Self::parse(&b) {
            Some(t) => Ok(t),
            None => bail!("{}: unreadable texture table", toc_path.display()),
        }
    }

    pub fn find(&self, key: u64) -> Option<&Texture> {
        self.by_key.get(&key).map(|i| &self.textures[*i])
    }
}

/// The bytes of one block (its command stream and texels), read from its pack.
pub fn read_record(pack: &mut std::fs::File, r: &Record) -> Result<Vec<u8>> {
    let mut v = vec![0u8; r.bytes_on_disk as usize];
    pack.seek(SeekFrom::Start(r.offset))?;
    pack.read_exact(&mut v)?;
    Ok(v)
}

/// Where a block's texels start, and the 256-byte GNF header it carries (only the block with the largest mip has one).
/// A block starts with u32 commands: 1 {texel offset, total size}, 5 {256-byte GNF header}, 3 {12 bytes}, 4 {8 bytes},
/// 2 = end.
pub fn block_header(b: &[u8]) -> Option<(usize, Option<[u8; 256]>)> {
    let (mut at, mut texels, mut gnf) = (0usize, None, None);
    for _ in 0..16 {
        match u32_at(b, at)? {
            1 => {
                texels = Some(u32_at(b, at + 4)? as usize);
                at += 12;
            }
            5 => {
                gnf = Some(b.get(at + 4..at + 260)?.try_into().ok()?);
                at += 260;
            }
            3 => at += 16,
            4 => at += 12,
            2 => return Some((texels?, gnf)),
            _ => return None,
        }
    }
    None
}

/// Where each mip of a GPU-tiled 2D surface sits, as the console's address library lays it out (the rules as the
/// community's smpack documents them; our own implementation): elements are 4x4 blocks of a compressed format;
/// tiles of 256 B, 4 KiB or 64 KiB (tile mode 1, 5, 9; 0 is linear with 256-byte rows) hold W x H elements; mips
/// small enough to share one tile (the "tail") sit in the first tile at fixed places, then the other mips follow from
/// the smallest to the full size, each a row-major grid of tiles.
pub struct Surface {
    mode: u32,
    bpe_log2: u32,
    tile_w: u32,
    tile_h: u32,
    tile_bytes: u64,
    /// Bit pattern of each element column and row inside a tile, OR-ed to address an element.
    xs: Vec<u32>,
    ys: Vec<u32>,
    /// Per mip: (elements wide, elements high, pitch in tiles, offset, tail position in elements if in the tail).
    mips: Vec<(u32, u32, u32, u64, Option<(u32, u32)>)>,
    pub total: u64,
}

impl Surface {
    pub fn new(mode: u32, bytes_per_element: u32, width: u32, height: u32, block: u32, mips: u32) -> Option<Surface> {
        let bpe_log2 = bytes_per_element.trailing_zeros();
        if !bytes_per_element.is_power_of_two() || bpe_log2 > 4 || !matches!(mode, 0 | 1 | 5 | 9) || mips == 0 || mips > 16 {
            return None;
        }
        let (tile_w, tile_h, top, tile_bytes) = if mode == 0 {
            (256 / bytes_per_element, 1, 8, 256u64)
        } else {
            let top = match mode {
                1 => 8,
                5 => 12,
                _ => 16,
            };
            let e = top - bpe_log2;
            (1 << e.div_ceil(2), 1 << (e / 2), top, 1u64 << top)
        };
        // how the low bits of x and y land in an element's address inside a tile, by bytes per element (1..16)
        const MICRO_X: [&[u32]; 5] = [&[0, 1, 2, 3], &[1, 2, 3, 7], &[2, 3, 7], &[3, 6, 7], &[6, 7]];
        const MICRO_Y: [&[u32]; 5] = [&[4, 5, 6, 7], &[4, 5, 6], &[4, 5, 6], &[4, 5], &[4, 5]];
        let swizzle = |v: u32, is_x: bool| -> u32 {
            if mode == 0 {
                return 0;
            }
            let (low, mut next) = if is_x { (MICRO_X[bpe_log2 as usize], MICRO_X[bpe_log2 as usize].len()) } else { (MICRO_Y[bpe_log2 as usize], MICRO_Y[bpe_log2 as usize].len()) };
            let mut a = low.iter().enumerate().filter(|(i, _)| (v >> i) & 1 == 1).fold(0, |a, (_, bit)| a | 1 << bit);
            // above bit 8 the bits alternate, y on even positions and x on odd
            for bit in 8..top {
                if (bit % 2 == 1) == is_x {
                    if (v >> next) & 1 == 1 {
                        a |= 1 << bit;
                    }
                    next += 1;
                }
            }
            a
        };
        let xs = (0..tile_w).map(|x| swizzle(x, true)).collect();
        let ys = (0..tile_h).map(|y| swizzle(y, false)).collect();
        // element counts per mip, halving the top level's (rounding up)
        let (mut lw, mut lh) = (width.div_ceil(block), height.div_ceil(block));
        let mut dims = Vec::new();
        for _ in 0..mips {
            dims.push((lw, lh));
            lw = lw.div_ceil(2).max(1);
            lh = lh.div_ceil(2).max(1);
        }
        let first_tail = if matches!(mode, 5 | 9) { dims.iter().position(|(w, h)| *w <= tile_w / 2 && *h <= tile_h).unwrap_or(dims.len()) } else { dims.len() };
        // where each tail level sits inside the shared tile, in elements, by tile mode and bytes per element
        let tail: &[(u32, u32)] = match (mode, bpe_log2) {
            (5, 0) => &[(32, 0), (16, 32), (0, 48), (0, 32), (16, 16), (16, 0), (0, 16)],
            (5, 1) => &[(32, 0), (16, 16), (0, 24), (0, 16), (16, 8), (16, 0), (0, 8)],
            (5, 2) => &[(16, 0), (8, 16), (0, 24), (0, 16), (8, 8), (8, 0), (0, 8)],
            (5, 3) => &[(16, 0), (8, 8), (0, 12), (0, 8), (8, 4), (8, 0), (0, 4)],
            (5, 4) => &[(8, 0), (4, 8), (0, 12), (0, 8), (4, 4), (4, 0), (0, 4)],
            (9, 0) => &[(128, 0), (0, 128), (64, 0), (0, 64), (32, 0), (16, 32), (0, 48), (0, 32), (16, 16), (16, 0), (0, 16)],
            (9, 1) => &[(128, 0), (0, 64), (64, 0), (0, 32), (32, 0), (16, 16), (0, 24), (0, 16), (16, 8), (16, 0), (0, 8)],
            (9, 2) => &[(64, 0), (0, 64), (32, 0), (0, 32), (16, 0), (8, 16), (0, 24), (0, 16), (8, 8), (8, 0), (0, 8)],
            (9, 3) => &[(64, 0), (0, 32), (32, 0), (0, 16), (16, 0), (8, 8), (0, 12), (0, 8), (8, 4), (8, 0), (0, 4)],
            (9, 4) => &[(32, 0), (0, 32), (16, 0), (0, 16), (8, 0), (4, 8), (0, 12), (0, 8), (4, 4), (4, 0), (0, 4)],
            _ => &[],
        };
        let mut out = vec![(0, 0, 0, 0, None); dims.len()];
        let mut off = 0u64;
        if first_tail < dims.len() {
            for m in first_tail..dims.len() {
                let k = m - first_tail;
                let pos = *tail.get(k)?;
                out[m] = (dims[m].0, dims[m].1, 1, 0, Some(pos));
            }
            off = tile_bytes;
        }
        for m in (0..first_tail).rev() {
            let (w, h) = dims[m];
            let (pw, ph) = (w.div_ceil(tile_w), h.div_ceil(tile_h));
            out[m] = (w, h, pw, off, None);
            off += pw as u64 * ph as u64 * tile_bytes;
        }
        Some(Surface { mode, bpe_log2, tile_w, tile_h, tile_bytes, xs, ys, mips: out, total: off })
    }

    /// The elements of mip `m`, row by row, from the tiled surface bytes.
    pub fn untile(&self, surface: &[u8], m: usize) -> Option<Vec<u8>> {
        let (w, h, pitch, off, tail) = *self.mips.get(m)?;
        let bpe = 1usize << self.bpe_log2;
        let mut out = vec![0u8; w as usize * h as usize * bpe];
        for y in 0..h {
            for x in 0..w {
                let (tx, ty) = tail.map(|(a, b)| (x + a, y + b)).unwrap_or((x, y));
                let tile = if tail.is_some() { 0 } else { (ty / self.tile_h) as u64 * pitch as u64 + (tx / self.tile_w) as u64 };
                let inside = if self.mode == 0 { (tx % self.tile_w) as u64 * bpe as u64 } else { (self.xs[(tx % self.tile_w) as usize] | self.ys[(ty % self.tile_h) as usize]) as u64 };
                let at = (off + tile * self.tile_bytes + inside) as usize;
                let dst = (y as usize * w as usize + x as usize) * bpe;
                out[dst..dst + bpe].copy_from_slice(surface.get(at..at + bpe)?);
            }
        }
        Some(out)
    }
}

/// A texture's full-size image from its blocks' bytes (smallest mips first, as chained): the texels of each block
/// joined make the tiled surface, and the largest block's GNF header describes it.
pub fn decode(blocks: &[Vec<u8>]) -> anyhow::Result<crate::dds::Image> {
    let mut surface = Vec::new();
    let mut gnf = None;
    for b in blocks {
        let Some((texels, g)) = block_header(b) else { bail!("block without a command stream") };
        surface.extend_from_slice(b.get(texels..).unwrap_or(&[]));
        gnf = g.or(gnf);
    }
    let Some(gnf) = gnf else { bail!("no GNF header in the blocks") };
    if &gnf[..4] != b"GNF " {
        bail!("bad GNF header");
    }
    let t = crate::wadtex::TSharp(std::array::from_fn(|k| u32::from_le_bytes(gnf[0x10 + 4 * k..0x14 + 4 * k].try_into().unwrap())));
    let Some((format, bpe, blk)) = crate::wadtex::format_info(t.format()) else { bail!("GPU format {} not supported", t.format()) };
    let Some(layout) = Surface::new(t.tile_mode(), bpe as u32, t.width(), t.height(), blk, t.mips()) else { bail!("tile mode {} not supported", t.tile_mode()) };
    if (surface.len() as u64) < layout.total {
        bail!("{} bytes of texels; the {}x{} surface needs {}", surface.len(), t.width(), t.height(), layout.total);
    }
    let Some(top) = layout.untile(&surface, 0) else { bail!("surface too short") };
    crate::dds::decode_raw(format, t.width(), t.height(), &top)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table whose textures chain their blocks in order (smallest first).
    fn toc(textures: &[(u64, &[(u32, u32, u16, u16)])]) -> Vec<u8> {
        let n2: usize = textures.iter().map(|t| t.1.len()).sum();
        let table2 = 56 + 24 * textures.len();
        let size = table2 + 32 * n2;
        let mut b = vec![0u8; 32];
        for v in [size as u32, n2 as u32, table2 as u32, textures.len() as u32, 6, 0] {
            b.extend(v.to_le_bytes());
        }
        let mut first = table2;
        for (key, recs) in textures {
            b.extend(key.to_le_bytes());
            b.extend((key ^ 0xFFFF).to_le_bytes());
            b.extend((first as u64).to_le_bytes());
            first += 32 * recs.len();
        }
        let mut at = table2;
        for (_, recs) in textures {
            for (k, (at16, bytes, w, h)) in recs.iter().enumerate() {
                for v in [*at16, *bytes, bytes + 0x130, 0] {
                    b.extend(v.to_le_bytes());
                }
                b.extend([9u8, 0, 0, 0]);
                b.extend(w.to_le_bytes());
                b.extend(h.to_le_bytes());
                let next = if k + 1 < recs.len() { (at + 32) as u64 } else { u64::MAX };
                b.extend(next.to_le_bytes());
                at += 32;
            }
        }
        b
    }

    #[test]
    fn a_tiled_surface_has_the_size_the_pack_records() {
        // 512x512 BC1, 10 mips, 4 KiB tiles: a tail tile plus 32, 8 and 2 tiles for the three largest mips
        let s = Surface::new(5, 8, 512, 512, 4, 10).unwrap();
        assert_eq!(s.total, 0x2B000);
        // 4 KiB tiles of 8-byte elements are 32 x 16 elements; of 16-byte ones 16 x 16
        assert_eq!((s.tile_w, s.tile_h), (32, 16));
        assert_eq!((Surface::new(5, 16, 64, 64, 4, 1).unwrap().tile_w, Surface::new(5, 16, 64, 64, 4, 1).unwrap().tile_h), (16, 16));
        // every element of a tile lands on its own address
        let mut seen: Vec<u32> = s.xs.iter().flat_map(|x| s.ys.iter().map(move |y| x | y)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 32 * 16);
        assert!(seen.iter().all(|a| a % 8 == 0 && *a < 4096));
    }

    #[test]
    fn untiling_puts_each_element_back_where_it_was() {
        // write each element's own coordinates through the layout, then read the full-size mip back
        let s = Surface::new(5, 16, 256, 128, 4, 8).unwrap();
        let (w, h, pitch, off, _) = s.mips[0];
        let mut surface = vec![0u8; s.total as usize];
        for y in 0..h {
            for x in 0..w {
                let tile = (y / s.tile_h) as u64 * pitch as u64 + (x / s.tile_w) as u64;
                let at = (off + tile * s.tile_bytes + (s.xs[(x % s.tile_w) as usize] | s.ys[(y % s.tile_h) as usize]) as u64) as usize;
                surface[at..at + 4].copy_from_slice(&(y * 1000 + x).to_le_bytes());
            }
        }
        let top = s.untile(&surface, 0).unwrap();
        for (i, e) in top.chunks_exact(16).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            assert_eq!(u32::from_le_bytes(e[..4].try_into().unwrap()), y * 1000 + x);
        }
    }

    #[test]
    fn a_block_header_gives_the_texels_and_the_gnf_header() {
        let mut b = Vec::new();
        for v in [1u32, 0x124, 0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(5u32.to_le_bytes());
        let mut gnf = [0u8; 256];
        gnf[..4].copy_from_slice(b"GNF ");
        b.extend(gnf);
        b.extend(3u32.to_le_bytes());
        b.extend([0u8; 12]);
        b.extend(2u32.to_le_bytes());
        let (texels, g) = block_header(&b).unwrap();
        assert_eq!(texels, 0x124);
        assert_eq!(&g.unwrap()[..4], b"GNF ");
        assert!(block_header(&[9, 0, 0, 0]).is_none());
    }

    /// A player's saved blocks (the probe of 0.1.42 cut each short by its 0x124-byte header; the gap is zero-filled):
    /// `GOWRING_TEX_RAW=dir GOWRING_TEX_OUT=dir cargo test -p gowring-gow real_texture_blocks -- --ignored`.
    #[test]
    #[ignore]
    fn real_texture_blocks() {
        let (Ok(dir), Ok(out)) = (std::env::var("GOWRING_TEX_RAW"), std::env::var("GOWRING_TEX_OUT")) else { return };
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with("_0.bin")).collect();
        names.sort();
        for n in names {
            let stem = n.trim_end_matches("_0.bin");
            let mut blocks = Vec::new();
            for k in 0.. {
                let Ok(b) = std::fs::read(std::path::Path::new(&dir).join(format!("{stem}_{k}.bin"))) else { break };
                blocks.push(b);
            }
            if let Some(last) = blocks.last_mut() {
                last.extend(vec![0u8; 0x124]);
            }
            match decode(&blocks) {
                Ok(img) => {
                    println!("{stem}: {}x{} {:?}", img.w, img.h, img.format);
                    std::fs::write(std::path::Path::new(&out).join(format!("{stem}.png")), crate::dds::to_png(&img).unwrap()).unwrap();
                }
                Err(e) => println!("{stem}: {e:#}"),
            }
        }
    }

    #[test]
    fn textures_own_the_records_up_to_the_next_one() {
        let b = toc(&[(0x10, &[(1, 64, 4, 4)]), (0x20, &[(5, 128, 8, 8), (13, 256, 16, 16), (29, 32, 2, 2)]), (0x30, &[(31, 16, 1, 1)])]);
        let t = Toc::parse(&b).unwrap();
        assert_eq!(t.textures.len(), 3);
        let two = t.find(0x20).unwrap();
        assert_eq!(two.records.len(), 3);
        assert_eq!((two.records[1].offset, two.records[1].bytes, two.records[1].width), (13 * 16, 256, 16));
        assert!(t.find(0x25).is_none());
        // a table that is not version 6 is refused
        let mut bad = b.clone();
        bad[48] = 5;
        assert!(Toc::parse(&bad).is_none());
    }

    /// With a player's decompressed `root.texpack.toc`: `GOWRING_TEXPACK_TOC=... cargo test -p gowring-gow
    /// real_texpack -- --ignored`.
    #[test]
    #[ignore]
    fn real_texpack_table() {
        let Ok(p) = std::env::var("GOWRING_TEXPACK_TOC") else { return };
        let t = Toc::parse(&std::fs::read(p).unwrap()).unwrap();
        let records: usize = t.textures.iter().map(|x| x.records.len()).sum();
        println!("{} textures, {records} blocks", t.textures.len());
        assert_eq!(records, 21767);
        // chains run from small mips to large
        assert!(t.textures.iter().all(|x| x.records.windows(2).all(|w| w[0].largest_mip < w[1].largest_mip)));
        assert!(t.textures.iter().all(|x| t.find(x.key).is_some()));
    }
}

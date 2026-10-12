//! LZ4 frame reading. The Ragnarök PC `.wad` files are LZ4 frames (magic `04 22 4d 18`, 64 KiB blocks,
//! content size in the header); the decompressed stream starts with `WTOC`. Only what that needs is
//! implemented: block decoding, and reading a prefix of a frame without touching the rest of the file.

use anyhow::{bail, Result};
use std::io::Read;

pub const MAGIC: [u8; 4] = [0x04, 0x22, 0x4d, 0x18];

/// Decode one LZ4 block, appending to `out`. Matches may reach back into earlier output.
pub fn block_decode(src: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let mut i = 0usize;
    let len = |src: &[u8], i: &mut usize, mut n: usize| -> Result<usize> {
        if n == 15 {
            loop {
                let Some(b) = src.get(*i) else { bail!("lz4: length runs past the block") };
                *i += 1;
                n += *b as usize;
                if *b != 255 {
                    break;
                }
            }
        }
        Ok(n)
    };
    while i < src.len() {
        let token = src[i];
        i += 1;
        let lit = len(src, &mut i, (token >> 4) as usize)?;
        let Some(chunk) = src.get(i..i + lit) else { bail!("lz4: literals run past the block") };
        out.extend_from_slice(chunk);
        i += lit;
        if i >= src.len() {
            break; // the last sequence has literals only
        }
        let Some(off) = src.get(i..i + 2) else { bail!("lz4: truncated offset") };
        let off = off[0] as usize | (off[1] as usize) << 8;
        i += 2;
        if off == 0 || off > out.len() {
            bail!("lz4: bad match offset {off} (have {})", out.len());
        }
        let ml = len(src, &mut i, (token & 15) as usize)? + 4;
        let from = out.len() - off;
        for k in 0..ml {
            let b = out[from + k];
            out.push(b);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct FrameInfo {
    pub content_size: Option<u64>,
    pub block_max: usize,
    pub independent: bool,
    pub blocks_read: usize,
    /// True when the whole frame (up to its end mark) was read.
    pub complete: bool,
}

/// An LZ4 frame read one block at a time, so a file of any size can be walked through with little memory.
pub struct FrameReader<R: Read> {
    r: R,
    pub info: FrameInfo,
    block_checksum: bool,
    done: bool,
}

impl<R: Read> FrameReader<R> {
    /// Read the frame header.
    pub fn open(mut r: R) -> Result<Self> {
        let mut head = [0u8; 6];
        r.read_exact(&mut head)?;
        if head[..4] != MAGIC {
            bail!("not an LZ4 frame");
        }
        let (flg, bd) = (head[4], head[5]);
        if flg >> 6 != 1 {
            bail!("lz4: unsupported frame version");
        }
        let mut info = FrameInfo { independent: flg & 0x20 != 0, block_max: match (bd >> 4) & 7 { 4 => 64 << 10, 5 => 256 << 10, 6 => 1 << 20, 7 => 4 << 20, _ => bail!("lz4: bad block size code") }, ..Default::default() };
        if flg & 0x08 != 0 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            info.content_size = Some(u64::from_le_bytes(b));
        }
        if flg & 0x01 != 0 {
            let mut b = [0u8; 4];
            r.read_exact(&mut b)?;
        }
        let mut hc = [0u8; 1];
        r.read_exact(&mut hc)?;
        Ok(FrameReader { r, info, block_checksum: flg & 0x10 != 0, done: false })
    }

    /// Decompress the next block onto the end of `out` (a dependent block's matches reach up to 64 KiB back into
    /// what `out` already holds). False at the frame's end.
    pub fn next_block(&mut self, out: &mut Vec<u8>) -> Result<bool> {
        if self.done {
            return Ok(false);
        }
        let mut sz = [0u8; 4];
        self.r.read_exact(&mut sz)?;
        let sz = u32::from_le_bytes(sz);
        if sz == 0 {
            self.info.complete = true;
            self.done = true;
            return Ok(false);
        }
        let raw = sz >> 31 == 1;
        let n = (sz & 0x7fff_ffff) as usize;
        if n > self.info.block_max {
            bail!("lz4: block of {n} bytes exceeds the frame's maximum {}", self.info.block_max);
        }
        let mut buf = vec![0u8; n];
        self.r.read_exact(&mut buf)?;
        if self.block_checksum {
            let mut c = [0u8; 4];
            self.r.read_exact(&mut c)?;
        }
        if raw {
            out.extend_from_slice(&buf);
        } else {
            block_decode(&buf, out)?;
        }
        self.info.blocks_read += 1;
        Ok(true)
    }
}

/// Read and decompress the first `max_out` bytes (rounded up to a block) of the LZ4 frame in `r`.
pub fn read_prefix(r: impl Read, max_out: usize) -> Result<(Vec<u8>, FrameInfo)> {
    let mut f = FrameReader::open(r)?;
    let mut out = Vec::new();
    while out.len() < max_out && f.next_block(&mut out)? {}
    Ok((out, f.info))
}

/// Read a prefix from a file on disk.
pub fn read_file_prefix(path: &std::path::Path, max_out: usize) -> Result<(Vec<u8>, FrameInfo)> {
    read_prefix(std::io::BufReader::with_capacity(1 << 16, std::fs::File::open(path)?), max_out)
}

/// True when the file starts with the LZ4 frame magic.
pub fn is_frame(path: &std::path::Path) -> bool {
    let mut m = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut m)).map(|_| m == MAGIC).unwrap_or(false)
}

/// Test helper: a frame with one raw block and one compressed block (independent, content size set).
#[cfg(test)]
pub fn frame_for_tests(raw: &[u8], compressed_block: &[u8], content: u64) -> Vec<u8> {
    let mut f = MAGIC.to_vec();
    f.extend_from_slice(&[0x6c, 0x40]);
    f.extend_from_slice(&content.to_le_bytes());
    f.push(0x68);
    f.extend_from_slice(&(raw.len() as u32 | 0x8000_0000).to_le_bytes());
    f.extend_from_slice(raw);
    f.extend_from_slice(&(compressed_block.len() as u32).to_le_bytes());
    f.extend_from_slice(compressed_block);
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&[0, 0, 0, 0]);
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_literals_and_overlapping_matches() {
        // "ab" then a match of offset 2, length 10 (overlapping copy), then literals "xyz".
        // token 0x2 6 -> 2 literals, match len 6+4 ; then final sequence with 3 literals.
        let block = [0x26, b'a', b'b', 0x02, 0x00, 0x30, b'x', b'y', b'z'];
        let mut out = Vec::new();
        block_decode(&block, &mut out).unwrap();
        assert_eq!(out, b"ababababababxyz");
    }

    #[test]
    fn extended_lengths() {
        // 20 literals: token 0xF0, extra 5; then end.
        let mut block = vec![0xF0, 5];
        block.extend(std::iter::repeat(7u8).take(20));
        let mut out = Vec::new();
        block_decode(&block, &mut out).unwrap();
        assert_eq!(out, vec![7u8; 20]);
    }

    #[test]
    fn bad_offsets_are_errors_not_panics() {
        let mut out = Vec::new();
        assert!(block_decode(&[0x10, b'a', 0x09, 0x00], &mut out).is_err());
        assert!(block_decode(&[0xF0], &mut Vec::new()).is_err());
    }

    #[test]
    fn frame_with_raw_and_compressed_blocks() {
        let comp = [0x26, b'a', b'b', 0x02, 0x00, 0x30, b'x', b'y', b'z'];
        let f = frame_for_tests(b"WTOC1234", &comp, 23);
        let (out, info) = read_prefix(&f[..], usize::MAX).unwrap();
        assert_eq!(out, b"WTOC1234ababababababxyz");
        assert!(info.complete);
        assert_eq!(info.content_size, Some(23));
        assert!(info.independent);
        assert_eq!(info.blocks_read, 2);
        // a prefix read stops early
        let (out, info) = read_prefix(&f[..], 4).unwrap();
        assert_eq!(out, b"WTOC1234");
        assert!(!info.complete);
    }

    /// The first 140 bytes of a real Ragnarök PC WAD (`r_ui.wad`, from a player's diagnose report).
    #[test]
    fn a_real_wad_head_decodes_to_wtoc() {
        let hex = "04224d186c4060d65c050000000069b54c0000f01057544f4302000000507b00000000a0059d5c00003003da0070da2f0000d2ba15000d020071fc113303058a0f1800d31500000008000000aaaaaabaaa0100c18711adaa5741445f525f5549";
        let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
        // The block is cut short, so decode what is there without the bounds check of a whole frame.
        let body = &bytes[19..]; // 15-byte frame header + 4-byte block size
        let mut out = Vec::new();
        let _ = block_decode(body, &mut out); // truncated: an error at the end is expected
        assert_eq!(&out[..4], b"WTOC");
        assert_eq!(&out[4..8], &[2, 0, 0, 0]);
        assert!(out.windows(8).any(|w| w == b"WAD_R_UI"), "{:02x?}", &out[..out.len().min(128)]);
    }
}

//! Ragnarök's level-of-detail packs (`exec/wad/pc_le/<area>.lodpack`), which hold the detailed levels of models
//! whose coarsest level sits in their WAD.
//!
//! A pack starts with a plain copy of its table of contents (its `.toc` file is the same table as an LZ4 frame):
//! u32 block count, u32 resource count, u32 0, u32 1, then 24-byte block records (u64 offset in the pack, u64 hash,
//! u32 bytes, u32 flags) and 24-byte resource records (u32 block, u32 offset in the block, u64 hash, u32 bytes, u32
//! flags). Blocks run back to back from the end of the table to the end of the pack and are stored plain: a block's
//! bytes are the sum of its resources' whatever its flag. A mesh draw names its resource by hash (`MESH_*` record
//! +0x68). Worked out from a player's `root.lodpack` (2.94 GB: 4,862 blocks, 19,259 resources).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, Context, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub offset: u64,
    pub hash: u64,
    pub bytes: u32,
    pub flags: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub block: u32,
    pub offset: u32,
    pub hash: u64,
    pub bytes: u32,
    pub flags: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Toc {
    pub blocks: Vec<Block>,
    pub resources: Vec<Resource>,
    by_hash: HashMap<u64, usize>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

impl Toc {
    /// The table's length in bytes, from its first 16 bytes.
    pub fn table_len(head: &[u8]) -> Option<usize> {
        let (blocks, resources) = (u32_at(head, 0)? as usize, u32_at(head, 4)? as usize);
        if blocks == 0 || blocks > 1 << 22 || resources > 1 << 24 || u32_at(head, 8)? != 0 {
            return None;
        }
        Some(16 + 24 * (blocks + resources))
    }

    pub fn parse(b: &[u8]) -> Option<Toc> {
        let len = Self::table_len(b)?;
        if b.len() < len {
            return None;
        }
        let nb = u32_at(b, 0)? as usize;
        let nr = u32_at(b, 4)? as usize;
        let blocks = (0..nb)
            .map(|i| {
                let o = 16 + 24 * i;
                Some(Block { offset: u64_at(b, o)?, hash: u64_at(b, o + 8)?, bytes: u32_at(b, o + 16)?, flags: u32_at(b, o + 20)? })
            })
            .collect::<Option<Vec<_>>>()?;
        let resources = (0..nr)
            .map(|i| {
                let o = 16 + 24 * (nb + i);
                Some(Resource { block: u32_at(b, o)?, offset: u32_at(b, o + 4)?, hash: u64_at(b, o + 8)?, bytes: u32_at(b, o + 16)?, flags: u32_at(b, o + 20)? })
            })
            .collect::<Option<Vec<_>>>()?;
        let by_hash = resources.iter().enumerate().map(|(i, r)| (r.hash, i)).collect();
        Some(Toc { blocks, resources, by_hash })
    }

    /// The table at the start of a pack, checked against where its first block starts.
    pub fn read_from_pack(path: &Path) -> Result<Toc> {
        let mut f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
        let mut head = [0u8; 16];
        f.read_exact(&mut head)?;
        let Some(len) = Self::table_len(&head) else { bail!("{}: no table of contents at the start", path.display()) };
        if len > 256 << 20 {
            bail!("{}: table of {len} bytes is too big", path.display());
        }
        let mut b = head.to_vec();
        b.resize(len, 0);
        f.read_exact(&mut b[16..])?;
        let Some(toc) = Self::parse(&b) else { bail!("{}: unreadable table of contents", path.display()) };
        if toc.blocks.first().map(|x| x.offset) != Some(len as u64) {
            bail!("{}: the first block does not start where the table ends ({len})", path.display());
        }
        Ok(toc)
    }

    /// Where the resource named `hash` sits in the pack, and its length.
    pub fn find(&self, hash: u64) -> Option<(u64, u32)> {
        let r = &self.resources[*self.by_hash.get(&hash)?];
        let b = self.blocks.get(r.block as usize)?;
        (r.offset as u64 + r.bytes as u64 <= b.bytes as u64).then_some((b.offset + r.offset as u64, r.bytes))
    }

    /// The bytes of the resource named `hash`, read from the pack at `path`.
    pub fn read(&self, f: &mut std::fs::File, hash: u64) -> Result<Option<Vec<u8>>> {
        let Some((at, n)) = self.find(hash) else { return Ok(None) };
        let mut v = vec![0u8; n as usize];
        f.seek(SeekFrom::Start(at))?;
        f.read_exact(&mut v)?;
        Ok(Some(v))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A pack with two blocks holding three resources.
    pub(crate) fn pack(resources: &[(u64, &[u8], u32)]) -> Vec<u8> {
        let blocks: Vec<u32> = {
            let mut b: Vec<u32> = resources.iter().map(|r| r.2).collect();
            b.dedup();
            b
        };
        let len = 16 + 24 * (blocks.len() + resources.len());
        let mut out = Vec::new();
        out.extend((blocks.len() as u32).to_le_bytes());
        out.extend((resources.len() as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        let mut at = len as u64;
        let mut placed = Vec::new();
        for (k, b) in blocks.iter().enumerate() {
            let inside: Vec<_> = resources.iter().filter(|r| r.2 == *b).collect();
            let bytes: u32 = inside.iter().map(|r| r.1.len() as u32).sum();
            out.extend(at.to_le_bytes());
            out.extend((0xB10C_0000u64 + k as u64).to_le_bytes());
            out.extend(bytes.to_le_bytes());
            out.extend((k as u32 & 1).to_le_bytes());
            let mut off = 0u32;
            for r in inside {
                placed.push((*b, off, r.0, r.1.len() as u32));
                off += r.1.len() as u32;
            }
            at += bytes as u64;
        }
        for (b, off, h, n) in &placed {
            out.extend(b.to_le_bytes());
            out.extend(off.to_le_bytes());
            out.extend(h.to_le_bytes());
            out.extend(n.to_le_bytes());
            out.extend(0u32.to_le_bytes());
        }
        for r in resources {
            out.extend_from_slice(r.1);
        }
        out
    }

    #[test]
    fn resources_are_found_by_hash_and_read_from_the_pack() {
        let p = pack(&[(0x11, b"first", 0), (0x22, b"second!", 0), (0x33, b"third", 1)]);
        let dir = std::env::temp_dir().join(format!("gowring-lodpack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("root.lodpack");
        std::fs::write(&path, &p).unwrap();
        let toc = Toc::read_from_pack(&path).unwrap();
        assert_eq!((toc.blocks.len(), toc.resources.len()), (2, 3));
        let mut f = std::fs::File::open(&path).unwrap();
        assert_eq!(toc.read(&mut f, 0x22).unwrap().as_deref(), Some(&b"second!"[..]));
        assert_eq!(toc.read(&mut f, 0x33).unwrap().as_deref(), Some(&b"third"[..]));
        assert_eq!(toc.read(&mut f, 0x44).unwrap(), None);
        // a file that does not start with a table is refused
        std::fs::write(&path, b"not a pack at all, just words").unwrap();
        assert!(Toc::read_from_pack(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With a player's `root.lodpack.toc` decompressed (or the first MB of the pack):
    /// `GOWRING_LODPACK_TOC=... cargo test -p gowring-gow real_lodpack -- --ignored`.
    #[test]
    #[ignore]
    fn real_lodpack_table() {
        let Ok(p) = std::env::var("GOWRING_LODPACK_TOC") else { return };
        let b = std::fs::read(p).unwrap();
        let toc = Toc::parse(&b).unwrap();
        println!("{} blocks, {} resources", toc.blocks.len(), toc.resources.len());
        assert_eq!(toc.blocks[0].offset as usize, Toc::table_len(&b).unwrap());
        for w in toc.blocks.windows(2) {
            assert_eq!(w[0].offset + w[0].bytes as u64, w[1].offset);
        }
        for r in &toc.resources {
            assert!(toc.find(r.hash).is_some());
        }
    }
}

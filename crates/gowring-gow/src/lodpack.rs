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

/// Resources by hash from the level-of-detail packs beside a WAD: `root.lodpack` (where all of Kratos's are) first,
/// and for a hash it lacks the other `*.lodpack` files in its folder, whose tables are read once, the first time one
/// is needed (a weapon's detailed draws may live in the pack of the area that shows it first). Each resource is read
/// once and kept. The body part keeps reading `root.lodpack` alone through [`Toc`].
pub struct LodPacks {
    dir: std::path::PathBuf,
    packs: Vec<(std::path::PathBuf, Toc)>,
    others_read: bool,
    cache: HashMap<u64, (usize, Vec<u8>)>,
    /// What reading the other packs' tables found: how many, and those that did not read.
    pub note: String,
}

impl LodPacks {
    /// The packs, starting from `root` (`root.lodpack`), whose table must read.
    pub fn open(root: &Path) -> Result<LodPacks> {
        let toc = Toc::read_from_pack(root)?;
        let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(LodPacks { dir, packs: vec![(root.to_path_buf(), toc)], others_read: false, cache: HashMap::new(), note: String::new() })
    }

    /// The bytes of the resource `hash`, from the first pack that lists it (None when none does).
    pub fn get(&mut self, hash: u64) -> Result<Option<&[u8]>> {
        if !self.cache.contains_key(&hash) {
            let mut at = self.packs.iter().position(|(_, t)| t.find(hash).is_some());
            if at.is_none() && !self.others_read {
                self.read_others();
                at = self.packs.iter().position(|(_, t)| t.find(hash).is_some());
            }
            let Some(k) = at else { return Ok(None) };
            let (path, toc) = &self.packs[k];
            let mut f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
            let Some(bytes) = toc.read(&mut f, hash)? else { return Ok(None) };
            self.cache.insert(hash, (k, bytes));
        }
        Ok(self.cache.get(&hash).map(|(_, b)| b.as_slice()))
    }

    /// The file name of the pack a resource already read came from.
    pub fn pack_of(&self, hash: u64) -> Option<String> {
        let (k, _) = self.cache.get(&hash)?;
        self.packs.get(*k).and_then(|(p, _)| p.file_name()).map(|n| n.to_string_lossy().into_owned())
    }

    /// How many packs' tables are read so far.
    pub fn packs_read(&self) -> usize {
        self.packs.len()
    }

    fn read_others(&mut self) {
        self.others_read = true;
        let first = self.packs[0].0.clone();
        let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(&self.dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lodpack")) && *p != first).collect()).unwrap_or_default();
        names.sort();
        let mut failed = Vec::new();
        for p in names {
            match Toc::read_from_pack(&p) {
                Ok(t) => self.packs.push((p, t)),
                Err(_) => failed.push(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            }
        }
        self.note = format!("{} other pack table(s) read{}", self.packs.len() - 1, if failed.is_empty() { String::new() } else { format!(", {} did not read ({})", failed.len(), failed.join(", ")) });
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

    #[test]
    fn a_hash_root_lacks_is_found_in_the_other_packs_whose_tables_read_once() {
        let dir = std::env::temp_dir().join(format!("gowring-lodpacks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.lodpack"), pack(&[(0x11, b"in root", 0)])).unwrap();
        std::fs::write(dir.join("valhalla.lodpack"), pack(&[(0x22, b"in valhalla", 0), (0x11, b"a copy", 0)])).unwrap();
        // not a pack: its table does not read, and it is named in the note
        std::fs::write(dir.join("broken.lodpack"), b"nothing here at all").unwrap();
        std::fs::write(dir.join("root.lodpack.toc"), b"an LZ4 copy of the table").unwrap();
        let mut packs = LodPacks::open(&dir.join("root.lodpack")).unwrap();
        // root first, even when another pack has the hash too; the others are not read for it
        assert_eq!(packs.get(0x11).unwrap(), Some(&b"in root"[..]));
        assert_eq!(packs.packs_read(), 1);
        assert_eq!(packs.get(0x22).unwrap(), Some(&b"in valhalla"[..]));
        assert_eq!(packs.packs_read(), 2);
        assert_eq!(packs.pack_of(0x22).as_deref(), Some("valhalla.lodpack"));
        assert!(packs.note.contains("1 other pack table(s) read") && packs.note.contains("broken.lodpack"), "{}", packs.note);
        assert_eq!(packs.get(0x33).unwrap(), None);
        assert_eq!(packs.packs_read(), 2);
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

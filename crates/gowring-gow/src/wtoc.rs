//! Ragnarök PC `.wad` tables (WTOC version 2, inside an LZ4 frame), with the data layout worked out.
//!
//! The table is a 0x40-byte header and then one 0x90-byte entry per chunk. An entry's `mem_off` is NOT a file offset:
//! it is the chunk's place inside one of nine memory areas ("arenas": cpu, gpu, low texture mips, debug, temp, ...).
//! The data after the table is a run of load blocks; each block stores its share of every arena one after the other
//! (arena 0's bytes, then arena 1's, ...). A chunk's file offset is therefore
//! `start of its arena within its block + (mem_off - everything that arena held in earlier blocks)`.
//! An entry with bit 0 of `bits` set ends a load block. Chunk id 25 entries are padding markers: they take no
//! space in their arena and add their length after the block.
//!
//! Field layout and the layout rule follow the public community documentation of the format (the smpack and
//! GoWToolkit projects); this is our own implementation of it.

pub const MAGIC: u32 = 0x434F_5457; // "WTOC"
pub const HEADER_LEN: usize = 0x40;
pub const ENTRY_LEN: usize = 0x90;
pub const ARENAS: usize = 9;
/// Chunk id of a padding marker.
pub const PAD_CHUNK: u16 = 25;
/// Arenas that hold debug and temporary side data.
const DEBUG_ARENA: usize = 5;
const TEMP_ARENA: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u16,
    pub version: u16,
    pub length: u32,
    pub name: String,
    pub debug_size: u32,
    pub temp_size: u32,
    pub align: u32,
    pub client: u8,
    pub server: u8,
    pub mem_type: u8,
    pub bits: u8,
    pub mem_off: u64,
    /// Where the chunk's bytes start in the decompressed file (filled by `layout`).
    pub file_off: u64,
    /// Which load block it belongs to (filled by `layout`).
    pub block: u32,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Number of entries, or `None` when this is not a WTOC v2 table.
pub fn count(buf: &[u8]) -> Option<usize> {
    if buf.len() < HEADER_LEN || u32_at(buf, 0) != MAGIC || u32_at(buf, 4) != 2 {
        return None;
    }
    Some(u32_at(buf, 8) as usize)
}

/// Read the table (as many entries as `buf` holds) and lay the data out.
pub fn read(buf: &[u8]) -> Option<Vec<Entry>> {
    let n = count(buf)?;
    let mut es = Vec::with_capacity(n.min(1 << 20));
    for i in 0..n {
        let o = HEADER_LEN + i * ENTRY_LEN;
        if o + ENTRY_LEN > buf.len() {
            return None;
        }
        let e = &buf[o..o + ENTRY_LEN];
        let raw = &e[0x18..0x50];
        let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
        es.push(Entry {
            id: u16_at(e, 0),
            version: u16_at(e, 2),
            length: u32_at(e, 4),
            name: String::from_utf8_lossy(&raw[..end]).into_owned(),
            debug_size: u32_at(e, 0x60),
            temp_size: u32_at(e, 0x64),
            align: u32_at(e, 0x68),
            client: e[0x6C],
            server: e[0x6D],
            mem_type: e[0x6F],
            bits: e[0x72],
            mem_off: u64_at(e, 0x78),
            file_off: 0,
            block: 0,
        });
    }
    let data_start = (HEADER_LEN + n * ENTRY_LEN) as u64;
    layout(&mut es, data_start);
    Some(es)
}

fn align_up(v: u64, a: u64) -> u64 {
    if a <= 1 {
        v
    } else {
        v.div_ceil(a) * a
    }
}

/// Fill `file_off` and `block` for every entry. Returns where the laid-out data ends.
pub fn layout(es: &mut [Entry], data_start: u64) -> u64 {
    let mut base = [0u64; ARENAS]; // what each arena held in earlier blocks
    let mut size = [0u64; ARENAS]; // what it holds in this block so far
    let mut pos = data_start;
    let mut pad_after = 0u64;
    let mut first = 0usize;
    let mut block = 0u32;
    for i in 0..es.len() {
        let t = (es[i].mem_type as usize).min(ARENAS - 1);
        let start = align_up(base[t] + size[t], es[i].align.max(1) as u64);
        if es[i].id == PAD_CHUNK {
            pad_after = es[i].length as u64;
            size[t] = start - base[t];
        } else {
            size[t] = start - base[t] + es[i].length as u64;
        }
        size[DEBUG_ARENA] += es[i].debug_size as u64;
        size[TEMP_ARENA] += es[i].temp_size as u64;
        es[i].block = block;
        if es[i].bits & 1 != 0 || i + 1 == es.len() {
            let mut region = [0u64; ARENAS];
            let mut p = pos;
            for k in 0..ARENAS {
                region[k] = p;
                p += size[k];
            }
            for e in es[first..=i].iter_mut() {
                let tt = (e.mem_type as usize).min(ARENAS - 1);
                e.file_off = if e.id == PAD_CHUNK { p } else { region[tt] + e.mem_off.saturating_sub(base[tt]) };
            }
            for k in 0..ARENAS {
                base[k] += size[k];
                size[k] = 0;
            }
            pos = p + pad_after;
            pad_after = 0;
            first = i + 1;
            block += 1;
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u16, name: &str, len: u32, mem_type: u8, mem_off: u64, align: u32, bits: u8) -> Vec<u8> {
        let mut e = vec![0u8; ENTRY_LEN];
        e[0..2].copy_from_slice(&id.to_le_bytes());
        e[4..8].copy_from_slice(&len.to_le_bytes());
        e[0x18..0x18 + name.len()].copy_from_slice(name.as_bytes());
        e[0x68..0x6C].copy_from_slice(&align.to_le_bytes());
        e[0x6F] = mem_type;
        e[0x72] = bits;
        e[0x78..0x80].copy_from_slice(&mem_off.to_le_bytes());
        e
    }

    /// Two load blocks, two arenas, alignment and a padding marker: every chunk must land on its own bytes.
    #[test]
    fn chunks_land_on_their_bytes_across_blocks_and_arenas() {
        // block 0: cpu "A" (5 bytes), gpu "B" (8 bytes, align 16), cpu "C" (3 bytes) ends the block
        // block 1: pad marker (4 bytes after the block), cpu "D" (2 bytes), gpu "E" (16 bytes) ends it
        let es = [
            entry(1, "A", 5, 0, 0, 1, 0),
            entry(29, "B", 8, 1, 0, 16, 0),
            entry(1, "C", 3, 0, 5, 1, 1),
            entry(PAD_CHUNK, "pad", 4, 0, 8, 1, 0),
            entry(1, "D", 2, 0, 8, 1, 0),
            entry(29, "E", 16, 1, 16, 16, 1),
        ];
        let mut buf = vec![0u8; HEADER_LEN];
        buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&2u32.to_le_bytes());
        buf[8..12].copy_from_slice(&(es.len() as u32).to_le_bytes());
        for e in &es {
            buf.extend_from_slice(e);
        }
        let start = buf.len();
        // block 0: cpu arena "AAAAACCC" (8), gpu arena "BBBBBBBB" (8)
        buf.extend_from_slice(b"AAAAACCC");
        buf.extend_from_slice(b"BBBBBBBB");
        // block 1: cpu "DD", gpu: 8 bytes of alignment fill then "E"x16, then the 4 pad bytes
        buf.extend_from_slice(b"DD");
        buf.extend_from_slice(&[0u8; 8]);
        buf.extend_from_slice(&[b'E'; 16]);
        buf.extend_from_slice(&[0xEE; 4]);
        let got = read(&buf).unwrap();
        let at = |n: &str| got.iter().find(|e| e.name == n).map(|e| (e.file_off as usize, e.length as usize)).unwrap();
        for n in ["A", "B", "C", "D", "E"] {
            let (o, l) = at(n);
            assert!(buf[o..o + l].iter().all(|b| *b == n.as_bytes()[0]), "{n} at {o:#x} (data starts {start:#x}): {:?}", &buf[o..o + l]);
        }
        assert_eq!(got.iter().map(|e| e.block).collect::<Vec<_>>(), vec![0, 0, 0, 1, 1, 1]);
        let mut es2 = got.clone();
        assert_eq!(layout(&mut es2, start as u64) as usize, buf.len(), "the layout accounts for every byte, padding included");
    }

    #[test]
    fn not_a_table() {
        assert!(read(b"nope").is_none());
        assert!(count(&[0u8; 0x40]).is_none());
    }
}

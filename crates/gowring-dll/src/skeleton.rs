//! The player character's skeleton as the game holds it: Havok's hkaSkeleton (bone names, parent indices and
//! reference pose), read once per session from memory and written to `%LOCALAPPDATA%\GowRing\er_skeleton.json`.
//! The launcher binds Kratos's mesh to these bones when it builds his body part. Read-only, off the game's thread:
//! every read goes through `ReadProcessMemory` on our own process, which fails cleanly on a bad address, and the
//! skeleton must look like one (matching array sizes, readable names) before anything is written.
//!
//! 0.1.43-0.1.45 checked each address with `VirtualQuery` before reading it, byte by byte for names, on the game's
//! main thread. In Elden Ring's process that call takes milliseconds: the read stalled the game for 10 s in 0.1.44,
//! and in 0.1.45 the skeleton read (30 s) and the file-table walk after it froze the game for good.
//!
//! Path, as the community's ERSkeletonMan documents it: ChrIns -> module container -> behaviour module (+0x28 in the
//! container) -> +0x10 -> +0x30 (hkbCharacter) -> +0x90 -> +0x28 = hkaSkeleton. hkaSkeleton (Havok 2018, x64, after a
//! 0x18-byte hkReferencedObject; as soulstruct-havok's type tables give it): +0x18 name, +0x20 parent indices (hkArray
//! of i16), +0x30 bones (hkArray of 16-byte hkaBone, name first), +0x40 reference pose (hkArray of 48-byte
//! hkQsTransform: translation, rotation quaternion, scale; each 4 floats). 0.1.43 read it 8 bytes early and missed.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Threading::GetCurrentProcess;

static DONE: AtomicBool = AtomicBool::new(false);
static READS: AtomicU64 = AtomicU64::new(0);

/// How many reads have been made so far (for the log).
pub(crate) fn reads() -> u64 {
    READS.load(Ordering::Relaxed)
}

/// Copy `buf.len()` bytes from `addr`; false (and nothing promised about `buf`) when any of it is not readable.
pub(crate) fn read_into(addr: usize, buf: &mut [u8]) -> bool {
    if addr < 0x10000 || addr.checked_add(buf.len()).is_none() {
        return false;
    }
    READS.fetch_add(1, Ordering::Relaxed);
    let mut got = 0usize;
    let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), addr as *const _, buf.as_mut_ptr() as *mut _, buf.len(), Some(&mut got)) };
    ok.is_ok() && got == buf.len()
}

/// A `T` read from `addr` (any plain-data type: integers, floats, arrays of them).
pub(crate) fn read<T: Copy>(addr: usize) -> Option<T> {
    let mut v = std::mem::MaybeUninit::<T>::uninit();
    let buf = unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u8, std::mem::size_of::<T>()) };
    read_into(addr, buf).then(|| unsafe { v.assume_init() })
}

/// `n` consecutive `T`s from `addr`, in one read.
pub(crate) fn read_vec<T: Copy + Default>(addr: usize, n: usize) -> Option<Vec<T>> {
    let mut v = vec![T::default(); n];
    let buf = unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u8, n * std::mem::size_of::<T>()) };
    read_into(addr, buf).then_some(v)
}

fn c_string(addr: usize) -> Option<String> {
    let addr = addr & !1; // hkStringPtr keeps a flag in the low bit
    let mut s = Vec::new();
    // in 16-byte pieces, so a name that ends just before unreadable memory still reads
    for piece in 0..8 {
        let chunk: [u8; 16] = read(addr + 16 * piece)?;
        for c in chunk {
            if c == 0 {
                return String::from_utf8(s).ok().filter(|s: &String| !s.is_empty());
            }
            if !(0x20..0x7f).contains(&c) {
                return None;
            }
            s.push(c);
        }
    }
    None
}

pub struct Bone {
    pub name: String,
    pub parent: i16,
    /// Translation, rotation (x, y, z, w) and scale, relative to the parent.
    pub t: [f32; 3],
    pub r: [f32; 4],
    pub s: [f32; 3],
}

/// The hkaSkeleton at `skel`, if it is one: Havok 2018's layout first, then one with a 0x10-byte base.
fn skeleton_at(skel: usize) -> Option<(String, Vec<Bone>)> {
    skeleton_with_base(skel, 0x18).or_else(|| skeleton_with_base(skel, 0x10))
}

fn skeleton_with_base(skel: usize, base: usize) -> Option<(String, Vec<Bone>)> {
    let name = read::<usize>(skel + base).and_then(c_string).unwrap_or_default();
    let array = |off: usize| -> Option<(usize, usize)> { Some((read::<usize>(skel + off)?, read::<i32>(skel + off + 8)? as usize)) };
    let (parents, n) = array(base + 0x08)?;
    let (bones, n2) = array(base + 0x18)?;
    let (pose, n3) = array(base + 0x28)?;
    if !(20..=1024).contains(&n) || n != n2 || n != n3 {
        return None;
    }
    let parents: Vec<i16> = read_vec(parents, n)?;
    let bones: Vec<[usize; 2]> = read_vec(bones, n)?;
    let pose: Vec<[f32; 12]> = read_vec(pose, n)?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let parent = parents[i];
        if parent < -1 || parent as isize >= i as isize {
            return None;
        }
        let f = &pose[i];
        out.push(Bone { name: c_string(bones[i][0])?, parent, t: [f[0], f[1], f[2]], r: [f[4], f[5], f[6], f[7]], s: [f[8], f[9], f[10]] });
    }
    Some((name, out))
}

/// Follow the documented path from the behaviour module, logging each step, and write the skeleton once.
pub fn dump_once(behavior_module: usize) {
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    let (t0, r0) = (std::time::Instant::now(), reads());
    let mut trail = String::new();
    let mut at = behavior_module;
    let mut found = None;
    for (k, off) in [0x10usize, 0x30, 0x90, 0x28, 0x0].iter().enumerate() {
        let Some(next) = read::<usize>(at + off) else {
            let _ = write!(trail, " | step {k} (+{off:#x}) unreadable at {at:#x}");
            break;
        };
        let _ = write!(trail, " | +{off:#x} -> {next:#x}");
        at = next;
        // the last two steps: take the skeleton wherever it validates
        if k >= 3 {
            if let Some(s) = skeleton_at(at) {
                found = Some(s);
                break;
            }
        }
    }
    let Some((name, bones)) = found else {
        // the objects along the way, for working out the layout from the log
        let mut dump = String::new();
        let mut at = behavior_module;
        for off in [0x10usize, 0x30, 0x90, 0x28] {
            let Some(next) = read::<usize>(at + off) else { break };
            at = next;
            let words: Vec<String> = (0..16).map(|k| read::<u64>(at + 8 * k).map(|w| format!("{w:016x}")).unwrap_or_else(|| "?".into())).collect();
            let _ = write!(dump, "\n    {at:#x}: {}", words.join(" "));
        }
        crate::glog!("skeleton", "player skeleton not found from the behaviour module at {behavior_module:#x}{trail}{dump} ({} reads, {} ms)", reads() - r0, t0.elapsed().as_millis());
        return;
    };
    let mut j = format!("{{\n  \"skeleton\": {:?},\n  \"bones\": [\n", name);
    for (i, b) in bones.iter().enumerate() {
        let _ = write!(
            j,
            "    {{\"name\": {:?}, \"parent\": {}, \"t\": [{}, {}, {}], \"r\": [{}, {}, {}, {}], \"s\": [{}, {}, {}]}}{}\n",
            b.name,
            b.parent,
            b.t[0],
            b.t[1],
            b.t[2],
            b.r[0],
            b.r[1],
            b.r[2],
            b.r[3],
            b.s[0],
            b.s[1],
            b.s[2],
            if i + 1 < bones.len() { "," } else { "" }
        );
    }
    j.push_str("  ]\n}\n");
    let path = crate::paths::get().local.join("er_skeleton.json");
    let wrote = std::fs::create_dir_all(&crate::paths::get().local).and_then(|_| std::fs::write(&path, j));
    crate::glog!(
        "skeleton",
        "player skeleton {name:?}: {} bones (first {:?}); {}{trail} ({} reads, {} ms)",
        bones.len(),
        bones.iter().take(6).map(|b| b.name.as_str()).collect::<Vec<_>>(),
        match wrote {
            Ok(()) => format!("written to {}", path.display()),
            Err(e) => format!("could not write it: {e}"),
        },
        reads() - r0,
        t0.elapsed().as_millis()
    );
}

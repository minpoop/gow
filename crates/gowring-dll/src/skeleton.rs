//! The player character's skeleton as the game holds it: Havok's hkaSkeleton (bone names, parent indices and
//! reference pose), read once per session from memory and written to `%LOCALAPPDATA%\GowRing\er_skeleton.json`.
//! The launcher binds Kratos's mesh to these bones when it builds his body part. Read-only: every pointer is checked
//! readable before it is followed, and the skeleton must look like one (matching array sizes, readable names)
//! before anything is written.
//!
//! Path, as the community's ERSkeletonMan documents it: ChrIns -> module container -> behaviour module (+0x28 in the
//! container) -> +0x10 -> +0x30 (hkbCharacter) -> +0x90 -> +0x28 -> +0 = hkaSkeleton. hkaSkeleton (Havok 2018, x64):
//! +0x10 name, +0x18 parent indices (hkArray of i16), +0x28 bones (hkArray of 16-byte hkaBone, name first), +0x38
//! reference pose (hkArray of 48-byte hkQsTransform: translation, rotation quaternion, scale; each 4 floats).

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS};

static DONE: AtomicBool = AtomicBool::new(false);

/// True when `len` bytes from `addr` are committed, readable memory.
fn readable(addr: usize, len: usize) -> bool {
    if addr < 0x10000 || addr.checked_add(len).is_none() {
        return false;
    }
    let mut at = addr;
    while at < addr + len {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let got = unsafe { VirtualQuery(Some(at as *const _), &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || mbi.State != MEM_COMMIT || (mbi.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0)) != 0 || mbi.Protect.0 == 0 {
            return false;
        }
        at = mbi.BaseAddress as usize + mbi.RegionSize;
    }
    true
}

fn read<T: Copy>(addr: usize) -> Option<T> {
    readable(addr, std::mem::size_of::<T>()).then(|| unsafe { std::ptr::read_unaligned(addr as *const T) })
}

fn c_string(addr: usize) -> Option<String> {
    let addr = addr & !1; // hkStringPtr keeps a flag in the low bit
    let mut s = Vec::new();
    for i in 0..128 {
        let c: u8 = read(addr + i)?;
        if c == 0 {
            return String::from_utf8(s).ok().filter(|s: &String| !s.is_empty());
        }
        if !(0x20..0x7f).contains(&c) {
            return None;
        }
        s.push(c);
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

/// The hkaSkeleton at `skel`, if it is one.
fn skeleton_at(skel: usize) -> Option<(String, Vec<Bone>)> {
    let name = c_string(read::<usize>(skel + 0x10)?).unwrap_or_default();
    let array = |off: usize| -> Option<(usize, usize)> { Some((read::<usize>(skel + off)?, read::<i32>(skel + off + 8)? as usize)) };
    let (parents, n) = array(0x18)?;
    let (bones, n2) = array(0x28)?;
    let (pose, n3) = array(0x38)?;
    if !(20..=1024).contains(&n) || n != n2 || n != n3 || !readable(parents, 2 * n) || !readable(bones, 16 * n) || !readable(pose, 48 * n) {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let parent: i16 = read(parents + 2 * i)?;
        if parent < -1 || parent as isize >= i as isize {
            return None;
        }
        let p = pose + 48 * i;
        let f = |k: usize| read::<f32>(p + 4 * k);
        out.push(Bone {
            name: c_string(read::<usize>(bones + 16 * i)?)?,
            parent,
            t: [f(0)?, f(1)?, f(2)?],
            r: [f(4)?, f(5)?, f(6)?, f(7)?],
            s: [f(8)?, f(9)?, f(10)?],
        });
    }
    Some((name, out))
}

/// Follow the documented path from the behaviour module, logging each step, and write the skeleton once.
pub fn dump_once(behavior_module: usize) {
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
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
        crate::glog!("skeleton", "player skeleton not found from the behaviour module at {behavior_module:#x}{trail}");
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
        "player skeleton {name:?}: {} bones (first {:?}); {}{trail}",
        bones.len(),
        bones.iter().take(6).map(|b| b.name.as_str()).collect::<Vec<_>>(),
        match wrote {
            Ok(()) => format!("written to {}", path.display()),
            Err(e) => format!("could not write it: {e}"),
        }
    );
}

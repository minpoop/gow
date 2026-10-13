//! What the game made of our part files. Every file Elden Ring asks for gets an FD4FileCap in CSFile's repository: a
//! hash table of caps, each with the file's name, reference count and load state (initial, queued, processing, ready).
//! A few times after the player is in the world this logs the caps for our part ids (9701-9703) and a sample of the
//! game's own part files for comparison. Read-only, and every pointer is checked readable before it is followed.

use std::fmt::Write as _;
use std::mem::{offset_of, size_of};

use eldenring::cs::{CSFileImp, CSFileRepository};
use eldenring::fd4::{FD4FileCap, FD4ResCap, FD4ResCapHolder};

use crate::skeleton::{read, readable};

/// The file ids of the parts the launcher installs.
const OURS: [&str; 3] = ["9701", "9702", "9703"];

/// CSFile's primary file repository, through the game's singleton table.
fn file_repository() -> Option<usize> {
    let slot = from_singleton::map().get("CSFile")?;
    let imp: usize = read(slot.as_ptr() as usize)?;
    read::<usize>(imp + offset_of!(CSFileImp, file_repository_1))
}

/// A cap's name. FD4BasicHashString is a vtable and then the game's UTF-16 std::basic_string (allocator, 16 bytes of
/// inline text or a pointer to it, length, capacity: the text is inline while the capacity is under 8 units).
fn cap_name(cap: usize) -> Option<String> {
    let s = cap + offset_of!(FD4FileCap, res_cap) + offset_of!(FD4ResCap, name) + 8;
    let (len, capacity) = (read::<usize>(s + 24)?, read::<usize>(s + 32)?);
    if len > capacity || len > 1024 {
        return None;
    }
    let text = if capacity < 8 { s + 8 } else { read::<usize>(s + 8)? };
    if !readable(text, 2 * len) {
        return None;
    }
    let units = unsafe { std::slice::from_raw_parts(text as *const u16, len) };
    Some(String::from_utf16_lossy(units))
}

fn state(s: u8) -> &'static str {
    match s {
        0 => "initial",
        1 => "queued",
        2 => "processing",
        3 => "state 3",
        4 => "ready",
        _ => "unknown state",
    }
}

/// Log the caps for our part files (`when` says at what point of the session).
pub fn report(when: &str) {
    let Some(repo) = file_repository() else {
        let names: Vec<&String> = from_singleton::map().keys().filter(|k| k.contains("File")).collect();
        crate::glog!("parts", "{when}: the game's file table was not found (singletons named *File*: {names:?})");
        return;
    };
    let (mut total, mut parts) = (0usize, 0usize);
    let mut ours = Vec::new();
    let mut samples = Vec::new();
    for holder in [offset_of!(CSFileRepository, res_cap_holder), offset_of!(CSFileRepository, holder2)] {
        let h = repo + holder;
        let (Some(count), Some(buckets)) = (read::<u32>(h + offset_of!(FD4ResCapHolder<FD4FileCap>, bucket_count)), read::<usize>(h + offset_of!(FD4ResCapHolder<FD4FileCap>, buckets))) else {
            continue;
        };
        if count == 0 || count > 1 << 20 || !readable(buckets, 8 * count as usize) {
            continue;
        }
        for b in 0..count as usize {
            let mut cap = read::<usize>(buckets + 8 * b).unwrap_or(0);
            let mut steps = 0;
            while cap != 0 && steps < 4096 && readable(cap, size_of::<FD4FileCap>()) {
                steps += 1;
                total += 1;
                if let Some(name) = cap_name(cap) {
                    let lower = name.to_ascii_lowercase();
                    let st = read::<u8>(cap + offset_of!(FD4FileCap, load_state)).unwrap_or(0xFF);
                    let refs = read::<u32>(cap + offset_of!(FD4FileCap, res_cap) + offset_of!(FD4ResCap, reference_count)).unwrap_or(0);
                    if OURS.iter().any(|id| lower.contains(id)) {
                        ours.push(format!("{name} ({}, {refs} refs)", state(st)));
                    } else if lower.contains("parts") {
                        parts += 1;
                        if samples.len() < 6 {
                            samples.push(format!("{name} ({})", state(st)));
                        }
                    }
                }
                cap = read::<usize>(cap + offset_of!(FD4FileCap, res_cap) + offset_of!(FD4ResCap, next_item)).unwrap_or(0);
            }
        }
    }
    let mut line = format!("{when}: {total} files in the game's table; ours: ");
    if ours.is_empty() {
        line.push_str("none (the game has not asked for them)");
    } else {
        line.push_str(&ours.join(", "));
    }
    let _ = write!(line, "; {parts} of the game's own part files, e.g. {}", samples.join(", "));
    crate::glog!("parts", "{line}");
}

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Report at 5 s, 30 s and 2 min into the world (`secs` since the player appeared), once each per session.
pub fn report_due(secs: u64) {
    const AT: [u64; 3] = [5, 30, 120];
    let k = NEXT.load(std::sync::atomic::Ordering::SeqCst);
    if k < AT.len() && secs >= AT[k] {
        NEXT.store(k + 1, std::sync::atomic::Ordering::SeqCst);
        report(&format!("{secs} s into the world"));
    }
}

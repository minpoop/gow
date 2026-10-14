//! What the game made of our part files. Every file Elden Ring asks for gets an FD4FileCap in CSFile's repository: a
//! hash table of caps, each with the file's name, reference count and load state (initial, queued, processing, ready).
//! A few times after the player is in the world this logs the caps for our part ids (9700-9703), the head, face and
//! hair parts loaded, and a sample of the game's other part files for comparison. Read-only and off the game's thread: every read is a `ReadProcessMemory`
//! copy (one per cap and one per name), and the walk stops at a time budget and on overlong chains.

use std::fmt::Write as _;
use std::mem::{offset_of, size_of};

use eldenring::cs::{CSFileImp, CSFileRepository};
use eldenring::fd4::{FD4FileCap, FD4ResCap, FD4ResCapHolder};

use crate::skeleton::{read, read_vec, reads};

/// The file ids of the parts the launcher installs.
const OURS: [&str; 4] = ["9700", "9701", "9702", "9703"];

/// Name starts of the head, face and hair parts (the ones that can show around Kratos's head).
const HEADS: [&str; 4] = ["parts:/hd_", "parts:/fc_", "parts:/fg_", "parts:/hr_"];

/// CSFile's primary file repository, through the game's singleton table.
fn file_repository() -> Option<usize> {
    let slot = from_singleton::map().get("CSFile")?;
    let imp: usize = read(slot.as_ptr() as usize)?;
    read::<usize>(imp + offset_of!(CSFileImp, file_repository_1))
}

/// The bytes of one cap.
type Cap = [u8; size_of::<FD4FileCap>()];

fn field<T: Copy>(c: &Cap, at: usize) -> T {
    unsafe { std::ptr::read_unaligned(c[at..at + size_of::<T>()].as_ptr() as *const T) }
}

/// A cap's name. FD4BasicHashString is a vtable and then the game's UTF-16 std::basic_string (allocator, 16 bytes of
/// inline text or a pointer to it, length, capacity: the text is inline while the capacity is under 8 units).
fn cap_name(c: &Cap) -> Option<String> {
    let s = offset_of!(FD4FileCap, res_cap) + offset_of!(FD4ResCap, name) + 8;
    let (len, capacity): (usize, usize) = (field(c, s + 24), field(c, s + 32));
    if len > capacity || len > 1024 {
        return None;
    }
    let units: Vec<u16> = if capacity < 8 { (0..len).map(|k| field::<u16>(c, s + 8 + 2 * k)).collect() } else { read_vec(field::<usize>(c, s + 8), len)? };
    Some(String::from_utf16_lossy(&units))
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
    let (t0, r0) = (std::time::Instant::now(), reads());
    let budget = std::time::Duration::from_secs(3);
    let (mut total, mut parts, mut cut) = (0usize, 0usize, None);
    let mut ours = Vec::new();
    let mut samples = Vec::new();
    let mut head_parts = Vec::new();
    'walk: for holder in [offset_of!(CSFileRepository, res_cap_holder), offset_of!(CSFileRepository, holder2)] {
        let h = repo + holder;
        let (Some(count), Some(buckets)) = (read::<u32>(h + offset_of!(FD4ResCapHolder<FD4FileCap>, bucket_count)), read::<usize>(h + offset_of!(FD4ResCapHolder<FD4FileCap>, buckets))) else {
            continue;
        };
        if count == 0 || count > 1 << 20 {
            continue;
        }
        let Some(heads) = read_vec::<usize>(buckets, count as usize) else { continue };
        for head in heads {
            let mut at = head;
            let mut steps = 0;
            while at != 0 {
                steps += 1;
                if steps > 64 {
                    cut = Some("a bucket chain over 64 long");
                    break 'walk;
                }
                if t0.elapsed() > budget {
                    cut = Some("the 3 s budget");
                    break 'walk;
                }
                let Some(c) = read::<Cap>(at) else { break };
                total += 1;
                if let Some(name) = cap_name(&c) {
                    let lower = name.to_ascii_lowercase();
                    let st: u8 = field(&c, offset_of!(FD4FileCap, load_state));
                    let refs: u32 = field(&c, offset_of!(FD4FileCap, res_cap) + offset_of!(FD4ResCap, reference_count));
                    if OURS.iter().any(|id| lower.contains(id)) {
                        ours.push(format!("{name} ({}, {refs} refs)", state(st)));
                    } else if HEADS.iter().any(|h| lower.starts_with(h)) {
                        head_parts.push(format!("{} ({}, {refs} refs)", &name["parts:/".len()..], state(st)));
                    } else if lower.contains("parts") {
                        parts += 1;
                        if samples.len() < 6 {
                            samples.push(format!("{name} ({})", state(st)));
                        }
                    }
                }
                at = field(&c, offset_of!(FD4FileCap, res_cap) + offset_of!(FD4ResCap, next_item));
            }
        }
    }
    let mut line = format!("{when}: {total} files in the game's table ({} reads, {} ms{}); ours: ", reads() - r0, t0.elapsed().as_millis(), cut.map(|c| format!(", stopped at {c}")).unwrap_or_default());
    if ours.is_empty() {
        line.push_str("none (the game has not asked for them)");
    } else {
        line.push_str(&ours.join(", "));
    }
    head_parts.sort();
    let _ = write!(line, "; head, face and hair parts: {}", if head_parts.is_empty() { "none".to_string() } else { head_parts.join(", ") });
    let _ = write!(line, "; {parts} of the game's other part files, e.g. {}", samples.join(", "));
    crate::glog!("parts", "{line}");
}

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Once per session, 5 s into the world: on a thread of our own, read the player skeleton (from the behaviour module
/// at `behavior_module`) and then report the file table at 5 s, 30 s and 2 min. Nothing here runs on the game's thread.
pub fn start_once(behavior_module: usize) {
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("gowring-probe".into()).spawn(move || {
        let t0 = std::time::Instant::now();
        let r = std::panic::catch_unwind(|| {
            crate::skeleton::dump_once(behavior_module);
            for at in [5u64, 30, 120] {
                let wait = std::time::Duration::from_secs(at - 5).saturating_sub(t0.elapsed());
                std::thread::sleep(wait);
                report(&format!("{at} s into the world"));
            }
        });
        if r.is_err() {
            crate::glog!("parts", "the skeleton and file-table probe panicked");
        }
    });
    if let Err(e) = spawned {
        crate::glog!("parts", "could not start the probe thread: {e}");
    }
}

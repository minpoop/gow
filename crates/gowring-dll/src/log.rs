//! A rolling log file: `%LOCALAPPDATA%/GowRing/logs/gowring.log`. The log is the oracle for every
//! hook (first success / first failure of each is recorded), so players and developers can see what
//! the game actually did.

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

static LOG: Mutex<Option<(File, Instant)>> = Mutex::new(None);

pub fn init(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("gowring.log");
    let old = dir.join("gowring.1.log");
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::rename(&path, &old);
    if let Ok(f) = File::create(&path) {
        *LOG.lock().unwrap() = Some((f, Instant::now()));
    }
}

pub fn write(tag: &str, msg: &str) {
    if let Ok(mut g) = LOG.lock() {
        if let Some((f, t0)) = g.as_mut() {
            let _ = writeln!(f, "[{:9.3}] {tag:<12} {msg}", t0.elapsed().as_secs_f64());
            let _ = f.flush();
        }
    }
}

#[macro_export]
macro_rules! glog {
    ($tag:expr, $($arg:tt)*) => { $crate::log::write($tag, &format!($($arg)*)) };
}

/// Log a message the first time `key` is seen (so per-frame failures do not flood the file).
pub fn once(key: &'static str, tag: &str, msg: &str) {
    use std::collections::HashSet;
    static SEEN: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut g = SEEN.lock().unwrap();
    let set = g.get_or_insert_with(HashSet::new);
    if set.insert(key) {
        write(tag, msg);
    }
}

/// Like [`once`] but keyed by a number as well (first time each value is seen, up to 64 values).
pub fn once_u64(group: &'static str, value: u64, tag: &str, msg: &str) {
    use std::collections::HashSet;
    static SEEN: Mutex<Option<HashSet<(&'static str, u64)>>> = Mutex::new(None);
    let mut g = SEEN.lock().unwrap();
    let set = g.get_or_insert_with(HashSet::new);
    if set.len() < 64 && set.insert((group, value)) {
        write(tag, msg);
    }
}

//! The probe's log: `%LOCALAPPDATA%/GowRing/gowr_probe/latest/probe.log`. The run before is kept in `previous/`.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static LOG: Mutex<Option<(File, Instant)>> = Mutex::new(None);
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where this run writes (the log and the screenshots).
pub fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_else(|| PathBuf::from("."))
}

pub fn init() {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let root = base.join("GowRing").join("gowr_probe");
    let latest = root.join("latest");
    let previous = root.join("previous");
    if latest.exists() {
        let _ = std::fs::remove_dir_all(&previous);
        let _ = std::fs::rename(&latest, &previous);
    }
    let _ = std::fs::create_dir_all(&latest);
    if let Ok(f) = File::create(latest.join("probe.log")) {
        *LOG.lock().unwrap() = Some((f, Instant::now()));
    }
    let _ = DIR.set(latest);
}

pub fn write(tag: &str, msg: &str) {
    if let Ok(mut g) = LOG.lock() {
        if let Some((f, t0)) = g.as_mut() {
            let _ = writeln!(f, "[{:9.3}] {tag:<8} {msg}", t0.elapsed().as_secs_f64());
            let _ = f.flush();
        }
    }
}

#[macro_export]
macro_rules! plog {
    ($tag:expr, $($arg:tt)*) => { $crate::log::write($tag, &format!($($arg)*)) };
}

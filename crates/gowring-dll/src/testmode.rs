//! Melty's test harness hook. While `%LOCALAPPDATA%\Melty\test\<slug>.json` exists (JSON with a
//! `checks` path) the mod appends one line per named check to that file: `pass <name>` or
//! `fail <name>: <why>`. Names are listed in the recipe's `test.checks`.

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const SLUG: &str = "kratos-in-the-lands-between";

static CHECKS_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
static DONE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub static PARAMS_OK: AtomicBool = AtomicBool::new(false);
pub static OVERLAY_OK: AtomicBool = AtomicBool::new(false);
static OVERLAY_ERR: Mutex<Option<String>> = Mutex::new(None);

pub fn overlay_failed(why: &str) {
    *OVERLAY_ERR.lock().unwrap() = Some(why.to_string());
}

pub fn init() {
    let file = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).and_then(|d| {
        let marker = d.join("Melty").join("test").join(format!("{SLUG}.json"));
        let text = std::fs::read_to_string(&marker).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        crate::glog!("test", "Melty test run detected: {}", text.trim());
        v.get("checks").and_then(|c| c.as_str()).map(PathBuf::from)
    });
    let _ = CHECKS_FILE.set(file);
}

pub fn active() -> bool {
    matches!(CHECKS_FILE.get(), Some(Some(_)))
}

/// Append a check result once per name.
pub fn report(name: &str, ok: bool, why: &str) {
    let Some(Some(path)) = CHECKS_FILE.get() else { return };
    {
        let mut g = DONE.lock().unwrap();
        if !g.get_or_insert_with(HashSet::new).insert(name.to_string()) {
            return;
        }
    }
    let line = if ok { format!("pass {name}\n") } else { format!("fail {name}: {why}\n") };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| f.write_all(line.as_bytes())) {
        Ok(()) => crate::glog!("test", "reported: {}", line.trim()),
        Err(e) => crate::glog!("test", "could not write check `{name}` to {}: {e}", path.display()),
    }
}

/// Watches the four things that can be known without playing and reports them as they happen.
pub fn spawn_watcher() {
    if !active() {
        return;
    }
    report("dll-loaded", true, "");
    let _ = std::thread::Builder::new().name("gowring-test".into()).spawn(|| {
        let start = std::time::Instant::now();
        loop {
            if crate::savehook::redirected() {
                report("save-redirected", true, "");
            }
            if PARAMS_OK.load(Ordering::SeqCst) {
                report("params-patched", true, "");
            }
            if OVERLAY_OK.load(Ordering::SeqCst) {
                report("overlay-hooked", true, "");
            } else if let Some(e) = OVERLAY_ERR.lock().unwrap().clone() {
                report("overlay-hooked", false, &e);
            }
            if start.elapsed().as_secs() > 150 {
                report("save-redirected", false, "Elden Ring never opened its save within 150 s");
                report("params-patched", false, "the param tables were not patched within 150 s (see gowring.log)");
                report("overlay-hooked", false, "the HUD overlay never reported ready within 150 s");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

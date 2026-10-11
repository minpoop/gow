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
/// How far start-up got and how often the param patch was attempted: shown when a check fails.
pub static TASKS_REGISTERED: AtomicBool = AtomicBool::new(false);
pub static H0_CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static PARAM_TRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static STAGE: Mutex<String> = Mutex::new(String::new());
static PARAM_ERR: Mutex<String> = Mutex::new(String::new());

/// In-game observations, reported as checks once the player is in the world.
pub static PLAYER_SEEN: AtomicBool = AtomicBool::new(false);
pub static KIT_OK: AtomicBool = AtomicBool::new(false);
pub static TICKS_WITH_PLAYER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static KIT_STATE: Mutex<String> = Mutex::new(String::new());
static EXIT_CALL: Mutex<String> = Mutex::new(String::new());

/// Recorded by the exit hooks: which call, from where.
pub fn exit_called(text: &str) {
    let mut g = EXIT_CALL.lock().unwrap();
    if g.is_empty() {
        *g = text.chars().take(600).collect();
    }
}
static TICK_PANIC: Mutex<String> = Mutex::new(String::new());

pub fn kit_state(s: String) {
    *KIT_STATE.lock().unwrap() = s;
}

pub fn tick_panic(s: &str) {
    let mut g = TICK_PANIC.lock().unwrap();
    if g.is_empty() {
        *g = s.chars().take(300).collect();
    }
}

// ---- fatal exceptions ------------------------------------------------------------------------------
// A first-chance handler that only looks: it never handles anything. The last fatal-class exception is
// kept so that, if the game goes away, the checks can say whether it died of one.
use std::sync::atomic::{AtomicU32, AtomicUsize};
static FAULT_COUNT: AtomicU32 = AtomicU32::new(0);
static FAULT_CODE: AtomicU32 = AtomicU32::new(0);
static FAULT_ADDR: AtomicUsize = AtomicUsize::new(0);
static BORN: OnceLock<std::time::Instant> = OnceLock::new();

fn is_fatal(code: u32) -> bool {
    matches!(code, 0xC000_0005 | 0xC000_00FD | 0xC000_001D | 0xC000_0094 | 0xC000_0374 | 0xC000_0409 | 0xC000_0096)
}

/// `module.dll+0x1234` for an address in this process.
pub fn locate(addr: usize) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut h = HMODULE::default();
    let ok = unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, PCWSTR(addr as *const u16), &mut h) };
    if ok.is_err() || h.0.is_null() {
        return format!("{addr:#x} (no module)");
    }
    let mut buf = [0u16; 512];
    let n = unsafe { GetModuleFileNameW(h, &mut buf) } as usize;
    let name = String::from_utf16_lossy(&buf[..n]);
    let name = name.rsplit(['\\', '/']).next().unwrap_or("?").to_string();
    format!("{name}+{:#x}", addr - h.0 as usize)
}

unsafe extern "system" fn veh(info: *mut windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
    if info.is_null() || (*info).ExceptionRecord.is_null() {
        return 0;
    }
    let rec = &*(*info).ExceptionRecord;
    let code = rec.ExceptionCode.0 as u32;
    if is_fatal(code) {
        let n = FAULT_COUNT.fetch_add(1, Ordering::SeqCst);
        FAULT_CODE.store(code, Ordering::SeqCst);
        FAULT_ADDR.store(rec.ExceptionAddress as usize, Ordering::SeqCst);
        // not on a stack overflow (no room to format), and not in a flood
        if code != 0xC000_00FD && n < 12 {
            let at = locate(rec.ExceptionAddress as usize);
            crate::glog!("fault", "exception {code:#010x} at {at} (#{})", n + 1);
            // The first one after start-up settles is a failed check: Arxan's own start-up tricks are over by then.
            if BORN.get().map(|b| b.elapsed().as_secs() > 30).unwrap_or(false) {
                report("no-fault", false, &format!("fatal exception {code:#010x} at {at}; {}", state()));
            }
        }
    }
    0 // EXCEPTION_CONTINUE_SEARCH: we only watch
}

pub fn install_fault_watch() {
    let _ = BORN.set(std::time::Instant::now());
    unsafe {
        windows::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler(1, Some(veh));
    }
}

/// Called when the process is going away (DLL_PROCESS_DETACH with the process terminating).
pub fn process_exit() {
    let n = FAULT_COUNT.load(Ordering::SeqCst);
    crate::glog!("exit", "process is exiting after {:?}; fatal-class exceptions seen: {n}", BORN.get().map(|b| b.elapsed()));
    if !active() {
        return;
    }
    // A run that ends early leaves the other checks unreported; say why instead.
    let secs = BORN.get().map(|b| b.elapsed().as_secs()).unwrap_or(0);
    let why = format!("the game exited after {secs} s: {} | exit call: [{}]", state(), EXIT_CALL.lock().unwrap());
    if !crate::savehook::redirected() {
        report("save-redirected", false, &why);
    }
    if !PLAYER_SEEN.load(Ordering::SeqCst) {
        report("player-found", false, &why);
        report("kit-equipped", false, &why);
        report("rules-ran", false, &why);
    } else {
        if !KIT_OK.load(Ordering::SeqCst) {
            report("kit-equipped", false, &format!("{why} | {}", KIT_STATE.lock().unwrap()));
        }
        if TICKS_WITH_PLAYER.load(Ordering::SeqCst) < 300 {
            report("rules-ran", false, &why);
        }
    }
    if n == 0 {
        report("no-fault", true, "");
    } else {
        let at = locate(FAULT_ADDR.load(Ordering::SeqCst));
        report("no-fault", false, &format!("the game exited after {n} fatal exception(s); last {:#010x} at {at}", FAULT_CODE.load(Ordering::SeqCst)));
    }
}

pub fn stage(s: &str) {
    *STAGE.lock().unwrap() = s.to_string();
}

pub fn param_error(e: &str) {
    *PARAM_ERR.lock().unwrap() = e.chars().take(300).collect();
}

/// One line describing the mod's state, for failing checks (the harness shows it to the developer).
fn state() -> String {
    format!(
        "v{} stage={} tasks_registered={} frame_calls={} param_tries={} param_error=[{}] recent=[{}]",
        env!("CARGO_PKG_VERSION"),
        STAGE.lock().unwrap(),
        TASKS_REGISTERED.load(Ordering::SeqCst),
        H0_CALLS.load(Ordering::SeqCst),
        PARAM_TRIES.load(Ordering::SeqCst),
        PARAM_ERR.lock().unwrap(),
        crate::log::recent(8, 700),
    )
}
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

/// Watches what can be known without playing (first 150 s) and then what needs the player in the world.
pub fn spawn_watcher() {
    if !active() {
        return;
    }
    report("dll-loaded", true, "");
    let _ = std::thread::Builder::new().name("gowring-test".into()).spawn(|| {
        let start = std::time::Instant::now();
        loop {
            let t = start.elapsed().as_secs();
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
            if PLAYER_SEEN.load(Ordering::SeqCst) {
                report("player-found", true, "");
            }
            if KIT_OK.load(Ordering::SeqCst) {
                report("kit-equipped", true, "");
            }
            {
                let tp = TICK_PANIC.lock().unwrap().clone();
                if !tp.is_empty() {
                    report("rules-ran", false, &format!("the rules tick panicked: {tp}; {}", state()));
                } else if TICKS_WITH_PLAYER.load(Ordering::SeqCst) >= 300 {
                    report("rules-ran", true, "");
                }
            }
            if t > 150 {
                report("save-redirected", false, "Elden Ring never opened its save within 150 s");
                report("params-patched", false, &format!("the param tables were not patched within 150 s: {}", state()));
                report("overlay-hooked", false, "the HUD overlay never reported ready within 150 s");
            }
            if t > 200 && FAULT_COUNT.load(Ordering::SeqCst) == 0 {
                report("no-fault", true, "");
            }
            if t > 255 {
                if !PLAYER_SEEN.load(Ordering::SeqCst) {
                    report("player-found", false, &format!("no player character was in the world within 255 s (load a character; a new one gets the Kratos kit): {}", state()));
                    report("kit-equipped", false, "no player character was in the world");
                    report("rules-ran", false, "no player character was in the world");
                } else {
                    report("kit-equipped", false, &format!("the Kratos kit is not equipped (use a NEW character on the mod's own save): {}", KIT_STATE.lock().unwrap()));
                    report("rules-ran", false, &format!("fewer than 300 rule ticks with the player in the world: {}", state()));
                }
            }
            if t > 280 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

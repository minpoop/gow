//! hooks.boot_guard / boot_gate / save_redirect / overlay / rules_tick wiring.

use crate::assets;
use crate::overlay::{Overlay, Shared, Snapshot};
use crate::runtime::{apply_camera, Runtime};
use crate::{paths, savehook};
use eldenring::cs::{CSTaskGroupIndex, CSTaskImp};
use eldenring::fd4::FD4TaskData;
use fromsoftware_shared::SharedTaskImpExt;
use hudhook::hooks::dx12::ImguiDx12Hooks;
use hudhook::windows::Win32::Foundation::HINSTANCE;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Modules that mean Easy Anti-Cheat is live in this process. We never run next to it.
const ANTI_CHEAT_MODULES: &[&str] = &["EasyAntiCheat_EOS.dll", "EasyAntiCheat_x64.dll", "EasyAntiCheat.dll", "EOSSDK-Win64-Shipping.dll"];

fn module_dir(hmodule: usize) -> Option<std::path::PathBuf> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(HMODULE(hmodule as _), &mut buf) } as usize;
    if n == 0 {
        return None;
    }
    std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n])).parent().map(|p| p.to_path_buf())
}

fn anti_cheat_loaded() -> Option<&'static str> {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    ANTI_CHEAT_MODULES.iter().copied().find(|name| {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { GetModuleHandleW(PCWSTR(wide.as_ptr())) }.is_ok()
    })
}

pub fn run(hmodule: usize) {
    let p = paths::init(module_dir(hmodule));
    crate::log::init(&p.local.join("logs"));
    crate::settings::init(&p.local);
    // Every panic (ours or a dependency's) lands in the log with its location.
    std::panic::set_hook(Box::new(|info| {
        // a per-frame panic must not flood the log
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n < 60 {
            crate::glog!("panic", "{info}");
        } else if n == 60 {
            crate::glog!("panic", "(further panics are not logged)");
        }
    }));
    crate::glog!("boot", "GowRing {} ({}) loading; home={} cache={}", env!("CARGO_PKG_VERSION"), p.channel, p.home.display(), p.cache.display());

    crate::testmode::stage("boot");
    crate::testmode::install_fault_watch();
    crate::testmode::init();
    crate::testmode::spawn_watcher();
    crate::testmode::spawn_heap_check();

    // ---- boot_guard ---------------------------------------------------------------------------------
    if !p.from_launcher {
        crate::glog!("boot_guard", "ABORT: not started by GowRing.exe (GOWRING_HOME missing). Use Play in Melty.");
        return;
    }
    if let Some(m) = anti_cheat_loaded() {
        crate::glog!("boot_guard", "ABORT: anti-cheat module {m} is loaded. This mod only runs offline through ModEngine2.");
        return;
    }

    crate::testmode::stage("guard-passed");
    // ---- save_redirect (must be in place before Elden Ring touches its save) ---------------------------------
    if let Err(e) = savehook::install(&p.save_file) {
        crate::glog!("save", "ABORT: could not redirect the save file ({e}). Your own save will not be used by this mod.");
        return;
    }
    crate::glog!("save", "CreateFileW redirect installed -> {}", p.save_file);
    crate::testmode::stage("save-hooked");

    match crate::exithook::install() {
        Ok(()) => crate::glog!("exit", "exit hooks installed (record-only)"),
        Err(e) => crate::glog!("exit", "exit hooks not installed: {e}"),
    }
    // ---- assets, overlay ---------------------------------------------------------------------------------------
    let store = Arc::new(assets::load(&p.cache, &p.channel));
    crate::glog!("gow_cache", "{}/{} Ragnarök assets present (prepared: {}); missing: {}", store.converted, store.total, store.prepared, store.missing.join(", "));
    let shared: Shared = Arc::new(Mutex::new(Snapshot::default()));
    // hudhook panics (rather than errors) when it cannot create its probe window or device; a missing
    // overlay must never take the rules layer down with it.
    let hooks = catch_unwind(AssertUnwindSafe(|| Hudhook::build(hmodule, shared.clone(), store.clone())));
    match hooks {
        Ok(Ok(())) => {
            crate::glog!("overlay", "D3D12 hook applied");
            crate::testmode::OVERLAY_OK.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(Err(e)) => {
            crate::glog!("overlay", "FAILED to hook D3D12: {e} (no HUD this session)");
            crate::testmode::overlay_failed(&format!("hudhook could not hook D3D12: {e}"));
        }
        Err(_) => {
            crate::glog!("overlay", "FAILED to hook D3D12 (hudhook panicked; no HUD this session)");
            crate::testmode::overlay_failed("hudhook panicked while hooking D3D12");
        }
    }

    crate::testmode::stage("overlay-done");
    // ---- boot_gate: wait for the task system, then register the per-frame tasks ---------------------------------------
    crate::testmode::stage("waiting-for-task-system");
    crate::glog!("boot_gate", "waiting for the game's task system (eldenring.exe build must be one the eldenring crate knows)");
    let cs_task = match catch_unwind(|| CSTaskImp::wait_for_instance(Duration::from_secs(600))) {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            crate::glog!("boot_gate", "ABORT: game task system never came up: {e}");
            crate::testmode::stage(&format!("task-system-error: {e}"));
            return;
        }
        Err(_) => {
            crate::glog!("boot_gate", "ABORT: the task system lookup panicked (see the panic line above)");
            crate::testmode::stage("task-system-panicked");
            return;
        }
    };
    crate::testmode::stage("task-system-ready");
    crate::glog!("boot_gate", "task system ready");
    let camera_req = Arc::new(Mutex::new(gowring_core::CameraRequest::default()));
    let rt = Arc::new(Mutex::new(Runtime::new(shared, store, p.cache.clone(), camera_req.clone())));

    // character creation happens at the title screen, so the param patch gets its own always-running task
    let rt0 = rt.clone();
    let h0 = cs_task.run_recurring(
        move |_: &FD4TaskData| {
            if crate::testmode::H0_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                crate::glog!("boot", "first frame task ran");
            }
            if let Ok(mut r) = rt0.try_lock() {
                if !r.carriers_ready() {
                    let _ = catch_unwind(AssertUnwindSafe(|| r.ensure_params()));
                }
            }
        },
        CSTaskGroupIndex::FrameBegin,
    );

    let rt2 = rt.clone();
    let h1 = cs_task.run_recurring(
        move |_: &FD4TaskData| {
            if let Ok(mut r) = rt2.try_lock() {
                let t0 = std::time::Instant::now();
                let res = catch_unwind(AssertUnwindSafe(|| r.tick()));
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    let us = t0.elapsed().as_micros() as u64;
                    crate::testmode::TICK_US_SUM.fetch_add(us, Relaxed);
                    crate::testmode::TICK_US_MAX.fetch_max(us, Relaxed);
                    crate::testmode::TICK_N.fetch_add(1, Relaxed);
                }
                if let Err(e) = res {
                    let why = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
                    crate::log::once("tick-panic", "rules_tick", &format!("PANIC in tick: {why}"));
                    crate::testmode::tick_panic(&why);
                }
            }
        },
        CSTaskGroupIndex::ChrIns_PreBehaviorSafe,
    );
    let h2 = cs_task.run_recurring(
        move |_: &FD4TaskData| {
            let req = camera_req.lock().map(|g| *g).unwrap_or_default();
            if let Err(e) = catch_unwind(AssertUnwindSafe(|| apply_camera(&req))) {
                crate::log::once("camera-panic", "camera_offset", &format!("PANIC: {:?}", e.downcast_ref::<&str>()));
            }
            if catch_unwind(|| crate::bonepose::on_stage(crate::bonepose::Stage::DrawPre)).is_err() {
                crate::log::once("bonepose-panic", "bonepose", "PANIC in the pose probe before drawing");
            }
            if catch_unwind(crate::cape::step).is_err() {
                crate::log::once("cape-panic", "cape", "PANIC in the cape before drawing");
            }
        },
        CSTaskGroupIndex::Draw_Pre,
    );
    // the pose probe's write after the characters' physics and cloth (see bonepose)
    let h3 = cs_task.run_recurring(
        move |_: &FD4TaskData| {
            if catch_unwind(|| crate::bonepose::on_stage(crate::bonepose::Stage::PostPhysics)).is_err() {
                crate::log::once("bonepose-panic-post", "bonepose", "PANIC in the pose probe after physics");
            }
        },
        CSTaskGroupIndex::ChrIns_PostPhysicsSafe,
    );
    // The handles cancel their tasks on drop; the mod lives as long as the process.
    std::mem::forget(h0);
    std::mem::forget(h1);
    std::mem::forget(h2);
    std::mem::forget(h3);
    crate::testmode::TASKS_REGISTERED.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::testmode::stage("tasks-registered");
    spawn_heartbeat();
    crate::glog!("boot", "per-frame tasks registered (params @ FrameBegin, rules @ ChrIns_PreBehaviorSafe, pose probe @ ChrIns_PostPhysicsSafe, camera and cape @ Draw_Pre)");
}

struct Hudhook;

impl Hudhook {
    fn build(hmodule: usize, shared: Shared, store: Arc<assets::Store>) -> Result<(), String> {
        let overlay = Overlay::new(shared, store);
        let hooks = hudhook::Hudhook::builder().with::<ImguiDx12Hooks>(overlay).with_hmodule(HINSTANCE(hmodule as _)).build();
        hooks.apply().map_err(|e| format!("{e:?}"))
    }
}

/// One log line every few seconds saying whether the game's main loop is running and how far the mod got,
/// so a stall or an early exit can be read off the log afterwards.
fn spawn_heartbeat() {
    use std::sync::atomic::Ordering::SeqCst;
    let _ = std::thread::Builder::new().name("gowring-heartbeat".into()).spawn(|| {
        let start = std::time::Instant::now();
        let mut last_frames = 0u32;
        loop {
            let t = start.elapsed().as_secs();
            let frames = crate::testmode::H0_CALLS.load(SeqCst);
            let (n, sum, max) = {
                use std::sync::atomic::Ordering::Relaxed;
                (crate::testmode::TICK_N.swap(0, Relaxed), crate::testmode::TICK_US_SUM.swap(0, Relaxed), crate::testmode::TICK_US_MAX.swap(0, Relaxed))
            };
            let avg_us = if n > 0 { sum / n as u64 } else { 0 };
            crate::glog!(
                "alive",
                "t={t}s frames_since_last={} rules_tick_avg={avg_us}us max={max}us params={} save_open={} player={} ticks={} kit={}",
                frames.wrapping_sub(last_frames),
                crate::testmode::PARAMS_OK.load(SeqCst),
                crate::savehook::redirected(),
                crate::testmode::PLAYER_SEEN.load(SeqCst),
                crate::testmode::TICKS_WITH_PLAYER.load(SeqCst),
                crate::testmode::KIT_OK.load(SeqCst),
            );
            last_frames = frames;
            // every 5 s for the first two minutes, then every 30 s
            std::thread::sleep(std::time::Duration::from_secs(if t < 120 { 5 } else { 30 }));
        }
    });
}

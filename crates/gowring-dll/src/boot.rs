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
    // Every panic (ours or a dependency's) lands in the log with its location.
    std::panic::set_hook(Box::new(|info| crate::glog!("panic", "{info}")));
    crate::glog!("boot", "GowRing {} ({}) loading; home={} cache={}", env!("CARGO_PKG_VERSION"), p.channel, p.home.display(), p.cache.display());

    // ---- boot_guard ---------------------------------------------------------------------------------
    if !p.from_launcher {
        crate::glog!("boot_guard", "ABORT: not started by GowRing.exe (GOWRING_HOME missing). Use Play in Melty.");
        return;
    }
    if let Some(m) = anti_cheat_loaded() {
        crate::glog!("boot_guard", "ABORT: anti-cheat module {m} is loaded. This mod only runs offline through ModEngine2.");
        return;
    }

    // ---- save_redirect (must be in place before Elden Ring touches its save) ---------------------------------
    if let Err(e) = savehook::install(&p.save_file) {
        crate::glog!("save", "ABORT: could not redirect the save file ({e}). Your own save will not be used by this mod.");
        return;
    }
    crate::glog!("save", "CreateFileW redirect installed -> {}", p.save_file);

    // ---- assets, overlay ---------------------------------------------------------------------------------------
    let store = Arc::new(assets::load(&p.cache, &p.channel));
    crate::glog!("gow_cache", "{}/{} Ragnarök assets present (prepared: {}); missing: {}", store.converted, store.total, store.prepared, store.missing.join(", "));
    let shared: Shared = Arc::new(Mutex::new(Snapshot::default()));
    // hudhook panics (rather than errors) when it cannot create its probe window or device; a missing
    // overlay must never take the rules layer down with it.
    let hooks = catch_unwind(AssertUnwindSafe(|| Hudhook::build(hmodule, shared.clone(), store.clone())));
    match hooks {
        Ok(Ok(())) => crate::glog!("overlay", "D3D12 hook applied"),
        Ok(Err(e)) => crate::glog!("overlay", "FAILED to hook D3D12: {e} (no HUD this session)"),
        Err(_) => crate::glog!("overlay", "FAILED to hook D3D12 (hudhook panicked; no HUD this session)"),
    }

    // ---- boot_gate: wait for the task system, then register the per-frame tasks ---------------------------------------
    let cs_task = match CSTaskImp::wait_for_instance(Duration::from_secs(600)) {
        Ok(t) => t,
        Err(e) => {
            crate::glog!("boot_gate", "ABORT: game task system never came up: {e}");
            return;
        }
    };
    crate::glog!("boot_gate", "task system ready");
    let camera_req = Arc::new(Mutex::new(gowring_core::CameraRequest::default()));
    let rt = Arc::new(Mutex::new(Runtime::new(shared, store, p.cache.clone(), camera_req.clone())));

    // character creation happens at the title screen, so the param patch gets its own always-running task
    let rt0 = rt.clone();
    let h0 = cs_task.run_recurring(
        move |_: &FD4TaskData| {
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
                if let Err(e) = catch_unwind(AssertUnwindSafe(|| r.tick())) {
                    crate::log::once("tick-panic", "rules_tick", &format!("PANIC in tick: {:?}", e.downcast_ref::<&str>()));
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
        },
        CSTaskGroupIndex::Draw_Pre,
    );
    // The handles cancel their tasks on drop; the mod lives as long as the process.
    std::mem::forget(h0);
    std::mem::forget(h1);
    std::mem::forget(h2);
    crate::glog!("boot", "per-frame tasks registered (params @ FrameBegin, rules @ ChrIns_PreBehaviorSafe, camera @ Draw_Pre)");
}

struct Hudhook;

impl Hudhook {
    fn build(hmodule: usize, shared: Shared, store: Arc<assets::Store>) -> Result<(), String> {
        let overlay = Overlay::new(shared, store);
        let hooks = hudhook::Hudhook::builder().with::<ImguiDx12Hooks>(overlay).with_hmodule(HINSTANCE(hmodule as _)).build();
        hooks.apply().map_err(|e| format!("{e:?}"))
    }
}

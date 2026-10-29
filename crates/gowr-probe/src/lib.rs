//! gowr_probe.dll — stages 1 and 2a of live Kratos (docs/LIVE-KRATOS-PLAN.md): get inside God of War Ragnarök and see
//! what it draws.
//!
//! Loaded into GoWR.exe by GowRProbe.exe (CreateRemoteThread + LoadLibraryW). It hooks Ragnarök's D3D12 Present and
//! command queue and only *watches*: the swapchain and adapter, frames per second in the foreground and background,
//! its video memory, which input libraries it loaded, and a few screenshots of its back buffer. One thing it changes:
//! while Ragnarök's window is not in front, XInput controller reads come back idle, so Kratos there stays still, and
//! it is held to 30 frames a second (it ran at 100+ behind Elden Ring and slowed Elden Ring's loading). 0.2.0 adds the
//! camera search on Ctrl+F9 (`camsearch`, live-Kratos stage 2a).
//! Everything goes to `%LOCALAPPDATA%/GowRing/gowr_probe/latest/`.

mod camsearch;
mod d3d;
mod input;
mod log;
mod monitor;
mod shot;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;

const DLL_PROCESS_ATTACH: u32 = 1;

#[no_mangle]
pub extern "system" fn DllMain(hmodule: HMODULE, reason: u32, _reserved: *mut core::ffi::c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            let _ = DisableThreadLibraryCalls(hmodule);
        }
        let _ = std::thread::Builder::new().name("gowr-probe".into()).spawn(|| {
            if std::panic::catch_unwind(boot).is_err() {
                log::write("boot", "PANIC during start; the probe is idle for this session");
            }
        });
    }
    1
}

fn boot() {
    log::init();
    plog!("boot", "gowr_probe {} loaded into {}", env!("CARGO_PKG_VERSION"), monitor::exe_identity());
    monitor::log_modules("at start");
    match d3d::install() {
        Ok(()) => plog!("d3d", "hooked: Present, Present1, ExecuteCommandLists"),
        Err(e) => plog!("d3d", "FAILED to hook D3D12: {e}"),
    }
    input::install();
    monitor::run();
}

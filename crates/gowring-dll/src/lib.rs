//! gowring.dll — the in-game half of "Kratos in the Lands Between".
//!
//! Loaded by ModEngine2 (`external_dlls`) into eldenring.exe, started by `GowRing.exe --play`.
//! The rules live in `gowring-core`; this crate is the Elden Ring bridge, overlay and audio.

mod assets;
mod audio;
mod boot;
mod host;
mod log;
mod overlay;
mod params;
mod paths;
mod runtime;
mod savehook;
mod testmode;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;

const DLL_PROCESS_ATTACH: u32 = 1;

#[no_mangle]
pub extern "system" fn DllMain(hmodule: HMODULE, reason: u32, _reserved: *mut core::ffi::c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            let _ = DisableThreadLibraryCalls(hmodule);
        }
        let h = hmodule.0 as usize;
        let _ = std::thread::Builder::new().name("gowring-boot".into()).spawn(move || {
            let r = std::panic::catch_unwind(|| boot::run(h));
            if r.is_err() {
                log::write("boot", "PANIC during boot; the mod is disabled for this session");
            }
        });
    }
    1
}

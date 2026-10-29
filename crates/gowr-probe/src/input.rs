//! While Ragnarök's window is not in front, its XInput controller reads come back idle (connected, nothing pressed),
//! so its Kratos stays still while you play Elden Ring. In front, nothing is changed. The log counts the reads, which
//! also says whether Ragnarök uses XInput at all (a DualSense may be read another way: see the module list).

use crate::plog;
use retour::GenericDetour;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Input::XboxController::{XINPUT_GAMEPAD, XINPUT_STATE};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

type GetStateFn = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

pub static READS: AtomicU64 = AtomicU64::new(0);
pub static MUTED: AtomicU64 = AtomicU64::new(0);

const MODULES: [&str; 3] = ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"];
static DETOURS: [OnceLock<GenericDetour<GetStateFn>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];

fn in_front() -> bool {
    let main = crate::d3d::HWND_MAIN.load(Relaxed);
    // before the first frame the window is unknown: change nothing
    main == 0 || unsafe { GetForegroundWindow() } == HWND(main as *mut _)
}

unsafe fn filtered(i: usize, user: u32, state: *mut XINPUT_STATE) -> u32 {
    let r = DETOURS[i].get().unwrap().call(user, state);
    READS.fetch_add(1, Relaxed);
    if r == 0 && !state.is_null() && !in_front() {
        (*state).Gamepad = XINPUT_GAMEPAD::default();
        MUTED.fetch_add(1, Relaxed);
    }
    r
}

unsafe extern "system" fn h0(u: u32, s: *mut XINPUT_STATE) -> u32 {
    filtered(0, u, s)
}
unsafe extern "system" fn h1(u: u32, s: *mut XINPUT_STATE) -> u32 {
    filtered(1, u, s)
}
unsafe extern "system" fn h2(u: u32, s: *mut XINPUT_STATE) -> u32 {
    filtered(2, u, s)
}

/// Hooks XInputGetState in whichever XInput libraries are loaded now; called again later for ones loaded after.
pub fn install() {
    let hooks: [GetStateFn; 3] = [h0, h1, h2];
    for (i, name) in MODULES.iter().enumerate() {
        if DETOURS[i].get().is_some() {
            continue;
        }
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let Ok(m) = GetModuleHandleW(PCWSTR(wide.as_ptr())) else { continue };
            let Some(a) = GetProcAddress(m, windows::core::s!("XInputGetState")) else { continue };
            match GenericDetour::new(std::mem::transmute::<_, GetStateFn>(a), hooks[i]) {
                Ok(d) => match d.enable() {
                    Ok(()) => {
                        let _ = DETOURS[i].set(d);
                        plog!("input", "{name} XInputGetState hooked: idle while Ragnarök is not in front");
                    }
                    Err(e) => plog!("input", "{name} XInputGetState enable failed: {e}"),
                },
                Err(e) => plog!("input", "{name} XInputGetState hook failed: {e}"),
            }
        }
    }
}

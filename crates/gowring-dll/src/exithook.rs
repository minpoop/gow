//! Who ends the process? `ExitProcess` and `TerminateProcess(self)` are hooked only to *record* the call
//! chain (module+offset of each caller) before passing the call on unchanged. When a test run says the game
//! "closed by itself", this says which code asked it to.

use retour::GenericDetour;
use std::sync::OnceLock;
use windows::Win32::Foundation::{BOOL, HANDLE};
use windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetProcessId};

type ExitFn = unsafe extern "system" fn(u32);
type TermFn = unsafe extern "system" fn(HANDLE, u32) -> BOOL;

static D_EXIT: OnceLock<GenericDetour<ExitFn>> = OnceLock::new();
static D_TERM: OnceLock<GenericDetour<TermFn>> = OnceLock::new();

fn note(api: &str, code: u32) {
    let mut frames = [std::ptr::null_mut::<core::ffi::c_void>(); 12];
    let n = unsafe { RtlCaptureStackBackTrace(1, &mut frames, None) } as usize;
    let chain: Vec<String> = frames[..n].iter().map(|f| crate::testmode::locate(*f as usize)).collect();
    let text = format!("{api}({code}) called from {}", chain.join(" <- "));
    crate::glog!("exit", "{text}");
    crate::testmode::exit_called(&text);
}

unsafe extern "system" fn h_exit(code: u32) {
    note("ExitProcess", code);
    D_EXIT.get().unwrap().call(code)
}

unsafe extern "system" fn h_term(h: HANDLE, code: u32) -> BOOL {
    if h.0 as isize == -1 || GetProcessId(h) == GetCurrentProcessId() {
        note("TerminateProcess(self)", code);
    }
    D_TERM.get().unwrap().call(h, code)
}

pub fn install() -> Result<(), String> {
    unsafe {
        let k32 = GetModuleHandleW(windows::core::w!("kernel32.dll")).map_err(|e| format!("kernel32: {e}"))?;
        let a = GetProcAddress(k32, windows::core::s!("ExitProcess")).ok_or("ExitProcess not found")?;
        let d = GenericDetour::new(std::mem::transmute::<_, ExitFn>(a), h_exit).map_err(|e| format!("ExitProcess: {e}"))?;
        d.enable().map_err(|e| format!("ExitProcess enable: {e}"))?;
        let _ = D_EXIT.set(d);
        let a = GetProcAddress(k32, windows::core::s!("TerminateProcess")).ok_or("TerminateProcess not found")?;
        let d = GenericDetour::new(std::mem::transmute::<_, TermFn>(a), h_term).map_err(|e| format!("TerminateProcess: {e}"))?;
        d.enable().map_err(|e| format!("TerminateProcess enable: {e}"))?;
        let _ = D_TERM.set(d);
    }
    Ok(())
}

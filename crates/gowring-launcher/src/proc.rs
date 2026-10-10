//! Is a process running? (Windows Toolhelp; a no-op elsewhere.)

use std::time::{Duration, Instant};

#[cfg(windows)]
pub fn running(name: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let want = name.to_ascii_lowercase();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..len]).to_ascii_lowercase() == want {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
pub fn running(_name: &str) -> bool {
    false
}

pub fn wait_for(name: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if running(name) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

pub fn wait_gone(name: &str) {
    while running(name) {
        std::thread::sleep(Duration::from_secs(2));
    }
}

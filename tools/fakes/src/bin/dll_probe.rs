//! Loads gowring.dll into a plain process (no Elden Ring) to check what can be checked without the
//! game: that it loads, boots, installs the CreateFileW redirect, and logs. Run under Wine.
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, WriteFile, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL};
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn main() {
    let dll = std::env::args().nth(1).expect("usage: dll_probe <gowring.dll> <save dir>");
    let dir = std::path::PathBuf::from(std::env::args().nth(2).expect("save dir"));
    std::fs::create_dir_all(&dir).unwrap();
    let h = unsafe { LoadLibraryW(wide(std::ffi::OsStr::new(&dll)).as_ptr()) };
    println!("LoadLibrary -> {}", if h.is_null() { "FAILED" } else { "ok" });
    if h.is_null() {
        std::process::exit(1);
    }
    std::thread::sleep(std::time::Duration::from_secs(5));
    // pretend to be Elden Ring opening its save
    let target = dir.join("ER0000.sl2");
    let f = unsafe { CreateFileW(wide(target.as_os_str()).as_ptr(), GENERIC_WRITE, 0, std::ptr::null(), CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut()) };
    if f != INVALID_HANDLE_VALUE {
        let mut n = 0u32;
        unsafe {
            WriteFile(f, b"probe".as_ptr(), 5, &mut n, std::ptr::null_mut());
            CloseHandle(f);
        }
    }
    println!("ER0000.sl2 exists: {}", target.exists());
    println!("ER0000.gowring.sl2 exists: {}", dir.join("ER0000.gowring.sl2").exists());
    std::thread::sleep(std::time::Duration::from_secs(2));
}

//! Loads gowring.dll into a plain process (no Elden Ring) to check what can be checked without the
//! game: that it loads, boots, installs the CreateFileW redirect, and logs. Run under Wine.
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CopyFileW, CreateFileW, DeleteFileW, GetFileAttributesW, MoveFileExW, WriteFile, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, MOVEFILE_REPLACE_EXISTING};
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
    // the game's other ways of reaching a save: rename a temp file onto it, copy it to .bak, ask for attributes, delete
    let tmp = dir.join("ER0000.tmp");
    std::fs::write(&tmp, b"temp-save").unwrap();
    let ok_move = unsafe { MoveFileExW(wide(tmp.as_os_str()).as_ptr(), wide(target.as_os_str()).as_ptr(), MOVEFILE_REPLACE_EXISTING) };
    let attrs = unsafe { GetFileAttributesW(wide(target.as_os_str()).as_ptr()) };
    let bak = dir.join("ER0000.sl2.bak");
    let ok_copy = unsafe { CopyFileW(wide(target.as_os_str()).as_ptr(), wide(bak.as_os_str()).as_ptr(), 0) };
    println!("move ok={} attrs_valid={} copy ok={}", ok_move != 0, attrs != u32::MAX, ok_copy != 0);
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    println!("files on disk: {names:?}");
    let ok_del = unsafe { DeleteFileW(wide(bak.as_os_str()).as_ptr()) };
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    println!("delete ok={} -> files on disk: {names:?}", ok_del != 0);
    std::thread::sleep(std::time::Duration::from_secs(2));
}

//! hooks.save_redirect — Elden Ring plays on `ER0000.gowring.sl2`, never on the player's own
//! `ER0000.sl2`. A detour on `CreateFileW` rewrites the file name. Until the first redirected open has
//! been seen, the rest of the mod stays inert, so a failed redirect can never touch the real save.

use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{FILE_CREATION_DISPOSITION, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

type CreateFileWFn = unsafe extern "system" fn(PCWSTR, u32, FILE_SHARE_MODE, *const SECURITY_ATTRIBUTES, FILE_CREATION_DISPOSITION, FILE_FLAGS_AND_ATTRIBUTES, HANDLE) -> HANDLE;

static DETOUR: OnceLock<GenericDetour<CreateFileWFn>> = OnceLock::new();
static REDIRECTED: AtomicBool = AtomicBool::new(false);
static REDIRECT_COUNT: AtomicU32 = AtomicU32::new(0);
static TARGET: OnceLock<String> = OnceLock::new();

/// True once Elden Ring has opened (a redirected) save file.
pub fn redirected() -> bool {
    REDIRECTED.load(Ordering::SeqCst)
}

fn wide_to_string(p: PCWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string().unwrap_or_default() }
}

/// `…\ER0000.sl2` → `…\<target>`; `…\ER0000.sl2.bak` → `…\<target>.bak`.
pub fn rewrite(path: &str, target: &str) -> Option<String> {
    let lower = path.to_ascii_lowercase();
    let (suffix, extra) = if lower.ends_with("er0000.sl2") {
        ("er0000.sl2", "")
    } else if lower.ends_with("er0000.sl2.bak") {
        ("er0000.sl2.bak", ".bak")
    } else {
        return None;
    };
    let cut = path.len() - suffix.len();
    Some(format!("{}{}{}", &path[..cut], target, extra))
}

unsafe extern "system" fn hooked(name: PCWSTR, access: u32, share: FILE_SHARE_MODE, sa: *const SECURITY_ATTRIBUTES, disp: FILE_CREATION_DISPOSITION, flags: FILE_FLAGS_AND_ATTRIBUTES, template: HANDLE) -> HANDLE {
    let detour = DETOUR.get().expect("detour");
    let original = wide_to_string(name);
    if let Some(target) = TARGET.get() {
        if let Some(new_path) = rewrite(&original, target) {
            let wide: Vec<u16> = new_path.encode_utf16().chain(std::iter::once(0)).collect();
            let n = REDIRECT_COUNT.fetch_add(1, Ordering::SeqCst);
            if n < 4 {
                crate::glog!("save", "redirect #{n}: {original} -> {new_path}");
            }
            REDIRECTED.store(true, Ordering::SeqCst);
            return detour.call(PCWSTR(wide.as_ptr()), access, share, sa, disp, flags, template);
        }
    }
    detour.call(name, access, share, sa, disp, flags, template)
}

pub fn install(target: &str) -> Result<(), String> {
    let _ = TARGET.set(target.to_string());
    unsafe {
        let k32 = GetModuleHandleW(windows::core::w!("kernel32.dll")).map_err(|e| format!("kernel32: {e}"))?;
        let addr = GetProcAddress(k32, windows::core::s!("CreateFileW")).ok_or("CreateFileW not found")?;
        let original: CreateFileWFn = std::mem::transmute(addr);
        let d = GenericDetour::new(original, hooked).map_err(|e| format!("detour: {e}"))?;
        d.enable().map_err(|e| format!("enable: {e}"))?;
        let _ = DETOUR.set(d);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rewrite;

    #[test]
    fn rewrites_only_the_er_save_names() {
        let t = "ER0000.gowring.sl2";
        assert_eq!(rewrite(r"C:\Users\x\AppData\Roaming\EldenRing\7656\ER0000.sl2", t).unwrap(), r"C:\Users\x\AppData\Roaming\EldenRing\7656\ER0000.gowring.sl2");
        assert_eq!(rewrite(r"C:\a\ER0000.sl2.bak", t).unwrap(), r"C:\a\ER0000.gowring.sl2.bak");
        assert_eq!(rewrite(r"c:\a\er0000.SL2", t).unwrap(), r"c:\a\ER0000.gowring.sl2");
        assert!(rewrite(r"C:\a\GraphicsConfig.xml", t).is_none());
        assert!(rewrite(r"C:\a\ER0000.gowring.sl2", t).is_none());
    }
}

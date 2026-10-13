//! hooks.save_redirect — Elden Ring plays on `ER0000.gowring.sl2`, never on the player's own
//! `ER0000.sl2`. Detours on the kernel32 file calls that can reach a save (open, attributes, move,
//! replace, copy, delete) rewrite any path ending in `ER0000.sl2[.bak]`. Until the first redirected
//! open has been seen, the rest of the mod stays inert, so a failed redirect can never touch the
//! real save.

use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, SetLastError, BOOL, HANDLE};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{FILE_CREATION_DISPOSITION, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

type CreateFileWFn = unsafe extern "system" fn(PCWSTR, u32, FILE_SHARE_MODE, *const SECURITY_ATTRIBUTES, FILE_CREATION_DISPOSITION, FILE_FLAGS_AND_ATTRIBUTES, HANDLE) -> HANDLE;
type GetAttrFn = unsafe extern "system" fn(PCWSTR) -> u32;
type GetAttrExFn = unsafe extern "system" fn(PCWSTR, i32, *mut core::ffi::c_void) -> BOOL;
type MoveExFn = unsafe extern "system" fn(PCWSTR, PCWSTR, u32) -> BOOL;
type MoveFn = unsafe extern "system" fn(PCWSTR, PCWSTR) -> BOOL;
type ReplaceFn = unsafe extern "system" fn(PCWSTR, PCWSTR, PCWSTR, u32, *mut core::ffi::c_void, *mut core::ffi::c_void) -> BOOL;
type CopyFn = unsafe extern "system" fn(PCWSTR, PCWSTR, BOOL) -> BOOL;
type DeleteFn = unsafe extern "system" fn(PCWSTR) -> BOOL;

static D_CREATE: OnceLock<GenericDetour<CreateFileWFn>> = OnceLock::new();
static D_ATTR: OnceLock<GenericDetour<GetAttrFn>> = OnceLock::new();
static D_ATTR_EX: OnceLock<GenericDetour<GetAttrExFn>> = OnceLock::new();
static D_MOVE_EX: OnceLock<GenericDetour<MoveExFn>> = OnceLock::new();
static D_MOVE: OnceLock<GenericDetour<MoveFn>> = OnceLock::new();
static D_REPLACE: OnceLock<GenericDetour<ReplaceFn>> = OnceLock::new();
static D_COPY: OnceLock<GenericDetour<CopyFn>> = OnceLock::new();
static D_DELETE: OnceLock<GenericDetour<DeleteFn>> = OnceLock::new();

static REDIRECTED: AtomicBool = AtomicBool::new(false);
static OPEN_COUNT: AtomicU32 = AtomicU32::new(0);
static SIDE_COUNT: AtomicU32 = AtomicU32::new(0);
static TARGET: OnceLock<String> = OnceLock::new();
static PART_NOTES: AtomicU32 = AtomicU32::new(0);

/// Kratos's body part: log the first opens and existence checks of armour part files (`*.partsbnd*`) and what they
/// returned, so a session shows whether the game (or ModEngine2) reached the files the launcher installed. The
/// thread's last-error value is put back afterwards, so the game sees exactly what it would have.
fn note_part(call: &str, p: PCWSTR, ok: bool) {
    let err = unsafe { GetLastError() };
    let path = wide_to_string(p);
    if path.to_ascii_lowercase().contains("partsbnd") && PART_NOTES.fetch_add(1, Ordering::SeqCst) < 60 {
        let result = if ok { "ok".to_string() } else { format!("failed, error {}", err.0) };
        crate::glog!("parts", "{call} {path}: {result}");
    }
    unsafe { SetLastError(err) };
}

/// True once Elden Ring has opened a (redirected) save file.
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

/// A NUL-terminated redirected copy of `p` when it names the real save.
fn redirect(p: PCWSTR, note: &str) -> Option<Vec<u16>> {
    let target = TARGET.get()?;
    let original = wide_to_string(p);
    let new_path = rewrite(&original, target)?;
    if SIDE_COUNT.fetch_add(1, Ordering::SeqCst) < 12 {
        crate::glog!("save", "{note}: {original} -> {new_path}");
    }
    Some(new_path.encode_utf16().chain(std::iter::once(0)).collect())
}

/// Pick the redirected pointer if there is one, else the caller's.
fn pick(r: &Option<Vec<u16>>, orig: PCWSTR) -> PCWSTR {
    match r {
        Some(w) => PCWSTR(w.as_ptr()),
        None => orig,
    }
}

unsafe extern "system" fn h_create(name: PCWSTR, access: u32, share: FILE_SHARE_MODE, sa: *const SECURITY_ATTRIBUTES, disp: FILE_CREATION_DISPOSITION, flags: FILE_FLAGS_AND_ATTRIBUTES, template: HANDLE) -> HANDLE {
    let r = redirect(name, "CreateFileW");
    if r.is_some() {
        if OPEN_COUNT.fetch_add(1, Ordering::SeqCst) == 0 {
            crate::glog!("save", "first save open redirected; the mod is now live");
        }
        REDIRECTED.store(true, Ordering::SeqCst);
    }
    let h = D_CREATE.get().unwrap().call(pick(&r, name), access, share, sa, disp, flags, template);
    note_part("CreateFileW", name, !h.is_invalid());
    h
}
unsafe extern "system" fn h_attr(p: PCWSTR) -> u32 {
    let r = redirect(p, "GetFileAttributesW");
    let a = D_ATTR.get().unwrap().call(pick(&r, p));
    note_part("GetFileAttributesW", p, a != u32::MAX);
    a
}
unsafe extern "system" fn h_attr_ex(p: PCWSTR, level: i32, info: *mut core::ffi::c_void) -> BOOL {
    let r = redirect(p, "GetFileAttributesExW");
    let ok = D_ATTR_EX.get().unwrap().call(pick(&r, p), level, info);
    note_part("GetFileAttributesExW", p, ok.as_bool());
    ok
}
unsafe extern "system" fn h_move_ex(a: PCWSTR, b: PCWSTR, flags: u32) -> BOOL {
    let (ra, rb) = (redirect(a, "MoveFileExW(from)"), redirect(b, "MoveFileExW(to)"));
    D_MOVE_EX.get().unwrap().call(pick(&ra, a), pick(&rb, b), flags)
}
unsafe extern "system" fn h_move(a: PCWSTR, b: PCWSTR) -> BOOL {
    let (ra, rb) = (redirect(a, "MoveFileW(from)"), redirect(b, "MoveFileW(to)"));
    D_MOVE.get().unwrap().call(pick(&ra, a), pick(&rb, b))
}
unsafe extern "system" fn h_replace(a: PCWSTR, b: PCWSTR, c: PCWSTR, flags: u32, x: *mut core::ffi::c_void, y: *mut core::ffi::c_void) -> BOOL {
    let (ra, rb, rc) = (redirect(a, "ReplaceFileW(replaced)"), redirect(b, "ReplaceFileW(replacement)"), redirect(c, "ReplaceFileW(backup)"));
    D_REPLACE.get().unwrap().call(pick(&ra, a), pick(&rb, b), pick(&rc, c), flags, x, y)
}
unsafe extern "system" fn h_copy(a: PCWSTR, b: PCWSTR, fail_if_exists: BOOL) -> BOOL {
    let (ra, rb) = (redirect(a, "CopyFileW(from)"), redirect(b, "CopyFileW(to)"));
    D_COPY.get().unwrap().call(pick(&ra, a), pick(&rb, b), fail_if_exists)
}
unsafe extern "system" fn h_delete(a: PCWSTR) -> BOOL {
    let r = redirect(a, "DeleteFileW");
    D_DELETE.get().unwrap().call(pick(&r, a))
}

pub fn install(target: &str) -> Result<(), String> {
    let _ = TARGET.set(target.to_string());
    unsafe {
        let k32 = GetModuleHandleW(windows::core::w!("kernel32.dll")).map_err(|e| format!("kernel32: {e}"))?;
        macro_rules! hook {
            ($cell:ident, $ty:ty, $name:literal, $hook:ident) => {{
                let addr = GetProcAddress(k32, windows::core::s!($name)).ok_or(concat!($name, " not found"))?;
                let original: $ty = std::mem::transmute(addr);
                let d = GenericDetour::new(original, $hook).map_err(|e| format!("{}: {e}", $name))?;
                d.enable().map_err(|e| format!("{} enable: {e}", $name))?;
                let _ = $cell.set(d);
            }};
        }
        // Open first: if this one fails nothing else is installed and the caller aborts.
        hook!(D_CREATE, CreateFileWFn, "CreateFileW", h_create);
        hook!(D_ATTR, GetAttrFn, "GetFileAttributesW", h_attr);
        hook!(D_ATTR_EX, GetAttrExFn, "GetFileAttributesExW", h_attr_ex);
        hook!(D_MOVE_EX, MoveExFn, "MoveFileExW", h_move_ex);
        hook!(D_MOVE, MoveFn, "MoveFileW", h_move);
        hook!(D_REPLACE, ReplaceFn, "ReplaceFileW", h_replace);
        hook!(D_COPY, CopyFn, "CopyFileW", h_copy);
        hook!(D_DELETE, DeleteFn, "DeleteFileW", h_delete);
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

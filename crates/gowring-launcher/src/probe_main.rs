//! GowRProbe.exe — live-Kratos stage 1 (docs/LIVE-KRATOS-PLAN.md). Waits for God of War Ragnarök (GoWR.exe), loads
//! gowr_probe.dll (kept next to this exe) into it, says whether the probe hooked the game, and opens the probe's
//! folder when Ragnarök closes. Nothing is written into Ragnarök's own folder.

#[cfg(windows)]
fn main() {
    windows_main::run();
    println!("\nPress Enter to close this window.");
    let _ = std::io::stdin().read_line(&mut String::new());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("GowRProbe only runs on Windows.");
}

#[cfg(windows)]
mod windows_main {
    use std::path::PathBuf;
    use std::time::{Duration, Instant, SystemTime};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
    use windows_sys::Win32::System::Threading::*;

    const GAME: &str = "GoWR.exe";
    const DLL: &str = "gowr_probe.dll";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn name_of(raw: &[u16]) -> String {
        let n = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
        String::from_utf16_lossy(&raw[..n])
    }

    fn pid_of(name: &str) -> Option<u32> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                if name_of(&e.szExeFile).eq_ignore_ascii_case(name) {
                    found = Some(e.th32ProcessID);
                    break;
                }
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
            found
        }
    }

    /// The libraries loaded in a process, lower case (None if it cannot be read right now).
    fn modules_of(pid: u32) -> Option<Vec<String>> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut e: MODULEENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
            let mut out = Vec::new();
            let mut ok = Module32FirstW(snap, &mut e);
            while ok != 0 {
                out.push(name_of(&e.szModule).to_ascii_lowercase());
                ok = Module32NextW(snap, &mut e);
            }
            CloseHandle(snap);
            Some(out)
        }
    }

    fn log_dir() -> PathBuf {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("GowRing").join("gowr_probe").join("latest")
    }

    /// LoadLibraryW(dll) inside the process, on a thread of its own. Ok(true) if the library loaded.
    fn inject(pid: u32, dll: &std::path::Path) -> Result<bool, String> {
        let path = wide(&dll.to_string_lossy());
        let bytes = path.len() * 2;
        unsafe {
            let access = PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ;
            let h: HANDLE = OpenProcess(access, 0, pid);
            if h.is_null() {
                let err = GetLastError();
                return Err(if err == 5 {
                    "Windows refused access to Ragnarök (error 5). If Ragnarök runs as administrator, right-click GowRProbe.exe and choose \"Run as administrator\".".into()
                } else {
                    format!("could not open Ragnarök's process (error {err})")
                });
            }
            let close = |msg: String| {
                CloseHandle(h);
                Err(msg)
            };
            let mem = VirtualAllocEx(h, std::ptr::null(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
            if mem.is_null() {
                return close(format!("could not reserve memory in Ragnarök (error {})", GetLastError()));
            }
            let mut written = 0usize;
            if WriteProcessMemory(h, mem, path.as_ptr() as *const _, bytes, &mut written) == 0 {
                VirtualFreeEx(h, mem, 0, MEM_RELEASE);
                return close(format!("could not write the probe's path into Ragnarök (error {})", GetLastError()));
            }
            let k32 = GetModuleHandleW(wide("kernel32.dll").as_ptr());
            let Some(load) = GetProcAddress(k32, c"LoadLibraryW".as_ptr() as *const u8) else {
                VirtualFreeEx(h, mem, 0, MEM_RELEASE);
                return close("LoadLibraryW not found".into());
            };
            let start: LPTHREAD_START_ROUTINE = Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn(*mut core::ffi::c_void) -> u32>(load));
            let t = CreateRemoteThread(h, std::ptr::null(), 0, start, mem, 0, std::ptr::null_mut());
            if t.is_null() {
                VirtualFreeEx(h, mem, 0, MEM_RELEASE);
                return close(format!("could not start the probe inside Ragnarök (error {})", GetLastError()));
            }
            WaitForSingleObject(t, 30_000);
            let mut code = 0u32;
            GetExitCodeThread(t, &mut code);
            CloseHandle(t);
            VirtualFreeEx(h, mem, 0, MEM_RELEASE);
            CloseHandle(h);
            Ok(code != 0)
        }
    }

    pub fn run() {
        println!("GowRProbe 0.2 — live Kratos, stage 2: finding Ragnarök's camera (built with GowRing {})\n", env!("CARGO_PKG_VERSION"));
        println!("This loads a small probe into God of War Ragnarök. It watches how the game draws, saves a few");
        println!("screenshots and, when you press Ctrl+F9, searches Ragnarök's memory for its camera. While Ragnarök is");
        println!("not the window in front, it keeps Ragnarök from reading your Xbox-style controller and holds it to");
        println!("30 frames a second. It changes nothing in Ragnarök's files or saves.\n");
        let dll = match std::env::current_exe() {
            Ok(p) => p.with_file_name(DLL),
            Err(e) => return println!("Could not find where GowRProbe.exe is: {e}"),
        };
        if !dll.exists() {
            return println!("{DLL} is missing: keep it in the same folder as GowRProbe.exe ({}).", dll.display());
        }

        println!("Waiting for God of War Ragnarök... start it from Steam now if it isn't running.");
        let pid = loop {
            if let Some(p) = pid_of(GAME) {
                break p;
            }
            std::thread::sleep(Duration::from_secs(1));
        };
        println!("Found Ragnarök (process {pid}). Waiting for it to start drawing...");
        let t0 = Instant::now();
        loop {
            if pid_of(GAME) != Some(pid) {
                return println!("Ragnarök closed before the probe could start.");
            }
            if let Some(m) = modules_of(pid) {
                if m.iter().any(|n| n == DLL) {
                    println!("The probe is already running in this Ragnarök (it was started earlier). Nothing to do.");
                    return wait_and_open(pid);
                }
                if m.iter().any(|n| n == "d3d12.dll") && t0.elapsed() > Duration::from_secs(5) {
                    break;
                }
            }
            if t0.elapsed() > Duration::from_secs(180) {
                println!("Ragnarök hasn't loaded its graphics after 3 minutes; trying anyway.");
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_secs(5));

        let before = SystemTime::now();
        match inject(pid, &dll) {
            Ok(true) => println!("Probe loaded into Ragnarök."),
            Ok(false) => return println!("Ragnarök did not load the probe (LoadLibrary failed inside it). Please tell Claude."),
            Err(e) => return println!("Could not load the probe: {e}"),
        }

        let log = log_dir().join("probe.log");
        let t1 = Instant::now();
        let mut said = false;
        while t1.elapsed() < Duration::from_secs(20) {
            let fresh = std::fs::metadata(&log).and_then(|m| m.modified()).map(|t| t >= before).unwrap_or(false);
            if fresh {
                let text = std::fs::read_to_string(&log).unwrap_or_default();
                if text.contains("hooked: Present") {
                    println!("The probe is watching Ragnarök's graphics.");
                    said = true;
                    break;
                }
                if text.contains("FAILED to hook") {
                    println!("The probe could not hook Ragnarök's graphics; the log says why. Please send it to Claude.");
                    said = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if !said {
            println!("No word from the probe after 20 s; carry on, the log will say what happened.");
        }

        println!("\nWhat to do now (this probe's job is to find Ragnarök's camera):");
        println!("  1. Get into the game itself, not a menu: Kratos standing somewhere calm with no enemies.");
        println!("  2. With Ragnarök in front, press Ctrl+F9. ONE beep: hold still.");
        println!("  3. TWO beeps: move the camera. Turn it slowly all the way round, look up and down, walk a few steps.");
        println!("     Keep going until the next beep (about 20 seconds).");
        println!("  4. ONE LONG beep: let go of the controller and keep completely still.");
        println!("  5. TWO beeps: move the camera again, until...");
        println!("  6. THREE beeps: done. If you can, walk somewhere else and do it once more (Ctrl+F9 again).");
        println!("  7. Then put Elden Ring in front and play it for a minute, and close Ragnarök.");
        println!("  Ragnarök runs at 30 fps at most while it is behind Elden Ring, so Elden Ring has more room.");
        println!("\nThe log is being written to {}", log.display());
        wait_and_open(pid);
    }

    fn wait_and_open(pid: u32) {
        while pid_of(GAME) == Some(pid) {
            std::thread::sleep(Duration::from_secs(2));
        }
        let dir = log_dir();
        println!("\nRagnarök has closed. Please send Claude everything in this folder (probe.log, the camsearch_*.bin");
        println!("recordings and the shot_*.png pictures). If the .bin files won't attach, right-click the folder ->");
        println!("Send to -> Compressed (zipped) folder, and send the zip.\n  {}", dir.display());
        let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
    }
}

//! The probe's own thread: every 10 s a line of figures (frames per second, slowest frame, how many frames were drawn
//! in front / behind / minimised, Ragnarök's video memory, controller reads, whether Elden Ring is running), focus
//! changes as they happen, and when to take a screenshot: at 20 s, 60 s and 120 s after the first frame, after 3 s
//! behind another window (up to 3), 3 s after coming back in front (up to 2), and on Ctrl+F10. Ctrl+F9 (Ragnarök in
//! front) starts a camera search (`camsearch`).

use crate::d3d::{self, ADAPTER, FIRST_PRESENT, HWND_MAIN, SHOTS_TAKEN, SHOT_LABEL, SHOT_WANTED};
use crate::{input, plog};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::Graphics::Dxgi::{DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_F10, VK_F9};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, IsIconic};

/// The game's executable: name, size and link time (which build of Ragnarök this is).
pub fn exe_identity() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    let size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    let stamp = unsafe {
        GetModuleHandleW(None).map(|m| {
            let base = m.0 as *const u8;
            let pe = *(base.add(0x3c) as *const u32) as usize;
            *(base.add(pe + 8) as *const u32)
        })
    }
    .unwrap_or(0);
    format!("{} ({size} bytes, PE time stamp {stamp:#x})", exe.display())
}

fn module_names(pid: u32) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid) else { return out };
        let mut e = MODULEENTRY32W { dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32, ..Default::default() };
        let mut ok = Module32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e.szModule.iter().position(|c| *c == 0).unwrap_or(e.szModule.len());
            out.push(String::from_utf16_lossy(&e.szModule[..n]));
            ok = Module32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

static SEEN: std::sync::Mutex<BTreeSet<String>> = std::sync::Mutex::new(BTreeSet::new());

/// Every library loaded into Ragnarök (only new ones after the first call), and the ones that read input or hook
/// the screen picked out.
pub fn log_modules(when: &str) {
    let names = module_names(unsafe { GetCurrentProcessId() });
    let mut seen = SEEN.lock().unwrap();
    let new: Vec<String> = names.into_iter().filter(|n| seen.insert(n.to_ascii_lowercase())).collect();
    if new.is_empty() {
        return;
    }
    plog!("modules", "{when}: {} new", new.len());
    for chunk in new.chunks(8) {
        plog!("modules", "  {}", chunk.join(", "));
    }
    const NOTE: [&str; 14] = [
        "xinput", "dinput", "hid.dll", "gaminginput", "gameinput", "sce", "pad", "steam", "sl.", "nvngx", "ffx", "xess", "reshade",
        "dxgi",
    ];
    let notable: Vec<&String> = new.iter().filter(|n| NOTE.iter().any(|k| n.to_ascii_lowercase().contains(k))).collect();
    if !notable.is_empty() {
        plog!("modules", "  input / graphics related: {}", notable.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
    }
}

fn process_running(name: &str) -> bool {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return false };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        let mut found = false;
        while ok {
            let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case(name) {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
        found
    }
}

fn window_title(h: HWND) -> String {
    let mut t = [0u16; 96];
    let n = unsafe { GetWindowTextW(h, &mut t) }.max(0) as usize;
    String::from_utf16_lossy(&t[..n])
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Focus {
    Front,
    Behind,
    Minimised,
}

fn want_shot(label: &str) {
    if SHOTS_TAKEN.load(Relaxed) >= d3d::MAX_SHOTS || SHOT_WANTED.load(Relaxed) != 0 {
        return;
    }
    if let Ok(mut l) = SHOT_LABEL.lock() {
        *l = label.to_string();
    }
    SHOT_WANTED.store(1, Relaxed);
    plog!("shot", "asked for '{label}'");
}

pub fn run() {
    let start = Instant::now();
    let mut last_stats = Instant::now();
    let mut last = (0u64, 0u64, 0u64, 0u64);
    let mut timed = [(20u64, false), (60, false), (120, false)];
    let mut focus = Focus::Front;
    let mut focus_since = Instant::now();
    let mut behind_shots = 0;
    let mut front_shots = 0;
    let mut was_behind = false;
    let mut shot_this_spell = false;
    let mut key_down = false;
    let mut search_key = false;
    // testing aid (tools/fakes): GOWR_PROBE_AUTOSEARCH=<s> starts a camera search that long after the first frame
    let mut auto_search = std::env::var("GOWR_PROBE_AUTOSEARCH").ok().and_then(|v| v.parse::<u64>().ok());
    let mut late_checks = [(30u64, false), (60, false), (180, false)];
    let mut counting = false;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let main = HWND_MAIN.load(Relaxed);

        for (secs, done) in late_checks.iter_mut() {
            if !*done && start.elapsed() >= Duration::from_secs(*secs) {
                *done = true;
                log_modules(&format!("after {secs} s"));
                input::install();
                if main == 0 {
                    plog!("stats", "no frame from Ragnarök seen yet ({secs} s after the probe started)");
                }
            }
        }
        if main == 0 {
            continue;
        }
        let h = HWND(main as *mut _);
        if !counting {
            // the first 10 s window starts at the first frame
            counting = true;
            last_stats = Instant::now();
            last.0 = d3d::FRAMES.load(Relaxed);
            last.1 = d3d::FG_FRAMES.load(Relaxed);
        }

        if let Some(t0) = FIRST_PRESENT.get() {
            for (secs, done) in timed.iter_mut() {
                if !*done && t0.elapsed() >= Duration::from_secs(*secs) {
                    *done = true;
                    want_shot(&format!("{secs}s-{}", if focus == Focus::Front { "front" } else { "behind" }));
                }
            }
        }

        let now_focus = unsafe {
            if IsIconic(h).as_bool() {
                Focus::Minimised
            } else if GetForegroundWindow() == h {
                Focus::Front
            } else {
                Focus::Behind
            }
        };
        if now_focus != focus {
            let fg = unsafe { GetForegroundWindow() };
            plog!("focus", "Ragnarök is now {:?} (window in front: '{}') after {:.1} s {:?}", now_focus, window_title(fg), focus_since.elapsed().as_secs_f64(), focus);
            if focus == Focus::Behind || focus == Focus::Minimised {
                was_behind = true;
            }
            focus = now_focus;
            focus_since = Instant::now();
            shot_this_spell = false;
        }
        let held = focus_since.elapsed() >= Duration::from_secs(3);
        if held && !shot_this_spell {
            if focus == Focus::Behind && behind_shots < 3 {
                behind_shots += 1;
                shot_this_spell = true;
                want_shot(&format!("behind-{behind_shots}"));
            } else if focus == Focus::Front && was_behind && front_shots < 2 {
                front_shots += 1;
                shot_this_spell = true;
                want_shot(&format!("front-again-{front_shots}"));
            }
        }

        let down = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 && GetAsyncKeyState(VK_F10.0 as i32) < 0 };
        if down && !key_down {
            want_shot("ctrl-f10");
        }
        key_down = down;
        let f9 = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 && GetAsyncKeyState(VK_F9.0 as i32) < 0 };
        if f9 && !search_key && focus == Focus::Front {
            crate::camsearch::start();
        }
        search_key = f9;
        if let (Some(secs), Some(t0)) = (auto_search, FIRST_PRESENT.get()) {
            if t0.elapsed() >= Duration::from_secs(secs) {
                auto_search = None;
                crate::camsearch::start();
            }
        }

        if last_stats.elapsed() >= Duration::from_secs(10) {
            let secs = last_stats.elapsed().as_secs_f64();
            last_stats = Instant::now();
            let now = (
                d3d::FRAMES.load(Relaxed),
                d3d::FG_FRAMES.load(Relaxed),
                d3d::BG_FRAMES.load(Relaxed),
                d3d::MIN_FRAMES.load(Relaxed),
            );
            let frames = now.0 - last.0;
            let worst = d3d::WORST_US.swap(0, Relaxed) as f64 / 1000.0;
            let vram = ADAPTER
                .get()
                .map(|a| unsafe {
                    let mut l = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                    let mut n = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                    let _ = a.0.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut l);
                    let _ = a.0.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut n);
                    format!(
                        "vram {} / {} MB budget (+{} MB in system memory)",
                        l.CurrentUsage >> 20,
                        l.Budget >> 20,
                        n.CurrentUsage >> 20
                    )
                })
                .unwrap_or_else(|| "vram unknown (no adapter yet)".into());
            plog!(
                "stats",
                "{:.1} fps (slowest frame {worst:.1} ms) | frames in front {}, behind {}, minimised {} | now {focus:?} | {vram} | xinput reads {}, muted {} | Elden Ring {}",
                frames as f64 / secs,
                now.1 - last.1,
                now.2 - last.2,
                now.3 - last.3,
                input::READS.load(Relaxed),
                input::MUTED.load(Relaxed),
                if process_running("eldenring.exe") { "running" } else { "not running" }
            );
            if frames == 0 {
                plog!("stats", "NO frames in the last {secs:.0} s (Ragnarök stopped drawing while {focus:?})");
            }
            last = now;
        }
    }
}

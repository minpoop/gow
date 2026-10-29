//! Ragnarök's D3D12, watched from the inside.
//!
//! The vtables come from a throwaway device, queue and swapchain on a hidden window (every swapchain and queue in the
//! process shares them). Hooked: `IDXGISwapChain::Present`, `IDXGISwapChain1::Present1` and
//! `ID3D12CommandQueue::ExecuteCommandLists` (only to learn the game's DIRECT queues: a back-buffer copy has to go
//! on the queue that drew the frame, taken as the last DIRECT queue the presenting thread submitted to).
//!
//! Per frame: counted, timed, and marked foreground / background / minimised. On request (`monitor`), the back buffer
//! is copied to a readback buffer on that queue, waited for (a few ms, rare) and handed to `shot` to save.

use crate::plog;
use retour::GenericDetour;
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use windows::core::{Interface, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, GetWindowTextW, IsIconic, RegisterClassExW,
    WINDOW_EX_STYLE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;
type Present1Fn = unsafe extern "system" fn(*mut c_void, u32, u32, *const c_void) -> HRESULT;
type ExecFn = unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void);

static D_PRESENT: OnceLock<GenericDetour<PresentFn>> = OnceLock::new();
static D_PRESENT1: OnceLock<GenericDetour<Present1Fn>> = OnceLock::new();
static D_EXEC: OnceLock<GenericDetour<ExecFn>> = OnceLock::new();

const DXGI_PRESENT_TEST_FLAG: u32 = 0x1;

// ---- what the monitor reads ---------------------------------------------------------------------------------------

pub static FRAMES: AtomicU64 = AtomicU64::new(0);
pub static FG_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static BG_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIN_FRAMES: AtomicU64 = AtomicU64::new(0);
/// Longest gap between two presents since the monitor last reset it, in microseconds.
pub static WORST_US: AtomicU64 = AtomicU64::new(0);
/// The game's swapchain window (the first swapchain that presented).
pub static HWND_MAIN: AtomicIsize = AtomicIsize::new(0);
/// A screenshot wanted at the next present (the monitor sets it with its label).
pub static SHOT_WANTED: AtomicU32 = AtomicU32::new(0);
pub static SHOT_LABEL: Mutex<String> = Mutex::new(String::new());
pub static SHOTS_TAKEN: AtomicU32 = AtomicU32::new(0);
pub const MAX_SHOTS: u32 = 20;
pub static FIRST_PRESENT: OnceLock<Instant> = OnceLock::new();
/// The swapchain's size (the camera search wants the screen's shape).
pub static SWAP_W: AtomicU32 = AtomicU32::new(0);
pub static SWAP_H: AtomicU32 = AtomicU32::new(0);
/// While Ragnarök is behind another window it is held to this many frames a second (it ran at 100+ behind Elden
/// Ring and slowed Elden Ring's loading); 0 = no cap.
pub const BEHIND_FPS_CAP: u64 = 30;

/// The adapter Ragnarök draws on, for its video memory figures.
pub struct Adapter(pub IDXGIAdapter3);
unsafe impl Send for Adapter {}
unsafe impl Sync for Adapter {}
pub static ADAPTER: OnceLock<Adapter> = OnceLock::new();

// ---- queues --------------------------------------------------------------------------------------------------------

/// Queues seen in ExecuteCommandLists: (pointer, is DIRECT).
static QUEUES: Mutex<Vec<(usize, bool)>> = Mutex::new(Vec::new());
static LAST_DIRECT: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static LAST_DIRECT_HERE: Cell<usize> = const { Cell::new(0) };
    /// Set while the probe itself submits or presents, so its own calls are not counted.
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn h_exec(q: *mut c_void, n: u32, lists: *const *mut c_void) {
    if !INSIDE.with(|c| c.get()) {
        note_queue(q as usize);
    }
    D_EXEC.get().unwrap().call(q, n, lists)
}

fn note_queue(q: usize) {
    let direct = {
        let mut g = match QUEUES.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match g.iter().find(|(p, _)| *p == q) {
            Some((_, d)) => *d,
            None => {
                let raw = q as *mut c_void;
                let d = unsafe { ID3D12CommandQueue::from_raw_borrowed(&raw) }
                    .map(|cq| unsafe { cq.GetDesc() }.Type == D3D12_COMMAND_LIST_TYPE_DIRECT)
                    .unwrap_or(false);
                if g.len() < 64 {
                    g.push((q, d));
                    let kinds = g.iter().filter(|e| e.1).count();
                    plog!("queue", "new command queue {q:#x} ({}); {} queues so far, {kinds} DIRECT", if d { "DIRECT" } else { "other" }, g.len());
                }
                d
            }
        }
    };
    if direct {
        LAST_DIRECT.store(q, Relaxed);
        LAST_DIRECT_HERE.with(|c| c.set(q));
    }
}

// ---- present -------------------------------------------------------------------------------------------------------

unsafe extern "system" fn h_present(sc: *mut c_void, sync: u32, flags: u32) -> HRESULT {
    on_present(sc, sync, flags, "Present");
    INSIDE.with(|c| c.set(true));
    let r = D_PRESENT.get().unwrap().call(sc, sync, flags);
    INSIDE.with(|c| c.set(false));
    r
}

unsafe extern "system" fn h_present1(sc: *mut c_void, sync: u32, flags: u32, params: *const c_void) -> HRESULT {
    on_present(sc, sync, flags, "Present1");
    INSIDE.with(|c| c.set(true));
    let r = D_PRESENT1.get().unwrap().call(sc, sync, flags, params);
    INSIDE.with(|c| c.set(false));
    r
}

static LAST_AT: Mutex<Option<Instant>> = Mutex::new(None);
static OTHER_SWAPCHAIN: AtomicU32 = AtomicU32::new(0);

fn on_present(sc: *mut c_void, sync: u32, flags: u32, api: &str) {
    if INSIDE.with(|c| c.get()) || flags & DXGI_PRESENT_TEST_FLAG != 0 {
        return;
    }
    let _ = std::panic::catch_unwind(|| unsafe { present_inner(sc, sync, flags, api) });
}

unsafe fn present_inner(raw: *mut c_void, sync: u32, flags: u32, api: &str) {
    let Some(sc) = IDXGISwapChain::from_raw_borrowed(&raw) else { return };
    let Ok(sc1) = sc.cast::<IDXGISwapChain1>() else { return };
    let hwnd = sc1.GetHwnd().map(|h| h.0 as isize).unwrap_or(0);
    let main = HWND_MAIN.load(Relaxed);
    if main == 0 {
        HWND_MAIN.store(hwnd, Relaxed);
        let _ = FIRST_PRESENT.set(Instant::now());
        describe(&sc1, hwnd, sync, flags, api);
    } else if hwnd != main {
        if OTHER_SWAPCHAIN.fetch_add(1, Relaxed) == 0 {
            plog!("present", "a second swapchain presents (window {hwnd:#x}); only the first is watched");
        }
        return;
    }

    let h = HWND(hwnd as *mut c_void);
    let behind = GetForegroundWindow() != h;
    let mut now = Instant::now();
    if let Ok(mut last) = LAST_AT.lock() {
        if let (true, Some(t), true) = (behind, *last, BEHIND_FPS_CAP > 0) {
            let min = std::time::Duration::from_micros(1_000_000 / BEHIND_FPS_CAP);
            let gap = now.duration_since(t);
            if gap < min {
                std::thread::sleep(min - gap);
                now = Instant::now();
            }
        }
        if let Some(t) = *last {
            WORST_US.fetch_max(now.duration_since(t).as_micros() as u64, Relaxed);
        }
        *last = Some(now);
    }
    FRAMES.fetch_add(1, Relaxed);
    crate::steer::restore();
    if IsIconic(h).as_bool() {
        MIN_FRAMES.fetch_add(1, Relaxed);
    } else if !behind {
        FG_FRAMES.fetch_add(1, Relaxed);
    } else {
        BG_FRAMES.fetch_add(1, Relaxed);
    }

    let queue = match LAST_DIRECT_HERE.with(|c| c.get()) {
        0 => LAST_DIRECT.load(Relaxed),
        q => q,
    };
    if queue == 0 {
        return;
    }
    let qraw = queue as *mut c_void;
    let Some(q) = ID3D12CommandQueue::from_raw_borrowed(&qraw) else { return };
    if ADAPTER.get().is_none() {
        find_adapter(q);
    }
    if SHOT_WANTED.swap(0, Relaxed) != 0 && SHOTS_TAKEN.load(Relaxed) < MAX_SHOTS {
        let label = SHOT_LABEL.lock().map(|s| s.clone()).unwrap_or_default();
        let t = Instant::now();
        INSIDE.with(|c| c.set(true));
        let r = capture(&sc1, q);
        INSIDE.with(|c| c.set(false));
        match r {
            Ok(img) => {
                let n = SHOTS_TAKEN.fetch_add(1, Relaxed) + 1;
                plog!("shot", "#{n} '{label}': back buffer copied in {:.1} ms (queue {queue:#x})", t.elapsed().as_secs_f64() * 1000.0);
                std::thread::spawn(move || crate::shot::save(img, n, &label));
            }
            Err(e) => plog!("shot", "'{label}': copy FAILED: {e}"),
        }
    }
}

/// The first frame: what Ragnarök presents and how.
unsafe fn describe(sc: &IDXGISwapChain1, hwnd: isize, sync: u32, flags: u32, api: &str) {
    if let Ok(d) = sc.GetDesc1() {
        SWAP_W.store(d.Width, Relaxed);
        SWAP_H.store(d.Height, Relaxed);
        plog!(
            "present",
            "first frame via {api}: {}x{} format {} ({}), {} buffers, swap effect {}, scaling {}, alpha {}, flags {:#x}; sync interval {sync}, present flags {flags:#x}",
            d.Width, d.Height, d.Format.0, format_name(d.Format), d.BufferCount, d.SwapEffect.0, d.Scaling.0, d.AlphaMode.0, d.Flags
        );
    }
    let mut full = BOOL(0);
    let _ = sc.GetFullscreenState(Some(&mut full), None);
    let mut title = [0u16; 128];
    let n = GetWindowTextW(HWND(hwnd as *mut c_void), &mut title).max(0) as usize;
    plog!(
        "present",
        "window {hwnd:#x} '{}', exclusive fullscreen: {}; swapchain is IDXGISwapChain4: {}",
        String::from_utf16_lossy(&title[..n]),
        full.as_bool(),
        sc.cast::<IDXGISwapChain4>().is_ok()
    );
}

unsafe fn queue_device(q: &ID3D12CommandQueue) -> windows::core::Result<ID3D12Device> {
    let mut dev: Option<ID3D12Device> = None;
    q.GetDevice(&mut dev)?;
    dev.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))
}

unsafe fn find_adapter(q: &ID3D12CommandQueue) {
    let Ok(dev) = queue_device(q) else { return };
    let luid = dev.GetAdapterLuid();
    let Ok(f) = CreateDXGIFactory1::<IDXGIFactory4>() else { return };
    let Ok(a) = f.EnumAdapterByLuid::<IDXGIAdapter3>(luid) else { return };
    if let Ok(d) = a.GetDesc2() {
        let n = d.Description.iter().position(|c| *c == 0).unwrap_or(d.Description.len());
        plog!(
            "gpu",
            "adapter '{}': {} MB dedicated video memory, {} MB shared; device feature level {:#x}",
            String::from_utf16_lossy(&d.Description[..n]),
            d.DedicatedVideoMemory / (1 << 20),
            d.SharedSystemMemory / (1 << 20),
            feature_level(&dev)
        );
    }
    let _ = ADAPTER.set(Adapter(a));
}

unsafe fn feature_level(dev: &ID3D12Device) -> u32 {
    let levels = [0xc200, 0xc100, 0xc000, 0xb100, 0xb000];
    let mut info = D3D12_FEATURE_DATA_FEATURE_LEVELS {
        NumFeatureLevels: levels.len() as u32,
        pFeatureLevelsRequested: levels.as_ptr() as *const _,
        MaxSupportedFeatureLevel: Default::default(),
    };
    let ok = dev.CheckFeatureSupport(
        D3D12_FEATURE_FEATURE_LEVELS,
        &mut info as *mut _ as *mut c_void,
        std::mem::size_of_val(&info) as u32,
    );
    if ok.is_ok() {
        info.MaxSupportedFeatureLevel.0 as u32
    } else {
        0
    }
}

pub fn format_name(f: DXGI_FORMAT) -> &'static str {
    match f {
        DXGI_FORMAT_R8G8B8A8_UNORM => "RGBA8",
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => "RGBA8 sRGB",
        DXGI_FORMAT_B8G8R8A8_UNORM => "BGRA8",
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => "BGRA8 sRGB",
        DXGI_FORMAT_R10G10B10A2_UNORM => "RGB10A2 (often HDR10)",
        DXGI_FORMAT_R16G16B16A16_FLOAT => "RGBA16F (scRGB HDR)",
        _ => "other",
    }
}

// ---- the back-buffer copy ------------------------------------------------------------------------------------------

/// One copied back buffer, tightly packed rows (`row_bytes` each).
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub row_bytes: usize,
    pub format: DXGI_FORMAT,
    pub data: Vec<u8>,
}

fn transition(r: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // borrowed, not AddRef'd: the barrier never releases it
                pResource: unsafe { std::mem::transmute_copy(r) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

unsafe fn capture(sc: &IDXGISwapChain1, q: &ID3D12CommandQueue) -> Result<Image, String> {
    let e = |what: &str| { let what = what.to_string(); move |err: windows::core::Error| format!("{what}: {err}") };
    let sc3 = sc.cast::<IDXGISwapChain3>().map_err(e("IDXGISwapChain3"))?;
    let dev = queue_device(q).map_err(e("queue device"))?;
    let buf: ID3D12Resource = sc3.GetBuffer(sc3.GetCurrentBackBufferIndex()).map_err(e("GetBuffer"))?;
    let desc = buf.GetDesc();
    if desc.SampleDesc.Count != 1 {
        return Err(format!("back buffer is multisampled ({}x)", desc.SampleDesc.Count));
    }
    let mut fp = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let (mut rows, mut row_bytes, mut total) = (0u32, 0u64, 0u64);
    dev.GetCopyableFootprints(&desc, 0, 1, 0, Some(&mut fp), Some(&mut rows), Some(&mut row_bytes), Some(&mut total));

    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let bdesc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: total,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut rb: Option<ID3D12Resource> = None;
    dev.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &bdesc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut rb)
        .map_err(e("readback buffer"))?;
    let rb = rb.ok_or("no readback buffer")?;
    let alloc: ID3D12CommandAllocator = dev.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).map_err(e("allocator"))?;
    let list: ID3D12GraphicsCommandList =
        dev.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None).map_err(e("command list"))?;
    list.ResourceBarrier(&[transition(&buf, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
    let dst = D3D12_TEXTURE_COPY_LOCATION {
        pResource: std::mem::transmute_copy(&rb),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: fp },
    };
    let src = D3D12_TEXTURE_COPY_LOCATION {
        pResource: std::mem::transmute_copy(&buf),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
    list.ResourceBarrier(&[transition(&buf, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT)]);
    list.Close().map_err(e("Close"))?;
    q.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().map_err(e("list"))?)]);

    let fence: ID3D12Fence = dev.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(e("fence"))?;
    q.Signal(&fence, 1).map_err(e("Signal"))?;
    let ev = CreateEventW(None, false, false, PCWSTR::null()).map_err(e("event"))?;
    fence.SetEventOnCompletion(1, ev).map_err(e("SetEventOnCompletion"))?;
    let waited = WaitForSingleObject(ev, 2000);
    let _ = CloseHandle(ev);
    if waited.0 != 0 {
        return Err("the GPU did not finish the copy within 2 s".into());
    }

    let mut p: *mut c_void = std::ptr::null_mut();
    let range = D3D12_RANGE { Begin: 0, End: total as usize };
    rb.Map(0, Some(&range), Some(&mut p)).map_err(e("Map"))?;
    let pitch = fp.Footprint.RowPitch as usize;
    let rb_len = row_bytes as usize;
    let mut data = Vec::with_capacity(rb_len * rows as usize);
    for y in 0..rows as usize {
        let row = std::slice::from_raw_parts((p as *const u8).add(fp.Offset as usize + y * pitch), rb_len);
        data.extend_from_slice(row);
    }
    rb.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
    Ok(Image { width: desc.Width as u32, height: desc.Height, row_bytes: rb_len, format: desc.Format, data })
}

// ---- install -------------------------------------------------------------------------------------------------------

unsafe extern "system" fn dummy_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

pub fn install() -> Result<(), String> {
    unsafe {
        // a window class of its own: the built-in ones are not always registered in a process with no windows yet
        let inst = GetModuleHandleW(None).map_err(|e| format!("module handle: {e}"))?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(dummy_proc),
            hInstance: inst.into(),
            lpszClassName: windows::core::w!("GowRProbeDummy"),
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(format!("window class: {}", windows::core::Error::from_win32()));
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::w!("GowRProbeDummy"),
            windows::core::w!("gowr-probe"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            64,
            64,
            None,
            None,
            inst,
            None,
        )
        .map_err(|e| format!("dummy window: {e}"))?;
        plog!("d3d", "helper window made");
        let r = vtables(hwnd);
        let _ = DestroyWindow(hwnd);
        let (present, present1, exec) = r?;
        plog!("d3d", "Present {present:#x}, Present1 {present1:#x}, ExecuteCommandLists {exec:#x}");
        let d = GenericDetour::new(std::mem::transmute::<usize, ExecFn>(exec), h_exec as ExecFn).map_err(|e| format!("ExecuteCommandLists: {e}"))?;
        d.enable().map_err(|e| format!("ExecuteCommandLists enable: {e}"))?;
        plog!("d3d", "ExecuteCommandLists hooked");
        let _ = D_EXEC.set(d);
        let d = GenericDetour::new(std::mem::transmute::<usize, PresentFn>(present), h_present as PresentFn).map_err(|e| format!("Present: {e}"))?;
        d.enable().map_err(|e| format!("Present enable: {e}"))?;
        plog!("d3d", "Present hooked");
        let _ = D_PRESENT.set(d);
        let d = GenericDetour::new(std::mem::transmute::<usize, Present1Fn>(present1), h_present1 as Present1Fn).map_err(|e| format!("Present1: {e}"))?;
        d.enable().map_err(|e| format!("Present1 enable: {e}"))?;
        plog!("d3d", "Present1 hooked");
        let _ = D_PRESENT1.set(d);
    }
    Ok(())
}

/// The addresses of Present, Present1 and ExecuteCommandLists, from throwaway objects on the default adapter.
unsafe fn vtables(hwnd: HWND) -> Result<(usize, usize, usize), String> {
    let mut dev: Option<ID3D12Device> = None;
    D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &mut dev).map_err(|e| format!("D3D12CreateDevice: {e}"))?;
    let dev = dev.ok_or("no device")?;
    plog!("d3d", "helper device made");
    let q: ID3D12CommandQueue = dev
        .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })
        .map_err(|e| format!("CreateCommandQueue: {e}"))?;
    let f: IDXGIFactory2 = CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 64,
        Height: 64,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        ..Default::default()
    };
    let sc: IDXGISwapChain1 = f.CreateSwapChainForHwnd(&q, hwnd, &desc, None, None).map_err(|e| format!("CreateSwapChainForHwnd: {e}"))?;
    plog!("d3d", "helper swapchain made");
    let vt = Interface::vtable(&sc);
    let qvt = Interface::vtable(&q);
    Ok((vt.base__.Present as usize, vt.Present1 as usize, qvt.ExecuteCommandLists as usize))
}

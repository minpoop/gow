//! Stand-in for GoWR.exe (God of War Ragnarök) to test GowRProbe.exe under Wine: loads d3d12.dll like the game, then
//! idles so the probe can be loaded into it, and exits. With FAKE_GOWR_DRAW=1 it instead opens a window and presents
//! frames through D3D12 (left half red, right half blue), for the probe's frame counting and screenshots, after
//! FAKE_GOWR_DELAY seconds (default 0: under Wine's software D3D12 a device made while another thread draws hangs).
//! While drawing it keeps a fake camera laid out like Ragnarök's (`fake_camera`, `builder`), circling
//! (100, 20, -300) at 5 m; the left half turns yellow if the camera it was drawn with is higher than 20 m. Like a player following the probe's beeps, the camera moves for 22 s and then holds
//! still for 8, over and over.
use std::time::{Duration, Instant};
use windows::core::{w, Interface};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::*;

fn main() {
    let secs = std::env::var("FAKE_GOWR_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(40);
    let loaded = unsafe { LoadLibraryW(w!("d3d12.dll")) }.is_ok();
    println!("fake GoWR: d3d12.dll {}", if loaded { "loaded" } else { "NOT loaded" });
    if std::env::var("FAKE_GOWR_DRAW").as_deref() == Ok("1") {
        let delay = std::env::var("FAKE_GOWR_DELAY").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        std::thread::sleep(Duration::from_secs(delay));
        match unsafe { draw(Duration::from_secs(secs)) } {
            Ok(n) => println!("fake GoWR: presented {n} frames"),
            Err(e) => println!("fake GoWR: drawing failed: {e}"),
        }
    } else {
        std::thread::sleep(Duration::from_secs(secs));
    }
    println!("fake GoWR: exiting");
}

unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

fn barrier(r: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: unsafe { std::mem::transmute_copy(r) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Ragnarök's camera as 0.3.0 and 0.4.0 found it: kept in the program's own data (`GAME_CAM`, the camera 0x110 bytes
/// in): its view matrix (rows from +0x30, translation -axis·position at +0x70) and, rows of 16 bytes from +0xc0, its
/// x, y and z axes (right, up, back), then its position at +0xf0. Every frame the game builds a record and
/// `copy_camera` copies it there (`fake_camera`), then `builder` turns it into the per-view block of floats: view at
/// 0x80, camera-to-world at 0xc0, transposed reversed-Z projection at 0x100.
#[repr(C, align(16))]
struct Kb([f32; 256]);
static mut GAME_CAM: Kb = Kb([0.0; 256]);
const CAM: usize = 0x110 / 4;

fn fake_camera(src: &mut [f64; 4], t: f32) {
    let moving = (t / 30.0).floor() * 22.0 + (t % 30.0).min(22.0);
    let yaw = moving * 0.5;
    let p = [100.0 + 5.0 * yaw.sin() as f64, 20.0, -300.0 - 5.0 * yaw.cos() as f64];
    for k in 0..3 {
        unsafe { std::ptr::write_volatile(&mut src[k], p[k]) };
    }
    let p = [p[0] as f32, p[1] as f32, p[2] as f32];
    let f = [-yaw.sin(), 0.0, yaw.cos()];
    let u = [0.0, 1.0, 0.0];
    let r = [u[1] * f[2] - u[2] * f[1], u[2] * f[0] - u[0] * f[2], u[0] * f[1] - u[1] * f[0]];
    let b = [-f[0], -f[1], -f[2]];
    let d = |a: [f32; 3]| -(a[0] * p[0] + a[1] * p[1] + a[2] * p[2]);
    // the record, as Ragnarök's: view matrix rows from +0x30 (translation at +0x70), axes from +0xc0, position +0xf0
    let mut record = [0f32; 64];
    let view = [[r[0], u[0], b[0], 0.0], [r[1], u[1], b[1], 0.0], [r[2], u[2], b[2], 0.0], [d(r), d(u), d(b), 1.0]];
    let rows = [[r[0], r[1], r[2], 0.0], [u[0], u[1], u[2], 0.0], [b[0], b[1], b[2], 0.0], [p[0], p[1], p[2], 1.0]];
    for k in 0..4 {
        for c in 0..4 {
            record[0x30 / 4 + k * 4 + c] = view[k][c];
            record[0xc0 / 4 + k * 4 + c] = rows[k][c];
        }
    }
    unsafe {
        let g = std::ptr::addr_of_mut!(GAME_CAM.0) as *mut f32;
        copy_camera(g.add(CAM), record.as_ptr());
        finish_camera(g.add(CAM));
    }
}

/// A second writer, as GoWR.exe+0x498b56: reads the camera back and writes its rows again (whole 16-byte rows).
#[inline(never)]
fn finish_camera(cam: *mut f32) {
    use std::arch::x86_64::{_mm_loadu_ps, _mm_storeu_ps};
    let mut rows = [unsafe { _mm_loadu_ps(cam) }; 4];
    for k in 0..4 {
        rows[k] = unsafe { _mm_loadu_ps(cam.add(0xc0 / 4 + k * 4)) };
    }
    std::hint::black_box(&mut rows);
    for k in 0..4 {
        unsafe { _mm_storeu_ps(cam.add(0xc0 / 4 + k * 4), rows[k]) };
    }
}

/// Copies a camera record into the camera's place (rcx = where to, rdx = the record) 16 bytes at a time, as
/// GoWR.exe+0x3cb18e does (movups).
#[inline(never)]
extern "system" fn copy_camera(dst: *mut f32, src: *const f32) {
    use std::arch::x86_64::{_mm_loadu_ps, _mm_storeu_ps};
    for k in 0..16 {
        unsafe { _mm_storeu_ps(dst.add(k * 4), _mm_loadu_ps(src.add(k * 4))) };
    }
}

/// Fills the per-view block from the game's camera; returns the camera's height as the block has it.
#[inline(never)]
fn builder(cam: &mut [f32; 256], aspect: f32, n: u64) -> f32 {
    let g = unsafe { std::ptr::addr_of!(GAME_CAM.0) as *const f32 };
    let at = |i: usize| unsafe { std::ptr::read_volatile(g.add(CAM + i / 4)) };
    let axis = |o: usize| [at(o), at(o + 4), at(o + 8)];
    let (r, u, b, p) = (axis(0xc0), axis(0xd0), axis(0xe0), axis(0xf0));
    let sy = 1.0 / (30f32.to_radians()).tan();
    let j = if n % 2 == 0 { 0.0004 } else { -0.0004 };
    let proj = [sy / aspect, 0.0, j, 0.0, 0.0, sy, -j, 0.0, 0.0, 0.0, 0.0, 0.1, 0.0, 0.0, -1.0, 0.0];
    let view = [at(0x30), at(0x40), at(0x50), at(0x70), at(0x34), at(0x44), at(0x54), at(0x74), at(0x38), at(0x48), at(0x58), at(0x78)];
    let to_world = [r[0], u[0], b[0], p[0], r[1], u[1], b[1], p[1], r[2], u[2], b[2], p[2]];
    for k in 0..12 {
        unsafe {
            std::ptr::write_volatile(&mut cam[32 + k], view[k]);
            std::ptr::write_volatile(&mut cam[48 + k], to_world[k]);
        }
    }
    for k in 0..16 {
        unsafe { std::ptr::write_volatile(&mut cam[64 + k], proj[k]) };
    }
    p[1]
}

/// What the probe's GOWR_PROBE_STEER needs for this stand-in: its time stamp, the camera's and the builder's RVAs.
fn steer_env() -> String {
    unsafe {
        let base = GetModuleHandleW(None).map(|m| m.0 as usize).unwrap_or(0);
        let pe = *((base + 0x3c) as *const u32) as usize;
        let stamp = *((base + pe + 8) as *const u32);
        let cam = std::ptr::addr_of!(GAME_CAM.0) as usize + 0x110 - base;
        format!("{stamp:x},{cam:x},{:x},{:x}", builder as *const () as usize - base, copy_camera as *const () as usize - base)
    }
}

unsafe fn draw(run: Duration) -> windows::core::Result<u64> {
    let (w, h) = (640u32, 360u32);
    let inst: HINSTANCE = GetModuleHandleW(None)?.into();
    let class = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(proc_), hInstance: inst, lpszClassName: w!("FakeGoWR"), ..Default::default() };
    RegisterClassExW(&class);
    let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("FakeGoWR"), w!("God of War Ragnarök (stand-in)"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 0, 0, w as i32, h as i32, None, None, inst, None)?;
    let mut dev: Option<ID3D12Device> = None;
    D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &mut dev)?;
    let dev = dev.unwrap();
    let q: ID3D12CommandQueue = dev.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })?;
    let f: IDXGIFactory2 = CreateDXGIFactory1()?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: w, Height: h, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT, BufferCount: 2, SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD, ..Default::default()
    };
    let sc: IDXGISwapChain3 = f.CreateSwapChainForHwnd(&q, hwnd, &desc, None, None)?.cast()?;
    let heap: ID3D12DescriptorHeap = dev.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 2, ..Default::default() })?;
    let step = dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
    let base = heap.GetCPUDescriptorHandleForHeapStart();
    let bufs: Vec<ID3D12Resource> = (0..2).map(|i| sc.GetBuffer(i)).collect::<Result<_, _>>()?;
    for (i, b) in bufs.iter().enumerate() {
        dev.CreateRenderTargetView(b, None, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + i * step });
    }
    let alloc: ID3D12CommandAllocator = dev.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
    let list: ID3D12GraphicsCommandList = dev.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)?;
    list.Close()?;
    let fence: ID3D12Fence = dev.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
    let ev = CreateEventW(None, false, false, None)?;
    let mut cam = Box::new([0f32; 256]);
    let mut cam_pos = Box::new([0f64; 4]);
    println!("fake GoWR: per-view block at {:p} (projection at +0x100); GOWR_PROBE_STEER={}", cam.as_ptr(), steer_env());
    let t0 = Instant::now();
    let mut n = 0u64;
    while t0.elapsed() < run {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        fake_camera(&mut cam_pos, t0.elapsed().as_secs_f32());
        let height = builder(&mut cam, w as f32 / h as f32, n);
        let i = sc.GetCurrentBackBufferIndex() as usize;
        let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + i * step };
        alloc.Reset()?;
        list.Reset(&alloc, None)?;
        list.ResourceBarrier(&[barrier(&bufs[i], D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
        let left = RECT { left: 0, top: 0, right: w as i32 / 2, bottom: h as i32 };
        let right = RECT { left: w as i32 / 2, top: 0, right: w as i32, bottom: h as i32 };
        // the left half turns from red to yellow as the camera the block was built with rises above 20 m
        let green = ((height - 20.0) / 4.0).clamp(0.0, 1.0);
        list.ClearRenderTargetView(rtv, &[1.0, green, 0.0, 1.0], Some(&[left]));
        list.ClearRenderTargetView(rtv, &[0.0, 0.0, 1.0, 1.0], Some(&[right]));
        list.ResourceBarrier(&[barrier(&bufs[i], D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT)]);
        list.Close()?;
        q.ExecuteCommandLists(&[Some(list.cast()?)]);
        sc.Present(1, DXGI_PRESENT(0)).ok()?;
        n += 1;
        q.Signal(&fence, n)?;
        fence.SetEventOnCompletion(n, ev)?;
        WaitForSingleObject(ev, 1000);
    }
    let _ = DestroyWindow(hwnd);
    Ok(n)
}

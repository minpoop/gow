//! Stand-in for GoWR.exe (God of War Ragnarök) to test GowRProbe.exe under Wine: loads d3d12.dll like the game, then
//! idles so the probe can be loaded into it, and exits. With FAKE_GOWR_DRAW=1 it instead opens a window and presents
//! frames through D3D12 (left half red, right half blue), for the probe's frame counting and screenshots, after
//! FAKE_GOWR_DELAY seconds (default 0: under Wine's software D3D12 a device made while another thread draws hangs).
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
    let t0 = Instant::now();
    let mut n = 0u64;
    while t0.elapsed() < run {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let i = sc.GetCurrentBackBufferIndex() as usize;
        let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + i * step };
        alloc.Reset()?;
        list.Reset(&alloc, None)?;
        list.ResourceBarrier(&[barrier(&bufs[i], D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
        let left = RECT { left: 0, top: 0, right: w as i32 / 2, bottom: h as i32 };
        let right = RECT { left: w as i32 / 2, top: 0, right: w as i32, bottom: h as i32 };
        list.ClearRenderTargetView(rtv, &[1.0, 0.0, 0.0, 1.0], Some(&[left]));
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

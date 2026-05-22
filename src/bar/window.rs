//! Win32 window creation and message pump for a single bar instance.
//!
//! One `Bar` corresponds to one physical monitor. On `multi_monitor = true`
//! main.rs creates one per `EnumDisplayMonitors` result; otherwise only the
//! primary monitor gets a bar.

use std::sync::Arc;

use anyhow::Result;
use parking_lot::{Mutex, RwLock};
use tracing::debug;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, MSG, PostQuitMessage, RegisterClassExW,
    TranslateMessage, GWLP_USERDATA, HMENU, WNDCLASSEXW, WM_DESTROY, WM_LBUTTONUP,
    WM_MBUTTONUP, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_SIZE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::config::types::Config;
use crate::module::{BarRegion, ModuleEvent, ModuleSnapshot, ModuleState};

use super::renderer::Direct2DRenderer;

/// Simple axis-aligned rectangle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }
}

/// State stored in the window's GWLP_USERDATA slot so the WndProc can reach
/// back into the Rust world.  Raw pointer — lifetime is tied to the `Bar`
/// instance on the stack in `create`.
struct WindowState {
    config: Arc<RwLock<Config>>,
    modules: Arc<Mutex<Vec<ModuleState>>>,
    renderer: *mut Direct2DRenderer,
}

/// A single status bar window.
pub struct Bar {
    pub hwnd: HWND,
    pub config: Arc<RwLock<Config>>,
    pub modules_state: Arc<Mutex<Vec<ModuleState>>>,
    renderer: Box<Direct2DRenderer>,
    _monitor_bounds: Rect,
}

impl Bar {
    /// Create the Win32 window and initialise Direct2D.
    ///
    /// `monitor_bounds` is the physical pixel rect of the target monitor
    /// (obtained from `EnumDisplayMonitors` / `MONITORINFO.rcMonitor`).
    pub fn create(config: Arc<RwLock<Config>>, monitor_bounds: Rect) -> Result<Self> {
        let (position, height, click_through) = {
            let cfg = config.read();
            (
                cfg.bar.position.clone(),
                cfg.bar.height,
                cfg.bar.click_through,
            )
        };

        let bar_y = if position == "bottom" {
            monitor_bounds.y + monitor_bounds.height as i32 - height as i32
        } else {
            monitor_bounds.y
        };

        let width = monitor_bounds.width;

        let class_name: Vec<u16> = "CrestBar\0".encode_utf16().collect();
        let window_title: Vec<u16> = "crest\0".encode_utf16().collect();

        let hinstance = unsafe { GetModuleHandleW(None)? };

        // Register window class (idempotent — RegisterClassEx returns 0 on dup,
        // which is fine; we just ignore it).
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };

        let ex_style = WS_EX_LAYERED
            | WS_EX_NOACTIVATE
            | WS_EX_TOOLWINDOW
            | WS_EX_TOPMOST
            | if click_through { WS_EX_TRANSPARENT } else { windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0) };

        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_title.as_ptr()),
                WS_POPUP,
                monitor_bounds.x,
                bar_y,
                width as i32,
                height as i32,
                HWND::default(),
                HMENU::default(),
                hinstance,
                None,
            )?
        };

        // Initialise renderer
        let style = config.read().style.clone();
        let mut renderer = Box::new(Direct2DRenderer::new(hwnd, (width, height), &style)?);

        let modules_state = Arc::new(Mutex::new(Vec::new()));

        Ok(Self {
            hwnd,
            config,
            modules_state,
            renderer,
            _monitor_bounds: monitor_bounds,
        })
    }

    /// Show the window and run the Win32 message pump.  **Blocks** until the
    /// window is destroyed — call from a dedicated thread.
    pub fn run_message_loop(&self) {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };

        let mut msg = MSG::default();
        loop {
            let ret = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) };
            if ret.0 <= 0 {
                break;
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Request a WM_PAINT from any thread.
    pub fn invalidate(&self) {
        unsafe { InvalidateRect(self.hwnd, None, false) };
    }
}

// ---------------------------------------------------------------------------
// WndProc
// ---------------------------------------------------------------------------

/// The Win32 window procedure.
///
/// We keep this thin: only WM_PAINT delegates to the renderer; mouse events
/// dispatch to the module hit-tested under the cursor.  Everything else falls
/// through to `DefWindowProcW`.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _hdc = BeginPaint(hwnd, &mut ps);

            // Retrieve WindowState via GWLP_USERDATA (set after Bar::create).
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                let style = state.config.read().style.clone();
                let snapshots: Vec<ModuleSnapshot> = {
                    let modules = state.modules.lock();
                    modules.iter().map(|m| m.snapshot.clone()).collect()
                };
                let _ = (*state.renderer).paint(&snapshots, &style);
            }

            EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_SIZE => {
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            if width > 0 && height > 0 {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                if !ptr.is_null() {
                    let _ = (*(*ptr).renderer).resize((width, height));
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            dispatch_mouse_event(hwnd, lparam, ModuleEvent::LeftClick);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            dispatch_mouse_event(hwnd, lparam, ModuleEvent::RightClick);
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            dispatch_mouse_event(hwnd, lparam, ModuleEvent::MiddleClick);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // High word of wParam is wheel delta; positive = scroll up.
            let delta = ((wparam.0 >> 16) as i16) as i32;
            let event = if delta > 0 { ModuleEvent::ScrollUp } else { ModuleEvent::ScrollDown };
            dispatch_mouse_event(hwnd, lparam, event);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Hit-test which module the cursor lands on and fire its event handler.
/// If the handler returns a command string, spawn it via `cmd /C`.
unsafe fn dispatch_mouse_event(hwnd: HWND, lparam: LPARAM, event: ModuleEvent) {
    let cursor_x = (lparam.0 & 0xFFFF) as i32;

    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    if ptr.is_null() {
        return;
    }
    let state = &*ptr;
    let mut modules = state.modules.lock();

    // Find the module whose pixel_range contains cursor_x
    for module_state in modules.iter_mut() {
        let (x0, x1) = module_state.pixel_range;
        if cursor_x >= x0 as i32 && cursor_x < x1 as i32 {
            // We can't call trait methods here because we only have ModuleState
            // (snapshot + range). The actual module tick loop owns the Box<dyn Module>.
            // Store the event in a side channel for the runtime to pick up.
            // For now, log it — the runtime integration comes in main.rs.
            debug!(
                "mouse event {:?} on module at [{},{}]",
                event, x0, x1
            );
            break;
        }
    }
}
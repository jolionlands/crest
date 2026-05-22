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
    GetWindowLongPtrW, SetWindowLongPtrW, KillTimer, SetTimer,
    MSG, PostQuitMessage, RegisterClassExW,
    TranslateMessage, GWLP_USERDATA, HMENU, WNDCLASSEXW,
    WM_CREATE, WM_DESTROY, WM_LBUTTONUP, WM_MBUTTONUP,
    WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WM_TIMER,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::config::types::Config;
use crate::module::runtime::ModuleRuntime;
use crate::module::{ModuleEvent, ModuleSnapshot};

use super::renderer::Direct2DRenderer;

// ---------------------------------------------------------------------------
// Timer ID
// ---------------------------------------------------------------------------

/// Timer ID for the 250 ms module-tick poll.
const MODULE_TICK_TIMER_ID: usize = 1;

/// Tick interval in milliseconds: 250 ms is granular enough for all built-in
/// modules (fastest at 100 ms) while keeping CPU load negligible.
const MODULE_TICK_INTERVAL_MS: u32 = 250;

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
    runtime: Arc<Mutex<ModuleRuntime>>,
    renderer: *mut Direct2DRenderer,
    bar_width: u32,
}

/// A single status bar window.
pub struct Bar {
    pub hwnd: HWND,
    pub config: Arc<RwLock<Config>>,
    runtime: Arc<Mutex<ModuleRuntime>>,
    renderer: Box<Direct2DRenderer>,
    _monitor_bounds: Rect,
}

impl Bar {
    /// Create the Win32 window and initialise Direct2D.
    ///
    /// `monitor_bounds` is the physical pixel rect of the target monitor
    /// (obtained from `EnumDisplayMonitors` / `MONITORINFO.rcMonitor`).
    ///
    /// `registry` is the fully-populated module registry from `main.rs`.
    pub fn create(
        config: Arc<RwLock<Config>>,
        registry: Arc<crate::module::ModuleRegistry>,
        monitor_bounds: Rect,
    ) -> Result<Self> {
        let (position, height, click_through, modules_config) = {
            let cfg = config.read();
            (
                cfg.bar.position.clone(),
                cfg.bar.height,
                cfg.bar.click_through,
                cfg.modules.clone(),
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
            | if click_through {
                WS_EX_TRANSPARENT
            } else {
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0)
            };

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

        // Build the module runtime from the config + registry.
        let runtime = Arc::new(Mutex::new(
            ModuleRuntime::new(&modules_config, &registry),
        ));

        // Initialise renderer
        let style = config.read().style.clone();
        let renderer = Box::new(Direct2DRenderer::new(hwnd, (width, height), &style)?);

        Ok(Self {
            hwnd,
            config,
            runtime,
            renderer,
            _monitor_bounds: monitor_bounds,
        })
    }

    /// Show the window and run the Win32 message pump.  **Blocks** until the
    /// window is destroyed — call from a dedicated thread.
    pub fn run_message_loop(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };

        // Install WindowState into GWLP_USERDATA so the WndProc can access it.
        // Safety: `state` lives on this stack frame for the duration of the
        // message loop, and the WndProc is only called from GetMessageW below.
        let mut state = WindowState {
            config: Arc::clone(&self.config),
            runtime: Arc::clone(&self.runtime),
            renderer: self.renderer.as_mut() as *mut Direct2DRenderer,
            bar_width: self._monitor_bounds.width,
        };
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                &mut state as *mut WindowState as isize,
            );
        }

        // Kick off the 250 ms module-tick timer.
        unsafe {
            SetTimer(self.hwnd, MODULE_TICK_TIMER_ID, MODULE_TICK_INTERVAL_MS, None);
        }

        // Force an initial paint so the bar isn't blank while waiting for the
        // first timer fire.
        unsafe { InvalidateRect(self.hwnd, None, false) };

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

        // Tear down before returning.
        unsafe {
            KillTimer(self.hwnd, MODULE_TICK_TIMER_ID);
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
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
/// WM_TIMER  → tick all modules; if any changed, invalidate the window.
/// WM_PAINT  → pull fresh snapshots from the runtime and repaint via D2D.
/// Mouse     → dispatch through the module runtime's event handler.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            // Nothing extra — WindowState is installed after CreateWindowExW
            // returns, in run_message_loop.
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == MODULE_TICK_TIMER_ID {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
                if !ptr.is_null() {
                    let state = &*ptr;
                    let changed = state.runtime.lock().tick();
                    if changed {
                        InvalidateRect(hwnd, None, false);
                    }
                }
            }
            LRESULT(0)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _hdc = BeginPaint(hwnd, &mut ps);

            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !ptr.is_null() {
                let state = &*ptr;
                let style = state.config.read().style.clone();
                let snapshots: Vec<ModuleSnapshot> = state.runtime.lock().snapshots();
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
                    let state = &mut *ptr;
                    state.bar_width = width;
                    let _ = (*state.renderer).resize((width, height));
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            dispatch_mouse_event(hwnd, lparam, wparam, ModuleEvent::LeftClick);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            dispatch_mouse_event(hwnd, lparam, wparam, ModuleEvent::RightClick);
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            dispatch_mouse_event(hwnd, lparam, wparam, ModuleEvent::MiddleClick);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // High word of wParam is wheel delta; positive = scroll up.
            let delta = ((wparam.0 >> 16) as i16) as i32;
            let event = if delta > 0 {
                ModuleEvent::ScrollUp
            } else {
                ModuleEvent::ScrollDown
            };
            dispatch_mouse_event(hwnd, lparam, wparam, event);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Dispatch a mouse/scroll event through the module runtime.
///
/// If the runtime returns a command string, spawn it via `cmd /C`.
unsafe fn dispatch_mouse_event(
    hwnd: HWND,
    lparam: LPARAM,
    _wparam: WPARAM,
    event: ModuleEvent,
) {
    let cursor_x = (lparam.0 & 0xFFFF) as i32;

    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    if ptr.is_null() {
        return;
    }
    let state = &*ptr;
    let bar_w = state.bar_width;

    let cmd = state.runtime.lock().dispatch_event(cursor_x, bar_w, event);

    if let Some(cmd_str) = cmd {
        debug!("spawning on-event command: {}", cmd_str);
        let _ = std::process::Command::new("cmd")
            .args(["/C", &cmd_str])
            .spawn();
    }
}

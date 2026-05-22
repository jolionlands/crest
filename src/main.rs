//! crest main entry-point.
//!
//! Startup sequence:
//!   1. Parse CLI args (clap)
//!   2. Handle --register-autostart / --unregister-autostart and exit early
//!   3. Initialise tracing
//!   4. CoInitializeEx (STA — required for Direct2D + COM on the UI thread)
//!   5. Load config, write default on first run
//!   6. Spawn the wiri IPC listener thread
//!   7. Spawn the aurora IPC listener thread
//!   8. Create bar window(s) — primary monitor only unless multi_monitor=true
//!   9. Wait for Ctrl+C, then clean up

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use parking_lot::RwLock;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

use crest::aurora_ipc::AuroraClient;
use crest::bar::window::{Bar, Rect};
use crest::config;
use crest::hooks::StartupManager;
use crest::module::builtins;
use crest::module::ModuleRegistry;
use crest::wiri_ipc::WiriClient;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "crest", version, about = "Modular status bar for Windows")]
struct Args {
    /// Path to the KDL config file. If unset, search %APPDATA%\crest\config.kdl.
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
    #[arg(short, long)]
    verbose: bool,
    /// Register crest in the Windows Run key so it starts with Windows
    #[arg(long)]
    register_autostart: bool,
    /// Remove crest from the Windows Run key
    #[arg(long)]
    unregister_autostart: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args = Args::parse();

    // 1. Handle autostart flags BEFORE config load + bar creation
    if args.register_autostart {
        // Tracing needs to be up for the log output to appear
        init_tracing(args.verbose);
        let mgr = StartupManager::new();
        match mgr.register() {
            Ok(()) => info!("crest: registered in Windows Run key (autostart enabled)"),
            Err(e) => tracing::error!("crest: failed to register autostart: {e}"),
        }
        return Ok(());
    }

    if args.unregister_autostart {
        init_tracing(args.verbose);
        let mgr = StartupManager::new();
        match mgr.unregister() {
            Ok(()) => info!("crest: removed from Windows Run key (autostart disabled)"),
            Err(e) => tracing::error!("crest: failed to unregister autostart: {e}"),
        }
        return Ok(());
    }

    // 2. Tracing (normal startup)
    init_tracing(args.verbose);

    info!("crest starting");

    // Log autostart registration status at normal startup
    {
        let mgr = StartupManager::new();
        if mgr.is_registered() {
            info!("crest: autostart is registered (Run key present)");
        } else {
            info!("crest: autostart not registered (use --register-autostart to enable)");
        }
    }

    // 3. COM STA (required for Direct2D HWND render targets)
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    // 4. Config
    let config = config::load_config()?;
    let multi_monitor = config.bar.multi_monitor;
    let config = Arc::new(RwLock::new(config));

    // 5. wiri IPC
    let wiri = WiriClient::new();
    let wiri_state = wiri.state();
    std::thread::spawn(move || {
        wiri.connect_and_listen();
    });

    // 6. aurora IPC
    let aurora = AuroraClient::new();
    let aurora_state = aurora.state();
    std::thread::spawn(move || {
        aurora.connect_and_listen();
    });

    // 7. Module registry — populated now; module tick loop wiring comes next round.
    let mut registry = ModuleRegistry::new();
    builtins::register_all(&mut registry, Arc::clone(&wiri_state), Arc::clone(&aurora_state));
    let registry = Arc::new(registry); // shared across bar threads

    // Enumerate monitors
    let monitors = enumerate_monitors();
    let target_monitors: Vec<Rect> = if multi_monitor {
        monitors
    } else {
        // Primary monitor only — the one containing (0,0)
        monitors
            .into_iter()
            .find(|r| r.x <= 0 && r.y <= 0)
            .map(|r| vec![r])
            .unwrap_or_else(|| vec![Rect::new(0, 0, 1920, 32)])
    };

    // Spawn a thread per bar
    let mut handles = Vec::new();
    for bounds in target_monitors {
        let cfg = Arc::clone(&config);
        let reg = Arc::clone(&registry);

        let handle = std::thread::spawn(move || {
            match Bar::create(cfg, reg, bounds) {
                Ok(mut bar) => bar.run_message_loop(),
                Err(e) => tracing::error!("failed to create bar: {e}"),
            }
        });
        handles.push(handle);
    }

    // 8. Wait for Ctrl+C
    ctrlc_wait();

    info!("crest exiting");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn init_tracing(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::from_default_env()
    };
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .init();
}

/// Block until the bar threads finish (or the process is killed).
fn ctrlc_wait() {
    // Park the main thread; bar threads run their own message loops.
    // On Ctrl+C the OS delivers SIGINT which terminates the process.
    loop {
        std::thread::park();
    }
}

// ---------------------------------------------------------------------------
// Monitor enumeration
// ---------------------------------------------------------------------------

/// Collect the physical pixel bounds of every connected monitor.
fn enumerate_monitors() -> Vec<Rect> {
    let mut rects: Vec<Rect> = Vec::new();
    let rects_ptr = &mut rects as *mut Vec<Rect> as isize;

    unsafe {
        EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(monitor_enum_proc),
            LPARAM(rects_ptr),
        );
    }

    if rects.is_empty() {
        // Fallback: assume a single 1920×1080 monitor at origin
        rects.push(Rect::new(0, 0, 1920, 1080));
    }
    rects
}

unsafe extern "system" fn monitor_enum_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmon, &mut info).as_bool() {
        let r = &info.rcMonitor;
        let rect = Rect::new(
            r.left,
            r.top,
            (r.right - r.left) as u32,
            (r.bottom - r.top) as u32,
        );
        let rects = &mut *(lparam.0 as *mut Vec<Rect>);
        rects.push(rect);
    }
    BOOL(1) // continue enumeration
}

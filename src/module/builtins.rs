//! Built-in module registration.
//!
//! Call [`register_all`] at startup to populate the [`ModuleRegistry`] with
//! every built-in module kind.  The registry will then build the correct
//! concrete type when [`ModuleRegistry::build`] is called.

use std::sync::{Arc, RwLock};

use super::{
    ModuleRegistry,
    WiriState,
    battery::BatteryModule,
    clock::ClockModule,
    cpu::CpuModule,
    custom::CustomModule,
    focused_window::FocusedWindowModule,
    memory::MemoryModule,
    network::NetworkModule,
    tray::TrayModule,
    volume::VolumeModule,
    workspaces::WorkspacesModule,
};

/// Register all built-in modules into `registry`.
///
/// `wiri_state` is shared with the modules that display window-manager state
/// (workspaces, focused-window).
pub fn register_all(registry: &mut ModuleRegistry, wiri_state: Arc<RwLock<WiriState>>) {
    // clock
    registry.register("clock", Box::new(|entry| Box::new(ClockModule::new(entry))));

    // battery
    registry.register("battery", Box::new(|entry| Box::new(BatteryModule::new(entry))));

    // cpu
    registry.register("cpu", Box::new(|entry| Box::new(CpuModule::new(entry))));

    // memory
    registry.register("memory", Box::new(|entry| Box::new(MemoryModule::new(entry))));

    // network
    registry.register("network", Box::new(|entry| Box::new(NetworkModule::new(entry))));

    // volume
    registry.register("volume", Box::new(|entry| Box::new(VolumeModule::new(entry))));

    // tray
    registry.register("tray", Box::new(|entry| Box::new(TrayModule::new(entry))));

    // custom — runs an arbitrary shell command
    registry.register("custom", Box::new(|entry| Box::new(CustomModule::new(entry))));

    // workspaces — wiri-state-driven
    {
        let state = Arc::clone(&wiri_state);
        registry.register(
            "workspaces",
            Box::new(move |entry| Box::new(WorkspacesModule::new(entry, Arc::clone(&state)))),
        );
    }

    // focused-window — wiri-state-driven
    {
        let state = Arc::clone(&wiri_state);
        registry.register(
            "focused-window",
            Box::new(move |entry| Box::new(FocusedWindowModule::new(entry, Arc::clone(&state)))),
        );
    }
}
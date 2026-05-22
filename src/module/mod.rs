use std::time::Duration;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

pub mod workspaces;
pub mod focused_window;
pub mod clock;
pub mod cpu;
pub mod memory;
pub mod battery;
pub mod network;
pub mod volume;
pub mod tray;
pub mod custom;
pub mod aurora_wallpaper;
pub mod builtins;
pub mod runtime;

// ---------------------------------------------------------------------------
// WiriState — shared IPC state from the window manager.
// ---------------------------------------------------------------------------
#[derive(Debug, Default, Clone)]
pub struct WiriState {
    /// Ordered workspace names, e.g. ["1", "2", "3", "4", "5"]
    pub workspaces: Vec<String>,
    /// Index (0-based) of the currently focused workspace.
    pub focused_workspace: usize,
    /// HWND of the currently focused window (as reported by wiri).
    pub focused_window_hwnd: Option<isize>,
    /// Title of the currently focused window.
    pub focused_window_title: String,
}

// ---------------------------------------------------------------------------
// BarRegion — which third of the bar a module belongs to.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarRegion {
    #[default]
    Left,
    Center,
    Right,
}

// ---------------------------------------------------------------------------
// ModuleSnapshot — immutable render state emitted by a module.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Default)]
pub struct ModuleSnapshot {
    pub text: String,
    pub fg: Option<String>,
    pub icon: Option<char>,
    pub region: BarRegion,
}

// ---------------------------------------------------------------------------
// ModuleEvent — input events forwarded from the bar renderer.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleEvent {
    LeftClick,
    RightClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
}

// ---------------------------------------------------------------------------
// Module trait
// TODO(audit): foundation agent may alter method signatures; add compat shims
// if needed rather than changing the individual module impls.
// ---------------------------------------------------------------------------
pub trait Module: Send + Sync {
    fn kind(&self) -> &'static str;
    fn initial(&self) -> ModuleSnapshot;
    fn tick(&mut self) -> ModuleSnapshot;
    fn on_event(&mut self, _event: ModuleEvent) -> Option<String> {
        None
    }
    fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}

// ---------------------------------------------------------------------------
// ModuleRegistry — maps kind strings to factory closures.
// ---------------------------------------------------------------------------
pub type ModuleFactory = Box<dyn Fn(&crate::config::types::ModuleEntry) -> Box<dyn Module> + Send + Sync>;

#[derive(Default)]
pub struct ModuleRegistry {
    factories: HashMap<String, ModuleFactory>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, kind: &str, factory: ModuleFactory) {
        self.factories.insert(kind.to_string(), factory);
    }

    pub fn build(&self, entry: &crate::config::types::ModuleEntry) -> Option<Box<dyn Module>> {
        self.factories.get(&entry.kind).map(|f| f(entry))
    }
}

// ---------------------------------------------------------------------------
// ModuleState — runtime state per module slot
// ---------------------------------------------------------------------------

/// Tracks the live render state and pixel position of a single module slot.
#[derive(Debug, Clone)]
pub struct ModuleState {
    pub snapshot: ModuleSnapshot,
    /// Pixel region occupied by this module (x_start, x_end) in bar coords.
    pub pixel_range: (u32, u32),
}

impl ModuleState {
    pub fn new(snapshot: ModuleSnapshot) -> Self {
        Self {
            snapshot,
            pixel_range: (0, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ModuleEntry;

    struct StaticModule {
        text: String,
    }

    impl Module for StaticModule {
        fn kind(&self) -> &'static str { "static" }
        fn initial(&self) -> ModuleSnapshot {
            ModuleSnapshot {
                text: self.text.clone(),
                fg: None,
                icon: None,
                region: BarRegion::Left,
            }
        }
        fn tick(&mut self) -> ModuleSnapshot { self.initial() }
    }

    #[test]
    fn test_module_trait_dyn() {
        let mut m: Box<dyn Module> = Box::new(StaticModule { text: "hello".to_string() });
        let snap = m.initial();
        assert_eq!(snap.text, "hello");
        assert_eq!(snap.region, BarRegion::Left);
        let tick_snap = m.tick();
        assert_eq!(tick_snap.text, "hello");
        // Default on_event returns None
        assert!(m.on_event(ModuleEvent::LeftClick).is_none());
        // Default interval is 1 second
        assert_eq!(m.interval(), Duration::from_secs(1));
    }

    #[test]
    fn test_registry_fallback_on_unknown_kind() {
        let registry = ModuleRegistry::new();
        let entry = ModuleEntry {
            kind: "nonexistent-module-kind".to_string(),
            ..Default::default()
        };
        // build returns None for unknown kinds; callers fall back to echo
        assert!(registry.build(&entry).is_none());
    }
}

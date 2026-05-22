// Stub: tray module — placeholder for systray icon integration.
use std::time::Duration;

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

pub struct TrayModule {
    region: BarRegion,
}

impl TrayModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("left") => BarRegion::Left,
            Some("center") => BarRegion::Center,
            _ => BarRegion::Right,
        };
        Self { region }
    }
}

impl Module for TrayModule {
    fn kind(&self) -> &'static str {
        "tray"
    }

    fn initial(&self) -> ModuleSnapshot {
        ModuleSnapshot {
            text: String::new(),
            fg: None,
            icon: None,
            region: self.region,
        }
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.initial()
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(5)
    }
}
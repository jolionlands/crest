// Stub: volume module — placeholder for future implementation.
// Will use Win32 Core Audio API (IMMDeviceEnumerator / IAudioEndpointVolume).
use std::time::Duration;

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

pub struct VolumeModule {
    region: BarRegion,
}

impl VolumeModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("left") => BarRegion::Left,
            Some("center") => BarRegion::Center,
            _ => BarRegion::Right,
        };
        Self { region }
    }
}

impl Module for VolumeModule {
    fn kind(&self) -> &'static str {
        "volume"
    }

    fn initial(&self) -> ModuleSnapshot {
        ModuleSnapshot {
            text: "VOL".to_string(),
            fg: None,
            icon: None,
            region: self.region,
        }
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.initial()
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}
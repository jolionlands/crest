//! Status-bar module that shows the currently-displayed wallpaper's filename
//! (or a short path) on the primary monitor.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;

use crate::aurora_ipc::AuroraState;
use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

pub struct AuroraWallpaperModule {
    state: Arc<RwLock<AuroraState>>,
    region: BarRegion,
    max_length: usize,
}

impl AuroraWallpaperModule {
    pub fn new(entry: &ModuleEntry, state: Arc<RwLock<AuroraState>>) -> Self {
        let max_length: usize = entry.extra.get("max-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(40);
        Self { state, region: BarRegion::Right, max_length }
    }

    fn snapshot_text(&self) -> String {
        let s = self.state.read();
        // Show the first monitor's filename for simplicity (TODO: per-monitor)
        if let Some((_, path)) = s.current_wallpapers.iter().next() {
            let file = Path::new(path).file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(path);
            if file.len() > self.max_length {
                let mut t = file.chars().take(self.max_length).collect::<String>();
                t.push('…');
                t
            } else {
                file.to_string()
            }
        } else {
            String::from("(no wallpaper)")
        }
    }
}

impl Module for AuroraWallpaperModule {
    fn kind(&self) -> &'static str { "aurora-wallpaper" }
    fn initial(&self) -> ModuleSnapshot {
        ModuleSnapshot { text: self.snapshot_text(), fg: None, icon: None, region: self.region }
    }
    fn tick(&mut self) -> ModuleSnapshot {
        ModuleSnapshot { text: self.snapshot_text(), fg: None, icon: None, region: self.region }
    }
    fn interval(&self) -> Duration { Duration::from_secs(2) }
}

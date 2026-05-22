//! Status-bar module that shows the currently-displayed wallpaper's filename
//! (or a short path) on the primary monitor.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;

use crate::aurora_ipc::AuroraState;
use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleEvent, ModuleSnapshot};

// Re-export for tests
#[cfg(test)]
use std::collections::HashMap;

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

    fn on_event(&mut self, event: ModuleEvent) -> Option<String> {
        match event {
            ModuleEvent::LeftClick => {
                // Retrieve the path of the first (primary-monitor) wallpaper.
                let path = self.state.read()
                    .current_wallpapers
                    .values()
                    .next()
                    .cloned()?;
                // `start "" "<path>"` opens the file in its default app.
                // The empty string after `start` is required so Windows doesn't
                // interpret the quoted path as the window title.
                Some(format!("start \"\" \"{}\"", path))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module_with_path(path: &str) -> AuroraWallpaperModule {
        let state = Arc::new(RwLock::new(AuroraState {
            current_wallpapers: {
                let mut m = HashMap::new();
                m.insert("primary".to_string(), path.to_string());
                m
            },
        }));
        let entry = ModuleEntry::default();
        AuroraWallpaperModule::new(&entry, state)
    }

    #[test]
    fn test_aurora_wallpaper_left_click_returns_start_command() {
        let path = r"C:\Users\kalli\Pictures\wallpaper.jpg";
        let mut module = make_module_with_path(path);
        let result = module.on_event(ModuleEvent::LeftClick);
        let cmd = result.expect("should return a command on LeftClick");
        // Must contain the path so the shell can open it.
        assert!(cmd.contains(path), "command should contain the wallpaper path: {cmd}");
        // Must use `start ""` so cmd /C opens in the default app.
        assert!(cmd.starts_with("start \"\""), "command should start with start \"\": {cmd}");
    }

    #[test]
    fn test_aurora_wallpaper_left_click_no_wallpaper_returns_none() {
        let state = Arc::new(RwLock::new(AuroraState {
            current_wallpapers: HashMap::new(),
        }));
        let entry = ModuleEntry::default();
        let mut module = AuroraWallpaperModule::new(&entry, state);
        let result = module.on_event(ModuleEvent::LeftClick);
        assert!(result.is_none(), "should return None when no wallpaper is set");
    }

    #[test]
    fn test_aurora_wallpaper_right_click_returns_none() {
        let mut module = make_module_with_path(r"C:\some\file.jpg");
        assert!(module.on_event(ModuleEvent::RightClick).is_none());
    }

    #[test]
    fn test_aurora_wallpaper_tick_shows_filename() {
        let path = r"C:\Pictures\cool_bg.png";
        let mut module = make_module_with_path(path);
        let snap = module.tick();
        assert_eq!(snap.text, "cool_bg.png");
    }
}

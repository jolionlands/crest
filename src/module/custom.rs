// Custom module — runs an arbitrary shell command and displays stdout.
// If the command emits JSON {"text":"...","fg":"..."} those fields are used.
use std::time::Duration;
use std::process::Command;

use serde::Deserialize;

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

#[derive(Deserialize)]
struct JsonOut {
    text: Option<String>,
    fg: Option<String>,
}

pub struct CustomModule {
    command: String,
    region: BarRegion,
    interval_ms: u64,
}

impl CustomModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let command = entry
            .extra
            .get("command")
            .cloned()
            .unwrap_or_else(|| entry.format.clone());
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("left") => BarRegion::Left,
            Some("center") => BarRegion::Center,
            _ => BarRegion::Right,
        };
        Self {
            command,
            region,
            interval_ms: entry.interval_ms.max(100),
        }
    }

    fn run_command(&self) -> ModuleSnapshot {
        if self.command.is_empty() {
            return ModuleSnapshot::default();
        }
        let raw = Command::new("cmd")
            .args(["/C", &self.command])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // Try JSON {"text": "...", "fg": "..."} first.
        if let Ok(j) = serde_json::from_str::<JsonOut>(&raw) {
            return ModuleSnapshot {
                text: j.text.unwrap_or_default(),
                fg: j.fg,
                icon: None,
                region: self.region,
            };
        }
        ModuleSnapshot {
            text: raw,
            fg: None,
            icon: None,
            region: self.region,
        }
    }
}

impl Module for CustomModule {
    fn kind(&self) -> &'static str {
        "custom"
    }

    fn initial(&self) -> ModuleSnapshot {
        ModuleSnapshot::default()
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.run_command()
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }
}
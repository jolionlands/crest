use std::time::Duration;

use chrono::Local;

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

const DEFAULT_FORMAT: &str = "%H:%M";

pub struct ClockModule {
    format: String,
    region: BarRegion,
}

impl ClockModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let format = if !entry.format.is_empty() {
            entry.format.clone()
        } else {
            entry
                .extra
                .get("format")
                .cloned()
                .unwrap_or_else(|| DEFAULT_FORMAT.to_string())
        };
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("left") => BarRegion::Left,
            Some("center") => BarRegion::Center,
            _ => BarRegion::Right,
        };
        Self { format, region }
    }

    fn render(&self) -> String {
        Local::now().format(&self.format).to_string()
    }

    fn snapshot(&self) -> ModuleSnapshot {
        ModuleSnapshot {
            text: self.render(),
            fg: None,
            icon: None,
            region: self.region,
        }
    }
}

impl Module for ClockModule {
    fn kind(&self) -> &'static str {
        "clock"
    }

    fn initial(&self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.snapshot()
    }

    /// 1 second when format includes `%S` (seconds), otherwise 30 seconds.
    fn interval(&self) -> Duration {
        if self.format.contains("%S") || self.format.contains("%T") {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(30)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, FixedOffset};

    #[test]
    fn test_clock_format_hm() {
        // Use a fixed time to verify formatting.
        // chrono's format is deterministic; we simulate by formatting a known DateTime.
        let offset = FixedOffset::east_opt(0).unwrap();
        let dt = offset.with_ymd_and_hms(2024, 6, 1, 14, 35, 0).unwrap();
        let formatted = dt.format("%H:%M").to_string();
        assert_eq!(formatted, "14:35");
    }

    #[test]
    fn test_clock_interval_without_seconds() {
        let entry = ModuleEntry {
            format: "%H:%M".to_string(),
            ..Default::default()
        };
        let m = ClockModule::new(&entry);
        assert_eq!(m.interval(), Duration::from_secs(30));
    }

    #[test]
    fn test_clock_interval_with_seconds() {
        let entry = ModuleEntry {
            format: "%H:%M:%S".to_string(),
            ..Default::default()
        };
        let m = ClockModule::new(&entry);
        assert_eq!(m.interval(), Duration::from_secs(1));
    }

    #[test]
    fn test_clock_snapshot_nonempty() {
        let entry = ModuleEntry::default();
        let m = ClockModule::new(&entry);
        let snap = m.initial();
        // Should produce something like "14:35"
        assert!(!snap.text.is_empty());
    }
}

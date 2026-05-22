use std::time::Duration;

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

const GB: u64 = 1024 * 1024 * 1024;

/// Format bytes as e.g. "10.2 GB".
pub fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / GB as f64
}

/// Build the display string from used/total bytes.
pub fn format_memory(used_bytes: u64, total_bytes: u64) -> String {
    format!("{:.1}/{:.1} GB", bytes_to_gb(used_bytes), bytes_to_gb(total_bytes))
}

fn query_memory() -> Option<(u64, u64)> {
    let mut ms = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        GlobalMemoryStatusEx(&mut ms).ok()?;
    }
    let total = ms.ullTotalPhys;
    let avail = ms.ullAvailPhys;
    let used = total.saturating_sub(avail);
    Some((used, total))
}

pub struct MemoryModule {
    region: BarRegion,
}

impl MemoryModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("center") => BarRegion::Center,
            Some("left") => BarRegion::Left,
            _ => BarRegion::Right,
        };
        Self { region }
    }

    fn snapshot(&self) -> ModuleSnapshot {
        let text = match query_memory() {
            Some((used, total)) => format_memory(used, total),
            None => "MEM ?".to_string(),
        };
        ModuleSnapshot {
            text,
            fg: None,
            icon: None,
            region: self.region,
        }
    }
}

impl Module for MemoryModule {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn initial(&self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_format() {
        // 10.2 GB used, 15.6 GB total
        let used = (10.2_f64 * GB as f64) as u64;
        let total = (15.6_f64 * GB as f64) as u64;
        let s = format_memory(used, total);
        assert_eq!(s, "10.2/15.6 GB");
    }

    #[test]
    fn test_memory_format_round() {
        let used = GB * 4;
        let total = GB * 16;
        let s = format_memory(used, total);
        assert_eq!(s, "4.0/16.0 GB");
    }

    #[test]
    fn test_bytes_to_gb() {
        assert!((bytes_to_gb(GB) - 1.0).abs() < 0.001);
        assert!((bytes_to_gb(GB / 2) - 0.5).abs() < 0.001);
    }
}

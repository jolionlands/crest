use std::time::{Duration, Instant};

use windows::Win32::NetworkManagement::IpHelper::{
    GetIfTable, MIB_IFTABLE, MIB_IFROW,
};

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

/// Format byte-rate as human-readable with SI prefix.
pub fn format_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 1_000.0 {
        format!("{:.0} kB/s", bytes_per_sec / 1_000.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// Sum rx/tx bytes across all up non-loopback interfaces using GetIfTable.
/// Returns (total_rx_bytes, total_tx_bytes) or None on error.
fn sample_bytes() -> Option<(u64, u64)> {
    // First call with null to get required size.
    let mut size: u32 = 0;
    unsafe {
        // ERROR_INSUFFICIENT_BUFFER = 122; ignore initial error.
        let _ = GetIfTable(None, &mut size, false);
    }
    if size == 0 {
        return None;
    }

    // Allocate a byte buffer large enough.
    let mut buf = vec![0u8; size as usize];
    let table_ptr = buf.as_mut_ptr() as *mut MIB_IFTABLE;
    unsafe {
        let result = GetIfTable(Some(table_ptr), &mut size, false);
        if result != 0 { return None; }
        let table = &*table_ptr;
        let rows: &[MIB_IFROW] =
            std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        let (rx, tx) = rows.iter().fold((0u64, 0u64), |(r, t), row| {
            // IF_TYPE_SOFTWARE_LOOPBACK = 24
            if row.dwType == 24 {
                (r, t)
            } else {
                (r + row.dwInOctets as u64, t + row.dwOutOctets as u64)
            }
        });
        Some((rx, tx))
    }
}

pub struct NetworkModule {
    prev_rx: u64,
    prev_tx: u64,
    prev_time: Instant,
    last_text: String,
    region: BarRegion,
}

impl NetworkModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("center") => BarRegion::Center,
            Some("left") => BarRegion::Left,
            _ => BarRegion::Right,
        };
        let (prev_rx, prev_tx) = sample_bytes().unwrap_or((0, 0));
        Self {
            prev_rx,
            prev_tx,
            prev_time: Instant::now(),
            last_text: "no network".to_string(),
            region,
        }
    }

    fn snapshot(&mut self) -> ModuleSnapshot {
        let now = Instant::now();
        let elapsed = now.duration_since(self.prev_time).as_secs_f64();

        let text = if let Some((rx, tx)) = sample_bytes() {
            if elapsed > 0.0 {
                let rx_rate = (rx.saturating_sub(self.prev_rx)) as f64 / elapsed;
                let tx_rate = (tx.saturating_sub(self.prev_tx)) as f64 / elapsed;
                self.prev_rx = rx;
                self.prev_tx = tx;
                self.prev_time = now;
                format!("↓ {} ↑ {}", format_rate(rx_rate), format_rate(tx_rate))
            } else {
                self.last_text.clone()
            }
        } else {
            "no network".to_string()
        };

        self.last_text = text.clone();
        ModuleSnapshot {
            text,
            fg: None,
            icon: None,
            region: self.region,
        }
    }
}

impl Module for NetworkModule {
    fn kind(&self) -> &'static str {
        "network"
    }

    fn initial(&self) -> ModuleSnapshot {
        ModuleSnapshot {
            text: self.last_text.clone(),
            fg: None,
            icon: None,
            region: self.region,
        }
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_rate_bytes() {
        assert_eq!(format_rate(512.0), "512 B/s");
    }

    #[test]
    fn test_format_rate_kb() {
        assert_eq!(format_rate(2048.0), "2 kB/s");
    }

    #[test]
    fn test_format_rate_mb() {
        assert_eq!(format_rate(1_200_000.0), "1.2 MB/s");
    }

    #[test]
    fn test_network_display_format() {
        // Verify the combined format string shape.
        let rx = format_rate(1_200_000.0);
        let tx = format_rate(256_000.0);
        let line = format!("↓ {} ↑ {}", rx, tx);
        assert_eq!(line, "↓ 1.2 MB/s ↑ 256 kB/s");
    }
}

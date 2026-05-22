use std::time::Duration;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::GetSystemTimes;

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

fn filetime_to_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// Sample idle/kernel/user times from the OS.
fn sample() -> (u64, u64, u64) {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        // GetSystemTimes: kernel time includes idle time.
        let _ = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
    }
    (
        filetime_to_u64(&idle),
        filetime_to_u64(&kernel),
        filetime_to_u64(&user),
    )
}

/// Compute CPU usage percent given two consecutive samples.
/// Returns a value in [0.0, 100.0].
pub fn cpu_percent(
    prev: (u64, u64, u64),
    curr: (u64, u64, u64),
) -> f64 {
    let (pi, pk, pu) = prev;
    let (ci, ck, cu) = curr;

    let d_idle = ci.saturating_sub(pi);
    let d_kernel = ck.saturating_sub(pk);
    let d_user = cu.saturating_sub(pu);

    // Kernel includes idle; total CPU time = kernel + user.
    let total = d_kernel + d_user;
    if total == 0 {
        return 0.0;
    }
    let busy = total.saturating_sub(d_idle);
    (busy as f64 / total as f64) * 100.0
}

pub struct CpuModule {
    prev: (u64, u64, u64),
    last_pct: f64,
    region: BarRegion,
}

impl CpuModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("center") => BarRegion::Center,
            Some("left") => BarRegion::Left,
            _ => BarRegion::Right,
        };
        Self {
            prev: sample(),
            last_pct: 0.0,
            region,
        }
    }

    fn snapshot(&self) -> ModuleSnapshot {
        ModuleSnapshot {
            text: format!("CPU {:.0}%", self.last_pct),
            fg: color_for_pct(self.last_pct),
            icon: None,
            region: self.region,
        }
    }
}

fn color_for_pct(pct: f64) -> Option<String> {
    if pct >= 90.0 {
        Some("#ff5555".to_string())
    } else if pct >= 70.0 {
        Some("#ffb86c".to_string())
    } else {
        None
    }
}

impl Module for CpuModule {
    fn kind(&self) -> &'static str {
        "cpu"
    }

    fn initial(&self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn tick(&mut self) -> ModuleSnapshot {
        let curr = sample();
        self.last_pct = cpu_percent(self.prev, curr);
        self.prev = curr;
        self.snapshot()
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_delta_idle() {
        // All time is idle → 0% usage.
        let prev = (0, 100, 0);
        let curr = (100, 200, 0); // idle += 100, kernel += 100, user unchanged
        assert_eq!(cpu_percent(prev, curr), 0.0);
    }

    #[test]
    fn test_cpu_delta_half() {
        // 50% busy: kernel+user=200, idle=100, busy=100.
        let prev = (0, 0, 0);
        let curr = (100, 150, 50); // idle=100, kernel=150 (incl idle), user=50
        // total = 150+50 = 200, busy = 200-100 = 100 → 50%
        let pct = cpu_percent(prev, curr);
        assert!((pct - 50.0).abs() < 0.01, "expected 50%, got {}", pct);
    }

    #[test]
    fn test_cpu_delta_full() {
        // 100% busy: no idle time.
        let prev = (0, 0, 0);
        let curr = (0, 100, 50);
        // total=150, busy=150 → 100%
        let pct = cpu_percent(prev, curr);
        assert!((pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_cpu_delta_zero_total() {
        assert_eq!(cpu_percent((5, 5, 5), (5, 5, 5)), 0.0);
    }
}

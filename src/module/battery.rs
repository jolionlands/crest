use std::time::Duration;

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleSnapshot};

const DEFAULT_WARNING: u8 = 20;
const DEFAULT_CRITICAL: u8 = 10;
const COLOR_NORMAL: &str = "#ffffff";
const COLOR_WARNING: &str = "#ffb86c";
const COLOR_CRITICAL: &str = "#ff5555";

// AC line status constants from Win32.
const AC_ONLINE: u8 = 1;
const BATTERY_FLAG_NO_BATTERY: u8 = 128;

pub struct BatteryModule {
    warning: u8,
    critical: u8,
    region: BarRegion,
}

impl BatteryModule {
    pub fn new(entry: &ModuleEntry) -> Self {
        let warning = entry
            .extra
            .get("warning")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_WARNING);
        let critical = entry
            .extra
            .get("critical")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_CRITICAL);
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("center") => BarRegion::Center,
            Some("left") => BarRegion::Left,
            _ => BarRegion::Right,
        };
        Self { warning, critical, region }
    }

    /// Compute display from a SYSTEM_POWER_STATUS.
    pub fn render(
        &self,
        flags: u8,
        ac_status: u8,
        percent: u8,
    ) -> ModuleSnapshot {
        if flags & BATTERY_FLAG_NO_BATTERY != 0 {
            return ModuleSnapshot {
                text: "AC".to_string(),
                fg: None,
                icon: None,
                region: self.region,
            };
        }

        let charging = ac_status == AC_ONLINE;
        let icon = if charging { '⚡' } else { '🔋' };
        let text = format!("{} {}%", icon, percent);
        let fg = self.color_for(percent);

        ModuleSnapshot {
            text,
            fg,
            icon: Some(icon),
            region: self.region,
        }
    }

    pub fn color_for(&self, percent: u8) -> Option<String> {
        if percent <= self.critical {
            Some(COLOR_CRITICAL.to_string())
        } else if percent <= self.warning {
            Some(COLOR_WARNING.to_string())
        } else {
            None
        }
    }

    fn snapshot(&self) -> ModuleSnapshot {
        let mut ps = SYSTEM_POWER_STATUS::default();
        unsafe {
            let _ = GetSystemPowerStatus(&mut ps);
        }
        self.render(ps.BatteryFlag, ps.ACLineStatus, ps.BatteryLifePercent)
    }
}

impl Module for BatteryModule {
    fn kind(&self) -> &'static str {
        "battery"
    }

    fn initial(&self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module() -> BatteryModule {
        BatteryModule {
            warning: DEFAULT_WARNING,
            critical: DEFAULT_CRITICAL,
            region: BarRegion::Right,
        }
    }

    #[test]
    fn test_battery_low_critical() {
        let m = make_module();
        let snap = m.render(0, 0, 5); // discharging, 5% (below critical=10)
        assert_eq!(snap.fg, Some("#ff5555".to_string()));
        assert!(snap.text.contains("5%"));
        assert!(snap.text.contains("🔋"));
    }

    #[test]
    fn test_battery_warning() {
        let m = make_module();
        let snap = m.render(0, 0, 15); // discharging, 15% (below warning=20)
        assert_eq!(snap.fg, Some("#ffb86c".to_string()));
    }

    #[test]
    fn test_battery_charging() {
        let m = make_module();
        let snap = m.render(0, AC_ONLINE, 78);
        assert!(snap.text.contains('⚡'));
        assert!(snap.text.contains("78%"));
        assert_eq!(snap.fg, None);
    }

    #[test]
    fn test_battery_no_battery() {
        let m = make_module();
        let snap = m.render(BATTERY_FLAG_NO_BATTERY, AC_ONLINE, 255);
        assert_eq!(snap.text, "AC");
    }

    #[test]
    fn test_battery_full_ok() {
        let m = make_module();
        let snap = m.render(0, 0, 80);
        assert_eq!(snap.fg, None);
    }
}

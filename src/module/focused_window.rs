use std::time::Duration;
use std::sync::{Arc, RwLock};

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleEvent, ModuleSnapshot, WiriState};

const DEFAULT_MAX_LENGTH: usize = 60;
const ELLIPSIS: char = '…';

pub struct FocusedWindowModule {
    state: Arc<RwLock<WiriState>>,
    max_length: usize,
    region: BarRegion,
}

impl FocusedWindowModule {
    pub fn new(entry: &ModuleEntry, state: Arc<RwLock<WiriState>>) -> Self {
        let max_length = entry
            .extra
            .get("max-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_LENGTH);
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("center") => BarRegion::Center,
            Some("right") => BarRegion::Right,
            _ => BarRegion::Center,
        };
        Self { state, max_length, region }
    }

    fn snapshot(&self) -> ModuleSnapshot {
        let title = self
            .state
            .read()
            .map(|s| s.focused_window_title.clone())
            .unwrap_or_default();

        let text = truncate(&title, self.max_length);
        ModuleSnapshot {
            text,
            fg: None,
            icon: None,
            region: self.region,
        }
    }
}

/// Truncate a string to `max` characters, appending `…` if cut.
fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        // Leave room for ellipsis
        let cut = max.saturating_sub(1);
        let mut out: String = chars[..cut].iter().collect();
        out.push(ELLIPSIS);
        out
    }
}

impl Module for FocusedWindowModule {
    fn kind(&self) -> &'static str {
        "focused-window"
    }

    fn initial(&self) -> ModuleSnapshot {
        self.snapshot()
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
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let s = "a".repeat(70);
        let result = truncate(&s, 60);
        assert_eq!(result.chars().count(), 60);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hi", 60), "hi");
    }

    #[test]
    fn test_focused_window_snapshot() {
        let state = Arc::new(RwLock::new(WiriState {
            focused_window_title: "Visual Studio Code".to_string(),
            ..Default::default()
        }));
        let entry = ModuleEntry::default();
        let m = FocusedWindowModule::new(&entry, state);
        assert_eq!(m.initial().text, "Visual Studio Code");
    }
}

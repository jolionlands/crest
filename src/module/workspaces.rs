use std::time::Duration;
use std::sync::{Arc, RwLock};
use std::process::Command;

use crate::config::types::ModuleEntry;
use super::{BarRegion, Module, ModuleEvent, ModuleSnapshot, WiriState};

pub struct WorkspacesModule {
    state: Arc<RwLock<WiriState>>,
    region: BarRegion,
}

impl WorkspacesModule {
    pub fn new(entry: &ModuleEntry, state: Arc<RwLock<WiriState>>) -> Self {
        let region = match entry.extra.get("region").map(|s| s.as_str()) {
            Some("center") => BarRegion::Center,
            Some("right") => BarRegion::Right,
            _ => BarRegion::Left,
        };
        Self { state, region }
    }

    fn snapshot(&self) -> ModuleSnapshot {
        let st = self.state.read().unwrap_or_else(|e| e.into_inner());
        let text = if st.workspaces.is_empty() {
            "—".to_string()
        } else {
            st.workspaces
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    if i == st.focused_workspace {
                        format!("[{}]", name)
                    } else {
                        name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        ModuleSnapshot {
            text,
            fg: None,
            icon: None,
            region: self.region,
        }
    }

    fn focused_index(&self) -> usize {
        self.state
            .read()
            .map(|s| s.focused_workspace)
            .unwrap_or(0)
    }

    fn workspace_count(&self) -> usize {
        self.state
            .read()
            .map(|s| s.workspaces.len())
            .unwrap_or(0)
    }

    fn switch_to(&self, index: usize) {
        // wiri-ctl focus-workspace is 1-based
        let tag = index + 1;
        let _ = Command::new("wiri-ctl")
            .args(["focus-workspace", &tag.to_string()])
            .spawn();
    }
}

impl Module for WorkspacesModule {
    fn kind(&self) -> &'static str {
        "workspaces"
    }

    fn initial(&self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn tick(&mut self) -> ModuleSnapshot {
        self.snapshot()
    }

    fn on_event(&mut self, event: ModuleEvent) -> Option<String> {
        let count = self.workspace_count();
        if count == 0 {
            return None;
        }
        let current = self.focused_index();
        let next = match event {
            ModuleEvent::LeftClick | ModuleEvent::ScrollDown => (current + 1) % count,
            ModuleEvent::RightClick | ModuleEvent::ScrollUp => {
                if current == 0 { count - 1 } else { current - 1 }
            }
            _ => return None,
        };
        self.switch_to(next);
        None
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(500)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(workspaces: Vec<&str>, focused: usize) -> Arc<RwLock<WiriState>> {
        Arc::new(RwLock::new(WiriState {
            workspaces: workspaces.iter().map(|s| s.to_string()).collect(),
            focused_workspace: focused,
            focused_window_title: String::new(),
        }))
    }

    #[test]
    fn test_workspace_format_active() {
        let state = make_state(vec!["1", "2", "3", "4", "5"], 2);
        let entry = ModuleEntry::default();
        let m = WorkspacesModule::new(&entry, state);
        let snap = m.initial();
        assert_eq!(snap.text, "1 2 [3] 4 5");
    }

    #[test]
    fn test_workspace_format_empty() {
        let state = make_state(vec![], 0);
        let entry = ModuleEntry::default();
        let m = WorkspacesModule::new(&entry, state);
        assert_eq!(m.initial().text, "—");
    }
}

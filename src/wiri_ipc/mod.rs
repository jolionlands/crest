//! wiri IPC subscriber.
//!
//! Opens `\\.\pipe\wiri_control`, sends a `subscribe_events` message, then
//! reads newline-delimited JSON events forever.  On each event it updates the
//! shared `WiriState`.  Reconnects with 5-second backoff when the pipe closes.
//!
//! If wiri is not running, this just keeps retrying — no error is surfaced to
//! the user.

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, info};

use crate::module::WiriState;

// ---------------------------------------------------------------------------
// IPC event types (subset of wiri's IpcEvent)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "data")]
enum WiriEvent {
    #[serde(rename = "workspaces_changed")]
    WorkspacesChanged { workspaces: Vec<WorkspaceInfo> },

    #[serde(rename = "workspace_activated")]
    WorkspaceActivated { workspace: String },

    #[serde(rename = "workspace_switched")]
    WorkspaceSwitched {
        workspace_id: i32,
        monitor_id: u64,
    },

    #[serde(rename = "window_focused")]
    WindowFocused { window_hwnd: isize },

    #[serde(rename = "window_opened")]
    WindowOpened { window: WindowInfoIpc },

    #[serde(rename = "window_closed")]
    WindowClosed { window_hwnd: isize },

    // Catch-all so unknown events don't cause parse errors
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Clone)]
struct WorkspaceInfo {
    pub name: String,
    pub id: u32,
    pub window_count: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct WindowInfoIpc {
    pub hwnd: isize,
    pub title: String,
}

// ---------------------------------------------------------------------------
// WiriClient
// ---------------------------------------------------------------------------

pub struct WiriClient {
    state: Arc<RwLock<WiriState>>,
}

impl WiriClient {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(WiriState::default())),
        }
    }

    /// Return a clone of the shared state handle.
    pub fn state(&self) -> Arc<RwLock<WiriState>> {
        Arc::clone(&self.state)
    }

    /// Connect to wiri's named pipe and listen for events.  Reconnects on
    /// disconnect.  This method blocks indefinitely — run it in a thread or
    /// tokio task.
    pub fn connect_and_listen(&self) {
        let pipe_name = r"\\.\pipe\wiri_control";
        let subscribe_msg = "{\"type\":\"subscribe_events\",\"data\":{\"event_types\":[]}}\n";

        loop {
            match self.try_connect_once(pipe_name, subscribe_msg) {
                Ok(()) => {
                    info!("wiri IPC: connection closed normally");
                }
                Err(e) => {
                    debug!("wiri IPC: {e}; retrying in 5s");
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    fn try_connect_once(&self, pipe_name: &str, subscribe_msg: &str) -> anyhow::Result<()> {
        use std::fs::OpenOptions;

        let mut pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe_name)
            .map_err(|e| anyhow::anyhow!("pipe open failed: {e}"))?;

        info!("wiri IPC: connected to {pipe_name}");

        // Send subscribe request
        pipe.write_all(subscribe_msg.as_bytes())
            .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;

        let reader = BufReader::new(&pipe);
        for line in reader.lines() {
            let line = line.map_err(|e| anyhow::anyhow!("read error: {e}"))?;
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<WiriEvent>(&line) {
                Ok(event) => self.apply_event(event),
                Err(e) => debug!("wiri IPC: unrecognised event ({e}): {line}"),
            }
        }

        Ok(())
    }

    fn apply_event(&self, event: WiriEvent) {
        let mut state = match self.state.write() {
            Ok(s) => s,
            Err(e) => e.into_inner(),
        };

        match event {
            WiriEvent::WorkspacesChanged { workspaces } => {
                state.workspaces = workspaces.iter().map(|w| w.name.clone()).collect();
                debug!("wiri IPC: workspaces_changed → {} workspaces", state.workspaces.len());
            }

            WiriEvent::WorkspaceActivated { workspace } => {
                if let Some(idx) = state.workspaces.iter().position(|w| w == &workspace) {
                    state.focused_workspace = idx;
                }
                debug!("wiri IPC: workspace_activated → {workspace}");
            }

            WiriEvent::WorkspaceSwitched { workspace_id, .. } => {
                // workspace_id is 1-based in wiri; convert to 0-based index
                let idx = (workspace_id as usize).saturating_sub(1);
                if idx < state.workspaces.len() {
                    state.focused_workspace = idx;
                }
                debug!("wiri IPC: workspace_switched → id={workspace_id}");
            }

            WiriEvent::WindowFocused { window_hwnd } => {
                // Title will be updated if we receive a separate window info
                // event; until then keep whatever we have.
                state.focused_window_hwnd = Some(window_hwnd);
                debug!("wiri IPC: window_focused → hwnd={window_hwnd}");
            }

            WiriEvent::WindowOpened { window } => {
                // If the newly opened window is the focused one, update title
                if state.focused_window_hwnd == Some(window.hwnd) {
                    state.focused_window_title = window.title.clone();
                }
                debug!("wiri IPC: window_opened → hwnd={} title={}", window.hwnd, window.title);
            }

            WiriEvent::WindowClosed { window_hwnd } => {
                if state.focused_window_hwnd == Some(window_hwnd) {
                    state.focused_window_hwnd = None;
                    state.focused_window_title.clear();
                }
                debug!("wiri IPC: window_closed → hwnd={window_hwnd}");
            }

            WiriEvent::Unknown => {}
        }
    }
}

impl Default for WiriClient {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_client_with_workspaces() -> WiriClient {
        let client = WiriClient::new();
        {
            let mut state = client.state.write().unwrap();
            state.workspaces = vec!["1".to_string(), "2".to_string(), "3".to_string()];
            state.focused_workspace = 0;
        }
        client
    }

    #[test]
    fn test_wiri_ipc_parses_workspaces_changed_event() {
        let json = r#"{"type":"workspaces_changed","data":{"workspaces":[{"name":"1","id":1,"window_count":2},{"name":"2","id":2,"window_count":0}]}}"#;
        let event: WiriEvent = serde_json::from_str(json).expect("should parse");
        let client = WiriClient::new();
        client.apply_event(event);
        let state = client.state.read().unwrap();
        assert_eq!(state.workspaces, vec!["1", "2"]);
    }

    #[test]
    fn test_wiri_ipc_parses_workspace_activated_event() {
        let client = make_client_with_workspaces();
        let json = r#"{"type":"workspace_activated","data":{"workspace":"2"}}"#;
        let event: WiriEvent = serde_json::from_str(json).expect("should parse");
        client.apply_event(event);
        let state = client.state.read().unwrap();
        assert_eq!(state.focused_workspace, 1); // "2" is at index 1
    }

    #[test]
    fn test_wiri_ipc_parses_workspace_switched_event() {
        let client = make_client_with_workspaces();
        let json = r#"{"type":"workspace_switched","data":{"workspace_id":3,"monitor_id":0}}"#;
        let event: WiriEvent = serde_json::from_str(json).expect("should parse");
        client.apply_event(event);
        let state = client.state.read().unwrap();
        assert_eq!(state.focused_workspace, 2); // id=3 → index 2
    }

    #[test]
    fn test_wiri_ipc_unknown_event_ignored() {
        let json = r#"{"type":"some_future_event","data":{}}"#;
        let event: WiriEvent = serde_json::from_str(json).expect("should parse");
        // Should not panic
        let client = WiriClient::new();
        client.apply_event(event);
        let state = client.state.read().unwrap();
        assert!(state.workspaces.is_empty());
    }

    #[test]
    fn test_wiri_ipc_window_focused_tracks_hwnd() {
        let client = WiriClient::new();
        let json = r#"{"type":"window_focused","data":{"window_hwnd":12345}}"#;
        let event: WiriEvent = serde_json::from_str(json).unwrap();
        client.apply_event(event);
        let state = client.state.read().unwrap();
        assert_eq!(state.focused_window_hwnd, Some(12345));
    }
}
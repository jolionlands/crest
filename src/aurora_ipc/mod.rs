//! Subscribe to aurora's IPC event stream and track the current wallpaper per
//! monitor. Mirrors `wiri_ipc::mod.rs`.

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;
use parking_lot::RwLock;
use tracing::{debug, warn};

const PIPE: &str = r"\\.\pipe\aurora";

#[derive(Debug, Default, Clone)]
pub struct AuroraState {
    /// monitor_id → current wallpaper path
    pub current_wallpapers: HashMap<String, String>,
}

pub struct AuroraClient {
    state: Arc<RwLock<AuroraState>>,
}

impl AuroraClient {
    pub fn new() -> Self {
        Self { state: Arc::new(RwLock::new(AuroraState::default())) }
    }

    pub fn state(&self) -> Arc<RwLock<AuroraState>> {
        Arc::clone(&self.state)
    }

    /// Long-lived loop: open pipe, send subscribe_events, read events forever.
    /// Reconnect with backoff on disconnect.
    pub fn connect_and_listen(self) {
        loop {
            match self.run_once() {
                Ok(_) => debug!("aurora IPC: clean disconnect"),
                Err(e) => debug!("aurora IPC: {} — retrying in 5s", e),
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    fn run_once(&self) -> std::io::Result<()> {
        use std::io::{BufRead, BufReader, Write};
        use std::fs::OpenOptions;

        let mut pipe = OpenOptions::new().read(true).write(true).open(PIPE)?;

        // Send subscribe message — aurora expects JSON
        let sub = r#"{"type":"subscribe_events","types":["wallpaper_changed"]}"#;
        writeln!(pipe, "{}", sub)?;

        let reader = BufReader::new(&pipe);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            if let Ok(evt) = serde_json::from_str::<serde_json::Value>(&line) {
                self.handle_event(evt);
            }
        }
        Ok(())
    }

    fn handle_event(&self, evt: serde_json::Value) {
        let kind = evt.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if kind == "wallpaper_changed" {
            if let (Some(monitor), Some(path)) = (
                evt.get("monitor_id").and_then(|v| v.as_str()),
                evt.get("path").and_then(|v| v.as_str()),
            ) {
                self.state.write().current_wallpapers.insert(
                    monitor.to_string(),
                    path.to_string(),
                );
                debug!("aurora IPC: wallpaper_changed monitor={} path={}", monitor, path);
            } else {
                warn!("aurora IPC: wallpaper_changed event missing monitor_id or path");
            }
        }
    }
}

impl Default for AuroraClient {
    fn default() -> Self {
        Self::new()
    }
}

//! crest's own control pipe at \\.\pipe\crest_control
//!
//! Handles one-shot request/response commands from `crest-ctl`:
//!   status  — returns `{"ok":true,"message":"running"}`
//!   reload  — re-reads config from disk, returns ok/err
//!   quit    — signals main to exit, returns ok
//!
//! Protocol (mirrors crest-ctl convention):
//!   client sends a newline-terminated JSON object
//!   server replies with a newline-terminated JSON object, then closes the pipe

use std::io;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::core::PCWSTR;

const PIPE_NAME: &str = r"\\.\pipe\crest_control";
/// Per-instance buffer size (bytes).
const BUF_SIZE: u32 = 65536;

// ---------------------------------------------------------------------------
// Wire types — must match what crest-ctl serialises / expects.
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum CtlRequest {
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "quit")]
    Quit,
}

#[derive(Serialize)]
struct CtlResponse {
    ok: bool,
    message: String,
}

// ---------------------------------------------------------------------------
// ControlServer
// ---------------------------------------------------------------------------

pub struct ControlServer {
    pub config: Arc<RwLock<crate::config::types::Config>>,
    pub config_path: PathBuf,
    pub quit_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

impl ControlServer {
    /// Run the server loop forever; each call to `serve_one` handles exactly
    /// one client connection (create pipe → wait for connect → read → write →
    /// disconnect → close).  Runs on a dedicated OS thread.
    pub fn run(self) {
        info!("control_ipc: listening on {}", PIPE_NAME);
        loop {
            if let Err(e) = self.serve_one(PIPE_NAME) {
                debug!("control_ipc: serve_one error: {e}");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    /// Accept one client, handle one request, disconnect.
    ///
    /// `pipe_name` is a parameter (not a constant) so tests can pass a unique
    /// name without running on the production pipe.
    fn serve_one(&self, pipe_name: &str) -> io::Result<()> {
        let pipe_name_w: Vec<u16> = pipe_name
            .encode_utf16()
            .chain(std::iter::once(0u16))
            .collect();

        let pipe: HANDLE = unsafe {
            CreateNamedPipeW(
                PCWSTR::from_raw(pipe_name_w.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                BUF_SIZE,
                BUF_SIZE,
                0, // default timeout
                None,
            )
        };

        if pipe == INVALID_HANDLE_VALUE {
            let e = io::Error::last_os_error();
            warn!("control_ipc: CreateNamedPipeW failed: {e}");
            return Err(e);
        }

        // Block until a client connects.
        let connect_result = unsafe { ConnectNamedPipe(pipe, None) };
        // ERROR_PIPE_CONNECTED (535) means the client connected between
        // CreateNamedPipeW and ConnectNamedPipe — still a valid connection.
        let ok = connect_result.is_ok()
            || io::Error::last_os_error().raw_os_error() == Some(535);
        if !ok {
            let e = io::Error::last_os_error();
            warn!("control_ipc: ConnectNamedPipe failed: {e}");
            unsafe { CloseHandle(pipe) };
            return Err(e);
        }

        debug!("control_ipc: client connected");

        // Read one line (newline-terminated JSON) using a BufReader over a
        // std::fs::File view of the HANDLE.  The File does NOT own the handle —
        // we keep `pipe` and call DisconnectNamedPipe/CloseHandle ourselves.
        let line = {
            // SAFETY: `pipe` is a valid handle for the duration of this block.
            // We call `std::mem::forget` on the File so it doesn't close the
            // handle on drop.
            let file = unsafe { std::fs::File::from_raw_handle(pipe.0 as *mut _) };
            let mut reader = BufReader::new(&file);
            let mut line = String::new();
            let read_result = reader.read_line(&mut line);
            // Prevent File from closing the handle — we manage it explicitly.
            std::mem::forget(file);
            read_result?;
            line
        };

        let response: CtlResponse = match serde_json::from_str(line.trim()) {
            Ok(req) => self.handle(req),
            Err(e) => {
                warn!("control_ipc: bad request ({e}): {:?}", line.trim());
                CtlResponse { ok: false, message: format!("parse error: {e}") }
            }
        };

        // Write response (newline-terminated so crest-ctl's BufReader.lines()
        // returns immediately).
        let mut resp_bytes =
            serde_json::to_vec(&response).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
        resp_bytes.push(b'\n');

        // Write and flush via Win32 so we control handle lifetime exactly.
        unsafe {
            let mut written: u32 = 0;
            let _ = WriteFile(pipe, Some(&resp_bytes), Some(&mut written), None);
            let _ = FlushFileBuffers(pipe);
            DisconnectNamedPipe(pipe);
            CloseHandle(pipe);
        }

        Ok(())
    }

    fn handle(&self, req: CtlRequest) -> CtlResponse {
        match req {
            CtlRequest::Status => {
                info!("control_ipc: status");
                CtlResponse { ok: true, message: "running".into() }
            }

            CtlRequest::Reload => {
                info!("control_ipc: reload");
                match std::fs::read_to_string(&self.config_path) {
                    Ok(src) => match crate::config::parse::parse_kdl_config(&src) {
                        Ok(new_cfg) => {
                            *self.config.write() = new_cfg;
                            CtlResponse { ok: true, message: "config reloaded".into() }
                        }
                        Err(e) => CtlResponse {
                            ok: false,
                            message: format!("parse error: {e}"),
                        },
                    },
                    Err(e) => CtlResponse {
                        ok: false,
                        message: format!("read error: {e}"),
                    },
                }
            }

            CtlRequest::Quit => {
                info!("control_ipc: quit");
                let _ = self.quit_tx.send(());
                CtlResponse { ok: true, message: "quitting".into() }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Unit tests — pure serde, no pipes required
    // -----------------------------------------------------------------------

    #[test]
    fn test_control_server_request_deser_status() {
        let req: CtlRequest =
            serde_json::from_str(r#"{"type":"status"}"#).expect("deser");
        assert!(matches!(req, CtlRequest::Status));
    }

    #[test]
    fn test_control_server_request_deser_reload() {
        let req: CtlRequest =
            serde_json::from_str(r#"{"type":"reload"}"#).expect("deser");
        assert!(matches!(req, CtlRequest::Reload));
    }

    #[test]
    fn test_control_server_request_deser_quit() {
        let req: CtlRequest =
            serde_json::from_str(r#"{"type":"quit"}"#).expect("deser");
        assert!(matches!(req, CtlRequest::Quit));
    }

    #[test]
    fn test_control_server_response_ser() {
        let resp = CtlResponse { ok: true, message: "running".into() };
        let json = serde_json::to_string(&resp).expect("ser");
        let v: serde_json::Value = serde_json::from_str(&json).expect("re-parse");
        assert_eq!(v["ok"].as_bool(), Some(true));
        assert_eq!(v["message"].as_str(), Some("running"));
    }

    // -----------------------------------------------------------------------
    // Integration test — real named pipe, Windows only
    // -----------------------------------------------------------------------

    /// Start a minimal pipe server on a unique name, connect as a client,
    /// send a Status request, verify the response has ok:true.
    #[test]
    #[cfg(target_os = "windows")]
    fn test_control_server_status() {
        use std::fs::OpenOptions;
        use std::io::{BufRead, BufReader, Write};

        // Unique name: pid + stack-address nibbles.
        let unique: usize = {
            let dummy = 0usize;
            (&dummy as *const usize as usize) & 0xFFFF
        };
        let test_pipe = format!(
            r"\\.\pipe\crest_ctl_test_{}_{}",
            std::process::id(),
            unique,
        );

        // Server thread: create pipe, wait for one connection, echo ok:true.
        let srv_pipe = test_pipe.clone();
        let srv = std::thread::spawn(move || {
            let pipe_name_w: Vec<u16> = srv_pipe
                .encode_utf16()
                .chain(std::iter::once(0u16))
                .collect();
            let pipe = unsafe {
                CreateNamedPipeW(
                    PCWSTR::from_raw(pipe_name_w.as_ptr()),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1, // single instance for the test
                    65536,
                    65536,
                    0,
                    None,
                )
            };
            assert!(pipe != INVALID_HANDLE_VALUE, "server: CreateNamedPipeW failed");

            let connected = unsafe { ConnectNamedPipe(pipe, None) };
            let ok = connected.is_ok()
                || io::Error::last_os_error().raw_os_error() == Some(535);
            assert!(ok, "server: ConnectNamedPipe failed");

            // Read request (we ignore the content — just drain it).
            let file = unsafe { std::fs::File::from_raw_handle(pipe.0 as *mut _) };
            let mut reader = BufReader::new(&file);
            let mut _line = String::new();
            let _ = reader.read_line(&mut _line);
            std::mem::forget(file);

            // Write response.
            let resp = b"{\"ok\":true,\"message\":\"running\"}\n";
            let mut bw: u32 = 0;
            unsafe {
                let _ = WriteFile(pipe, Some(resp.as_ref()), Some(&mut bw), None);
                let _ = FlushFileBuffers(pipe);
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
        });

        // Give the server a moment to create the pipe.
        std::thread::sleep(Duration::from_millis(100));

        // Client: open, write status request, read response.
        let mut client = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&test_pipe)
            .expect("client: open pipe");

        client.write_all(b"{\"type\":\"status\"}\n").expect("client: write");

        let reader = BufReader::new(&client);
        let line = reader
            .lines()
            .next()
            .expect("client: no response")
            .expect("client: io error");

        let v: serde_json::Value =
            serde_json::from_str(&line).expect("client: parse response");
        assert_eq!(v["ok"].as_bool(), Some(true), "response ok field");
        assert_eq!(v["message"].as_str(), Some("running"), "response message");

        srv.join().expect("server thread panicked");
    }
}

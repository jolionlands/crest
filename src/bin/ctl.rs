//! crest-ctl — command-line interface to a running crest daemon.
//!
//! Communicates via `\\.\pipe\crest_control` using newline-delimited JSON,
//! the same pattern as wiri-ctl / wiri.
//!
//! Sub-commands:
//!   status   — check if the daemon is reachable
//!   reload   — ask the daemon to reload its config
//!   quit     — ask the daemon to exit

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

const PIPE_NAME: &str = r"\\.\pipe\crest_control";
const TIMEOUT_MS: u64 = 3000;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "crest-ctl", version, about = "Control a running crest bar")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check if the crest daemon is running and reachable.
    Status,
    /// Tell the daemon to reload its configuration file.
    Reload,
    /// Tell the daemon to exit.
    Quit,
}

// ---------------------------------------------------------------------------
// IPC message types (crest-internal, not wiri)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum CtlRequest {
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "quit")]
    Quit,
}

#[derive(Debug, Deserialize)]
struct CtlResponse {
    pub ok: bool,
    #[serde(default)]
    pub message: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    let request = match cli.command {
        Command::Status => CtlRequest::Status,
        Command::Reload => CtlRequest::Reload,
        Command::Quit => CtlRequest::Quit,
    };

    match send_request(request) {
        Ok(resp) => {
            if resp.ok {
                println!("ok: {}", resp.message);
            } else {
                eprintln!("error: {}", resp.message);
                std::process::exit(1);
            }
        }
        Err(e) => {
            // Pipe not found means crest is not running — give a friendly message.
            if is_not_found(&e) {
                eprintln!("crest is not running (pipe not found)");
            } else {
                eprintln!("crest-ctl: {e}");
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Pipe I/O
// ---------------------------------------------------------------------------

/// Open the control pipe, send a JSON request, read back one JSON response.
fn send_request(request: CtlRequest) -> Result<CtlResponse> {
    use std::fs::OpenOptions;

    // Wait for the pipe to become available (daemon may still be starting).
    let deadline = std::time::Instant::now() + Duration::from_millis(TIMEOUT_MS);
    let mut pipe = loop {
        match OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
            Ok(f) => break f,
            Err(e) if is_not_found_io(&e) => {
                bail!("pipe not found: crest daemon is not running");
            }
            Err(e) if is_busy_io(&e) => {
                if std::time::Instant::now() >= deadline {
                    bail!("pipe busy timeout after {TIMEOUT_MS}ms");
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    };

    // Send request
    let mut payload = serde_json::to_string(&request)
        .context("serialize request")?;
    payload.push('\n');
    pipe.write_all(payload.as_bytes())
        .context("write to pipe")?;

    // Read one line response
    let reader = BufReader::new(&pipe);
    let mut line = String::new();
    reader
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no response from daemon"))??
        .clone_into(&mut line);

    let resp: CtlResponse =
        serde_json::from_str(&line).context("deserialize response")?;
    Ok(resp)
}

// ---------------------------------------------------------------------------
// OS error classification
// ---------------------------------------------------------------------------

fn is_not_found(e: &anyhow::Error) -> bool {
    e.to_string().contains("pipe not found")
}

fn is_not_found_io(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound
}

fn is_busy_io(e: &std::io::Error) -> bool {
    // Win32 error 231 = ERROR_PIPE_BUSY
    e.raw_os_error() == Some(231)
}
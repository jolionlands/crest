//! crest — modular Direct2D status bar for Windows.
//!
//! Public module tree:
//!
//! - [`config`]      — KDL config loading and types
//! - [`bar`]         — Win32 window + Direct2D renderer
//! - [`module`]      — module trait, registry, built-in stubs
//! - [`wiri_ipc`]    — wiri named-pipe event subscriber + one-shot request helper
//! - [`aurora_ipc`]  — aurora named-pipe event subscriber (wallpaper)
//! - [`control_ipc`] — crest's own named-pipe server (status / reload / quit)
//! - [`hooks`]       — Windows Run-key autostart management

pub mod bar;
pub mod config;
pub mod control_ipc;
pub mod hooks;
pub mod module;
pub mod aurora_ipc;
pub mod wiri_ipc;
//! Bar window management.
//!
//! `mod.rs` re-exports the public surface of the `bar` subsystem so the rest
//! of crest only needs to `use crate::bar::*`.

pub mod window;
pub mod renderer;

pub use renderer::Direct2DRenderer;
pub use window::{Bar, Rect as MonitorRect};
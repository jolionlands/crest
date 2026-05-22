pub mod parse;
pub mod types;

pub use parse::parse_kdl_config;
pub use types::*;

use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

/// Default config bundled into the binary.
pub const DEFAULT_CONFIG_KDL: &str = include_str!("../../resources/default_config.kdl");

/// Return the platform config path: `%APPDATA%\crest\config.kdl`.
pub fn config_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| {
        dirs_fallback()
    });
    PathBuf::from(appdata).join("crest").join("config.kdl")
}

fn dirs_fallback() -> String {
    // Fallback: %USERPROFILE%\AppData\Roaming
    let profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
    format!("{}\\AppData\\Roaming", profile)
}

/// Load config from disk, writing the default if the file does not exist.
pub fn load_config() -> Result<Config> {
    let path = config_path();

    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, DEFAULT_CONFIG_KDL)?;
        info!("wrote default config to {}", path.display());
    }

    let contents = std::fs::read_to_string(&path)?;
    let config = parse_kdl_config(&contents)?;
    info!("loaded config from {}", path.display());
    Ok(config)
}

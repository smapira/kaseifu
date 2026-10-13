//! User configuration file location.
//!
//! The path is resolved through the OS user-data directory (never a
//! hard-coded home string), so the CLI works from any working directory.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// File name of the user configuration.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Resolve the user configuration path.
///
/// On macOS this is `~/Library/Application Support/kaseifu/config.toml`.
pub fn user_config_path() -> PathBuf {
    app_support_dir().join("kaseifu").join(CONFIG_FILE_NAME)
}

/// Resolve the kaseifu application directory (parent of the config file).
pub fn app_dir() -> PathBuf {
    app_support_dir().join("kaseifu")
}

fn app_support_dir() -> PathBuf {
    if let Some(dir) = dirs::data_dir() {
        return dir;
    }
    fallback_app_support()
}

fn fallback_app_support() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("Library").join("Application Support")
}

/// Create the parent directory of `path` when it is missing.
pub fn ensure_parent(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    Ok(())
}

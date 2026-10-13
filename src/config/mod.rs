//! User-facing configuration: defaults, TOML storage, priority resolution.
//!
//! Secrets never live here; the API key is resolved from the environment or
//! the macOS Keychain (see `crate::auth`).

pub mod defaults;
pub mod key_source;
pub mod loader;
pub mod resolver;
pub mod storage;

pub use key_source::{
    ApiKeyOrigin, KEY_SOURCE_ENV, KeySourceError, KeySourceMode, KeySourceSetting,
};
pub use loader::{
    SUPPORTED_KEYS, UserConfig, apply_setting, load_from, load_user, parse_toml, save_to, validate,
};
pub use resolver::{
    ApiKey, CliOverrides, JevConfig, LocatedValue, ResolvedConfig, ResolvedMetadata,
    key_source_setting, resolve, resolve_with, resolve_with_env,
};
pub use storage::{app_dir, user_config_path};

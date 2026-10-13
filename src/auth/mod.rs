//! Authentication: Keychain storage, interactive prompts, and the
//! zero-configuration key acquisition flow.
//!
//! The API key lives in the macOS Keychain (or the process environment for
//! developers/CI). It is never stored in TOML, never taken from CLI
//! arguments, and never printed.

pub mod acquire;
pub mod keychain;
pub mod prompt;

pub use keychain::{
    AuthError, KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, KeychainStore, MemoryStore, SecretStore,
    TEST_KEYCHAIN_SERVICE,
};

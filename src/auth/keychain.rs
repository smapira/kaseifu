//! macOS Keychain access for the Jev API key.
//!
//! The service (`com.routeflags.kaseifu.jev`) and account (`default`) are
//! fixed so every command addresses the same entry. Tests must use
//! [`KeychainStore::with_service`] (a `.test` service id) or [`MemoryStore`]
//! and never touch the production entry. Error messages never contain the key.

use thiserror::Error;

/// Keychain service identifier for the Jev API key.
pub const KEYCHAIN_SERVICE: &str = "com.routeflags.kaseifu.jev";

/// Keychain account identifier for the Jev API key.
pub const KEYCHAIN_ACCOUNT: &str = "default";

/// Service identifier reserved for automated tests.
pub const TEST_KEYCHAIN_SERVICE: &str = "com.routeflags.kaseifu.jev.test";

/// Keychain failures (the key value is never included).
#[derive(Debug, Error)]
pub enum AuthError {
    /// No entry exists in the Keychain.
    #[error("no API key is stored")]
    NotFound,
    /// The Keychain denied access (user refused, locked, ...).
    #[error("Keychain access was denied")]
    AccessDenied,
    /// The Keychain is unavailable on this machine.
    #[error("Keychain is unavailable: {0}")]
    Unavailable(String),
    /// An empty value was offered (rejected before touching the Keychain).
    #[error("API key must not be empty")]
    EmptySecret,
}

/// Minimal secret-storage boundary (Keychain or in-memory for tests).
pub trait SecretStore {
    /// Read the stored secret, or [`AuthError::NotFound`] when absent.
    fn get(&self) -> Result<String, AuthError>;
    /// Create or replace the stored secret (empty values are rejected).
    fn set(&self, secret: &str) -> Result<(), AuthError>;
    /// Delete the stored secret, or [`AuthError::NotFound`] when absent.
    fn delete(&self) -> Result<(), AuthError>;
}

/// Keychain-backed secret store.
pub struct KeychainStore {
    service: String,
    account: String,
}

impl KeychainStore {
    /// Production store (`com.routeflags.kaseifu.jev` / `default`).
    pub fn production() -> Self {
        Self::with_service(KEYCHAIN_SERVICE)
    }

    /// Store under an explicit service id (tests use [`TEST_KEYCHAIN_SERVICE`]).
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_string(),
            account: KEYCHAIN_ACCOUNT.to_string(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, AuthError> {
        keyring::Entry::new(&self.service, &self.account)
            .map_err(|error| AuthError::Unavailable(error.to_string()))
    }
}

impl SecretStore for KeychainStore {
    fn get(&self) -> Result<String, AuthError> {
        let entry = self.entry()?;
        entry.get_password().map_err(map_keyring_error)
    }

    fn set(&self, secret: &str) -> Result<(), AuthError> {
        if secret.trim().is_empty() {
            return Err(AuthError::EmptySecret);
        }
        let entry = self.entry()?;
        entry
            .set_password(secret)
            .map_err(|error| AuthError::Unavailable(error.to_string()))
    }

    fn delete(&self) -> Result<(), AuthError> {
        let entry = self.entry()?;
        entry.delete_credential().map_err(map_keyring_error)
    }
}

fn map_keyring_error(error: keyring::Error) -> AuthError {
    match error {
        keyring::Error::NoEntry => AuthError::NotFound,
        keyring::Error::PlatformFailure(_) => AuthError::AccessDenied,
        other => AuthError::Unavailable(other.to_string()),
    }
}

/// In-memory secret store for tests (never touches the real Keychain).
#[derive(Debug, Default)]
pub struct MemoryStore {
    secret: std::sync::Mutex<Option<String>>,
}

impl MemoryStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a store pre-filled with a secret.
    pub fn with_secret(secret: &str) -> Self {
        Self {
            secret: std::sync::Mutex::new(Some(secret.to_string())),
        }
    }
}

impl SecretStore for MemoryStore {
    fn get(&self) -> Result<String, AuthError> {
        read_secret(&self.secret).ok_or(AuthError::NotFound)
    }

    fn set(&self, secret: &str) -> Result<(), AuthError> {
        if secret.trim().is_empty() {
            return Err(AuthError::EmptySecret);
        }
        write_secret(&self.secret, secret)
    }

    fn delete(&self) -> Result<(), AuthError> {
        take_secret(&self.secret).ok_or(AuthError::NotFound)?;
        Ok(())
    }
}

fn read_secret(slot: &std::sync::Mutex<Option<String>>) -> Option<String> {
    slot.lock().ok()?.clone()
}

fn write_secret(slot: &std::sync::Mutex<Option<String>>, secret: &str) -> Result<(), AuthError> {
    match slot.lock() {
        Ok(mut guard) => {
            *guard = Some(secret.to_string());
            Ok(())
        }
        Err(_) => Err(AuthError::Unavailable(
            "memory store lock failed".to_string(),
        )),
    }
}

fn take_secret(slot: &std::sync::Mutex<Option<String>>) -> Option<String> {
    slot.lock().ok()?.take()
}

//! API key source selection (`KASEIFU_KEY_SOURCE`).
//!
//! The API key is resolved from exactly one place per invocation: the
//! environment (including the project-local `.env`) or the macOS Keychain.
//! `auto` keeps the historical order (environment first, Keychain as the
//! fallback), `env` and `keychain` pin a single source. Debug builds default
//! to `env` so `cargo run` and tests never trigger a Keychain approval dialog;
//! release builds keep `auto` for end users.
//!
//! Everything here is pure over its inputs (the caller supplies the raw
//! variable value), so tests never mutate the process environment.

use thiserror::Error;

/// Environment variable selecting the API key source.
pub const KEY_SOURCE_ENV: &str = "KASEIFU_KEY_SOURCE";

/// Where the API key may be read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySourceMode {
    /// Environment/`.env` first, the Keychain as the fallback (release default).
    Auto,
    /// Environment/`.env` only; the Keychain is never touched (debug default).
    Env,
    /// The Keychain only; environment values are ignored.
    Keychain,
}

impl KeySourceMode {
    /// Parse a `KASEIFU_KEY_SOURCE` value (trimmed, case-insensitive).
    pub fn parse(raw: &str) -> Result<Self, KeySourceError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "env" => Ok(Self::Env),
            "keychain" => Ok(Self::Keychain),
            _ => Err(KeySourceError {
                value: raw.trim().to_string(),
            }),
        }
    }

    /// Default mode for a build: debug builds prefer `.env`, releases `auto`.
    pub fn default_for_build(debug_build: bool) -> Self {
        if debug_build { Self::Env } else { Self::Auto }
    }

    /// The effective default of the running build.
    pub fn build_default() -> Self {
        Self::default_for_build(cfg!(debug_assertions))
    }

    /// Display label (`auto` / `env` / `keychain`).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Env => "env",
            Self::Keychain => "keychain",
        }
    }

    /// True when the mode may read or write the macOS Keychain.
    ///
    /// `env` is development/test mode: no Keychain call is ever allowed in
    /// that mode (neither reads nor writes).
    pub fn keychain_allowed(&self) -> bool {
        !matches!(self, Self::Env)
    }
}

/// The effective `KASEIFU_KEY_SOURCE` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySourceSetting {
    /// Selected mode.
    pub mode: KeySourceMode,
    /// True when the variable was set explicitly (not the build default).
    pub explicit: bool,
}

impl KeySourceSetting {
    /// Build a setting from a raw variable value (`None` = unset).
    ///
    /// Empty/whitespace values count as unset and resolve to the build
    /// default; unknown values are rejected so typos never fall back silently.
    pub fn from_raw(raw: Option<&str>) -> Result<Self, KeySourceError> {
        match raw.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => Ok(Self {
                mode: KeySourceMode::parse(value)?,
                explicit: true,
            }),
            None => Ok(Self {
                mode: KeySourceMode::build_default(),
                explicit: false,
            }),
        }
    }

    /// Explicit setting for callers that already know the mode.
    pub fn explicit(mode: KeySourceMode) -> Self {
        Self {
            mode,
            explicit: true,
        }
    }

    /// Display label with a `(default)` suffix when the variable was unset.
    pub fn label(&self) -> String {
        if self.explicit {
            self.mode.label().to_string()
        } else {
            format!("{} (default)", self.mode.label())
        }
    }
}

/// Where the resolved API key came from (never the key itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyOrigin {
    /// `JEV_API_KEY` from the process environment.
    Environment,
    /// `JEV_API_KEY` from the project-local `.env`.
    Dotenv,
    /// The macOS Keychain.
    Keychain,
    /// No key was found in the selected source.
    Missing,
    /// The selected source failed (the message carries the cause, never the key).
    Unavailable(String),
}

impl ApiKeyOrigin {
    /// Display label for reports (`env` / `.env` / `keychain` / ...).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Environment => "env",
            Self::Dotenv => ".env",
            Self::Keychain => "keychain",
            Self::Missing => "not configured",
            Self::Unavailable(_) => "unavailable",
        }
    }
}

/// `KASEIFU_KEY_SOURCE` rejected the offered value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid KASEIFU_KEY_SOURCE value {value:?}: expected auto, env, or keychain")]
pub struct KeySourceError {
    /// The offending value (a mode name, never a secret).
    pub value: String,
}

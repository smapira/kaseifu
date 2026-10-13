//! Environment diagnostics (`kaseifu doctor`).
//!
//! Local checks always run; network checks only run with an explicit flag so
//! a plain `doctor` never touches the network. Output is ASCII-only and the
//! API key value never appears.

use crate::auth::keychain::{AuthError, SecretStore};
use crate::config::loader::load_user;
use crate::config::resolver::ResolvedConfig;
use std::time::Duration;

/// Outcome of one diagnostic check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// Short check name (e.g. `Configuration`).
    pub name: &'static str,
    /// True when the check passed.
    pub ok: bool,
    /// Human-readable detail (never contains secrets).
    pub detail: String,
}

/// Connectivity outcome for the Jev endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connectivity {
    /// No network access was attempted.
    NotTested,
    /// The endpoint answered (any HTTP status counts as reachable).
    Reachable,
    /// No usable answer arrived.
    Unreachable(String),
}

/// Full doctor report (local checks plus optional connectivity).
#[derive(Debug, Clone)]
pub struct DoctorReport {
    /// Local environment checks.
    pub checks: Vec<CheckResult>,
    /// Connectivity result.
    pub connectivity: Connectivity,
    /// Effective endpoint shown for context (never the key).
    pub endpoint: String,
}

/// Run local diagnostics; probe the network only when `check_connection`.
pub async fn diagnose(
    config: &ResolvedConfig,
    store: &dyn SecretStore,
    check_connection: bool,
) -> DoctorReport {
    let checks = vec![
        check_platform(),
        check_configuration(),
        check_keychain(config, store),
        check_endpoint(&config.jev.endpoint),
        check_spotlight(),
        check_storage(),
    ];
    let connectivity = probe_connectivity(&config.jev.endpoint, check_connection).await;
    DoctorReport {
        checks,
        connectivity,
        endpoint: config.jev.endpoint.clone(),
    }
}

fn pass(name: &'static str, detail: &str) -> CheckResult {
    CheckResult {
        name,
        ok: true,
        detail: detail.to_string(),
    }
}

fn fail(name: &'static str, detail: &str) -> CheckResult {
    CheckResult {
        name,
        ok: false,
        detail: detail.to_string(),
    }
}

fn check_platform() -> CheckResult {
    if cfg!(target_os = "macos") {
        pass("Platform", "macOS supported")
    } else {
        fail("Platform", "not macOS; Keychain and Spotlight need macOS")
    }
}

fn check_configuration() -> CheckResult {
    match load_user() {
        Ok(_) => pass("Configuration", "TOML loaded"),
        Err(error) => fail("Configuration", &short_error(&error)),
    }
}

fn check_keychain(config: &ResolvedConfig, store: &dyn SecretStore) -> CheckResult {
    if config.api_key_from_env {
        return pass("Keychain", "API key provided via environment");
    }
    match store.get() {
        Ok(_) => pass("Keychain", "API key configured"),
        Err(AuthError::NotFound) => fail("Keychain", "no API key stored (run setup)"),
        Err(error) => fail("Keychain", &error.to_string()),
    }
}

fn check_endpoint(endpoint: &str) -> CheckResult {
    match url::Url::parse(endpoint) {
        Ok(url) if url.scheme() == "https" || url.scheme() == "http" => {
            pass("Jev Endpoint", "URL form looks valid")
        }
        _ => fail("Jev Endpoint", "URL form is invalid"),
    }
}

fn check_spotlight() -> CheckResult {
    let mdls = command_available("mdls");
    let mdfind = command_available("mdfind");
    if mdls && mdfind {
        pass("Spotlight", "mdls and mdfind available")
    } else {
        fail("Spotlight", "mdls or mdfind is missing")
    }
}

fn check_storage() -> CheckResult {
    let dir = crate::config::storage::app_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => pass("Storage", "data directory is writable"),
        Err(error) => fail("Storage", &short_io_error(&error)),
    }
}

fn command_available(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .is_ok_and(|out| out.status.success())
}

async fn probe_connectivity(endpoint: &str, check_connection: bool) -> Connectivity {
    if !check_connection {
        return Connectivity::NotTested;
    }
    match reach_endpoint(endpoint).await {
        Ok(()) => Connectivity::Reachable,
        Err(detail) => Connectivity::Unreachable(detail),
    }
}

async fn reach_endpoint(endpoint: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| first_line(&error.to_string()))?;
    let response = client
        .get(endpoint)
        .send()
        .await
        .map_err(|error| first_line(&error.to_string()))?;
    let _ = response
        .bytes()
        .await
        .map_err(|error| first_line(&error.to_string()))?;
    Ok(())
}

fn short_error(error: &anyhow::Error) -> String {
    first_line(&error.to_string())
}

fn short_io_error(error: &std::io::Error) -> String {
    first_line(&error.to_string())
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or(text)
        .chars()
        .take(120)
        .collect()
}

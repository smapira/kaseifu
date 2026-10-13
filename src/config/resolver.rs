//! Configuration resolution: CLI > environment > user TOML/Keychain > defaults.
//!
//! The resolver is pure over its inputs (no global reads inside the core), so
//! tests inject values instead of mutating the process environment. The only
//! secret handled here is the API key, wrapped in [`ApiKey`] whose `Debug`
//! output is redacted.

use super::defaults;
use super::loader::UserConfig;
use crate::auth::keychain::SecretStore;
use crate::classifier::jev::{JEV_API_KEY_ENV, JEV_API_URL_ENV, JEV_ENDPOINT_ENV, JEV_MODEL_ENV};
use anyhow::Result;

/// An API key that never prints its value (no `Display`, redacted `Debug`).
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap a raw key value.
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrow the raw value for authenticated requests only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(REDACTED)")
    }
}

/// Per-invocation CLI overrides (highest priority, never a secret).
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    /// Override for the Jev endpoint URL.
    pub endpoint: Option<String>,
    /// Override for the Jev model alias.
    pub model: Option<String>,
}

/// Fully resolved Jev credentials and connection settings.
#[derive(Debug, Clone)]
pub struct JevConfig {
    /// Endpoint URL (base URL or full `/v1/systemone` URL; never duplicated).
    pub endpoint: String,
    /// Model alias sent to the API.
    pub model: String,
    /// API key from the environment or Keychain (never logged).
    pub api_key: Option<ApiKey>,
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// Resolved Jev connection settings.
    pub jev: JevConfig,
    /// Effective policy thresholds.
    pub auto_move_confidence: f64,
    /// Effective policy thresholds.
    pub auto_move_margin: f64,
    /// Effective policy thresholds.
    pub review_confidence: f64,
    /// Effective dry-run default.
    pub dry_run: bool,
    /// Effective undo availability.
    pub enable_undo: bool,
    /// Whether the API key came from the process environment.
    pub api_key_from_env: bool,
}

/// Resolve from the live process: env, project `.env`, user TOML, Keychain.
pub fn resolve(cli: &CliOverrides, store: &dyn SecretStore) -> Result<ResolvedConfig> {
    let file = super::loader::load_user()?;
    resolve_with_env(cli, &file, store)
}

/// Resolve from the live environment with an explicit file configuration.
///
/// Callers that must survive a corrupt TOML (e.g. `doctor`, `setup`) load
/// the file themselves with a default fallback and use this function.
pub fn resolve_with_env(
    cli: &CliOverrides,
    file: &UserConfig,
    store: &dyn SecretStore,
) -> Result<ResolvedConfig> {
    let dotenv = dotenv_map();
    let lookup = |name: &str| env_or_dotenv(name, &dotenv);
    resolve_with(cli, &lookup, file, store)
}

/// Core resolution over injected inputs (used by [`resolve`] and tests).
pub fn resolve_with(
    cli: &CliOverrides,
    lookup: &dyn Fn(&str) -> Option<String>,
    file: &UserConfig,
    store: &dyn SecretStore,
) -> Result<ResolvedConfig> {
    let endpoint = pick_endpoint(cli, lookup, file)?;
    let model = pick_model(cli, lookup, file);
    let (api_key, from_env) = pick_api_key(lookup, store)?;
    Ok(ResolvedConfig {
        jev: JevConfig {
            endpoint,
            model,
            api_key,
        },
        auto_move_confidence: file
            .classifier
            .thresholds
            .auto_move_confidence
            .unwrap_or(defaults::DEFAULT_AUTO_MOVE_CONFIDENCE),
        auto_move_margin: file
            .classifier
            .thresholds
            .auto_move_margin
            .unwrap_or(defaults::DEFAULT_AUTO_MOVE_MARGIN),
        review_confidence: file
            .classifier
            .thresholds
            .review_confidence
            .unwrap_or(defaults::DEFAULT_REVIEW_CONFIDENCE),
        dry_run: file.operations.dry_run.unwrap_or(defaults::DEFAULT_DRY_RUN),
        enable_undo: file
            .operations
            .enable_undo
            .unwrap_or(defaults::DEFAULT_ENABLE_UNDO),
        api_key_from_env: from_env,
    })
}

fn pick_endpoint(
    cli: &CliOverrides,
    lookup: &dyn Fn(&str) -> Option<String>,
    file: &UserConfig,
) -> Result<String> {
    if let Some(endpoint) = cli.endpoint.clone() {
        return Ok(endpoint);
    }
    if let Some(endpoint) = lookup(JEV_API_URL_ENV) {
        return Ok(endpoint);
    }
    if let Some(endpoint) = lookup(JEV_ENDPOINT_ENV) {
        return Ok(endpoint);
    }
    if let Some(endpoint) = file.jev.endpoint.clone() {
        return Ok(endpoint);
    }
    Ok(defaults::DEFAULT_ENDPOINT.to_string())
}

fn pick_model(
    cli: &CliOverrides,
    lookup: &dyn Fn(&str) -> Option<String>,
    file: &UserConfig,
) -> String {
    if let Some(model) = cli.model.clone() {
        return model;
    }
    if let Some(model) = lookup(JEV_MODEL_ENV) {
        return model;
    }
    if let Some(model) = file.jev.model.clone() {
        return model;
    }
    defaults::DEFAULT_MODEL.to_string()
}

fn pick_api_key(
    lookup: &dyn Fn(&str) -> Option<String>,
    store: &dyn SecretStore,
) -> Result<(Option<ApiKey>, bool)> {
    if let Some(raw) = lookup(JEV_API_KEY_ENV) {
        if raw.trim().is_empty() {
            return Ok((None, false));
        }
        return Ok((Some(ApiKey::new(raw)), true));
    }
    // A Keychain failure reads as "no key" here so diagnostics (`doctor`,
    // `auth status`) can report the cause themselves instead of aborting.
    // Callers that write (`setup`, `auth set-key`) still surface the error.
    match store.get() {
        Ok(raw) => Ok((Some(ApiKey::new(raw)), false)),
        Err(_) => Ok((None, false)),
    }
}

/// Developer/CI compatibility: read one value from the project-local `.env`.
pub fn dotenv_value(name: &str) -> Option<String> {
    dotenv_map().remove(name)
}

fn env_or_dotenv(name: &str, dotenv: &std::collections::HashMap<String, String>) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        return Some(value);
    }
    dotenv.get(name).cloned()
}

fn dotenv_map() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Ok(contents) = std::fs::read_to_string(".env") else {
        return map;
    };
    for (key, value) in parse_dotenv(&contents) {
        map.insert(key, value);
    }
    map
}

fn parse_dotenv(contents: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in contents.lines() {
        if let Some(pair) = parse_dotenv_line(line) {
            pairs.push(pair);
        }
    }
    pairs
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    if key.trim().is_empty() {
        return None;
    }
    Some((key.trim().to_string(), unquote(value.trim())))
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_string()
}

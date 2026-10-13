//! Configuration resolution: CLI > environment > user TOML/Keychain > defaults.
//!
//! The API key comes from exactly one source per invocation, selected by
//! `KASEIFU_KEY_SOURCE` (see [`crate::config::key_source`]): `auto` keeps the
//! historical environment-then-Keychain order, `env` never touches the
//! Keychain and `keychain` never reads the environment. The resolver is pure
//! over its inputs (no global reads inside the core), so tests inject values
//! instead of mutating the process environment. The only secret handled here
//! is the API key, wrapped in [`ApiKey`] whose `Debug` output is redacted.

use super::defaults;
use super::key_source::{ApiKeyOrigin, KEY_SOURCE_ENV, KeySourceMode, KeySourceSetting};
use super::loader::UserConfig;
use crate::auth::keychain::{AuthError, SecretStore};
use crate::classifier::jev::{JEV_API_KEY_ENV, JEV_API_URL_ENV, JEV_ENDPOINT_ENV, JEV_MODEL_ENV};
use anyhow::Result;

/// A value found for a configuration key, with its origin.
///
/// Only the API key reports the origin (the process environment and `.env`
/// are shown separately); other settings ignore [`LocatedValue::from_dotenv`].
/// The value is a secret when the key is an API key, so this type deliberately
/// has no `Debug` implementation.
#[derive(Clone)]
pub struct LocatedValue {
    /// The raw value.
    pub value: String,
    /// True when the value came from `.env` rather than the process environment.
    pub from_dotenv: bool,
}

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
    /// Effective image metadata and privacy settings (v3.2).
    pub metadata: ResolvedMetadata,
    /// Whether the API key came from the process environment or `.env`.
    pub api_key_from_env: bool,
    /// Effective `KASEIFU_KEY_SOURCE` setting for this invocation.
    pub key_source: KeySourceSetting,
    /// Where the resolved API key came from (never the key itself).
    pub api_key_origin: ApiKeyOrigin,
}

/// Fully resolved image metadata and privacy settings (v3.2).
#[derive(Debug, Clone)]
pub struct ResolvedMetadata {
    /// Master switch for image metadata extraction.
    pub image_enabled: bool,
    /// Use Spotlight as the first source.
    pub spotlight: bool,
    /// Use embedded EXIF as a fallback for missing values.
    pub exif_fallback: bool,
    /// Send capture time to Jev.
    pub send_capture_time_to_jev: bool,
    /// Send GPS coordinates to Jev (default false).
    pub send_gps_to_jev: bool,
    /// Weather enrichment (Phase 8 reserved; no behavior).
    pub weather_enabled: bool,
}

/// Resolve from the live process: env, project `.env`, user TOML, Keychain.
pub fn resolve(cli: &CliOverrides, store: &dyn SecretStore) -> Result<ResolvedConfig> {
    let file = super::loader::load_user()?;
    resolve_with_env(cli, &file, store)
}

/// Effective key source from the live process environment and `.env`.
///
/// Commands that act before resolution (`auth set-key`, `auth remove-key`) use
/// this to decide whether the Keychain may be touched at all. The process
/// environment wins over `.env`; empty values count as unset.
pub fn key_source_setting() -> Result<KeySourceSetting> {
    let dotenv = dotenv_map();
    Ok(KeySourceSetting::from_raw(
        raw_key_source(&dotenv).as_deref(),
    )?)
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
    let key_source = KeySourceSetting::from_raw(raw_key_source(&dotenv).as_deref())?;
    let lookup = |name: &str| env_or_dotenv(name, &dotenv);
    resolve_with(cli, &lookup, file, store, key_source)
}

/// Raw `KASEIFU_KEY_SOURCE` value: process environment first, then `.env`.
///
/// An empty process variable never masks a `.env` value; both empty resolve to
/// the build default in [`KeySourceSetting::from_raw`].
fn raw_key_source(dotenv: &std::collections::HashMap<String, String>) -> Option<String> {
    let from_env = std::env::var(KEY_SOURCE_ENV).ok();
    if let Some(value) = non_empty(from_env) {
        return Some(value);
    }
    non_empty(dotenv.get(KEY_SOURCE_ENV).cloned())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

/// Core resolution over injected inputs (used by [`resolve`] and tests).
pub fn resolve_with(
    cli: &CliOverrides,
    lookup: &dyn Fn(&str) -> Option<LocatedValue>,
    file: &UserConfig,
    store: &dyn SecretStore,
    key_source: KeySourceSetting,
) -> Result<ResolvedConfig> {
    let endpoint = pick_endpoint(cli, lookup, file)?;
    let model = pick_model(cli, lookup, file);
    let (api_key, api_key_origin) = pick_api_key(lookup, store, key_source.mode);
    let api_key_from_env = matches!(
        api_key_origin,
        ApiKeyOrigin::Environment | ApiKeyOrigin::Dotenv
    );
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
        metadata: resolve_metadata(file),
        api_key_from_env,
        key_source,
        api_key_origin,
    })
}

/// Resolve `[metadata.*]` settings (user TOML over application defaults).
fn resolve_metadata(file: &UserConfig) -> ResolvedMetadata {
    ResolvedMetadata {
        image_enabled: file
            .metadata
            .image
            .enabled
            .unwrap_or(defaults::DEFAULT_METADATA_IMAGE_ENABLED),
        spotlight: file
            .metadata
            .image
            .spotlight
            .unwrap_or(defaults::DEFAULT_METADATA_SPOTLIGHT),
        exif_fallback: file
            .metadata
            .image
            .exif_fallback
            .unwrap_or(defaults::DEFAULT_METADATA_EXIF_FALLBACK),
        send_capture_time_to_jev: file
            .metadata
            .privacy
            .send_capture_time_to_jev
            .unwrap_or(defaults::DEFAULT_SEND_CAPTURE_TIME_TO_JEV),
        send_gps_to_jev: file
            .metadata
            .privacy
            .send_gps_to_jev
            .unwrap_or(defaults::DEFAULT_SEND_GPS_TO_JEV),
        weather_enabled: file
            .metadata
            .weather
            .enabled
            .unwrap_or(defaults::DEFAULT_WEATHER_ENABLED),
    }
}

fn pick_endpoint(
    cli: &CliOverrides,
    lookup: &dyn Fn(&str) -> Option<LocatedValue>,
    file: &UserConfig,
) -> Result<String> {
    if let Some(endpoint) = cli.endpoint.clone() {
        return Ok(endpoint);
    }
    if let Some(endpoint) = lookup(JEV_API_URL_ENV) {
        return Ok(endpoint.value);
    }
    if let Some(endpoint) = lookup(JEV_ENDPOINT_ENV) {
        return Ok(endpoint.value);
    }
    if let Some(endpoint) = file.jev.endpoint.clone() {
        return Ok(endpoint);
    }
    Ok(defaults::DEFAULT_ENDPOINT.to_string())
}

fn pick_model(
    cli: &CliOverrides,
    lookup: &dyn Fn(&str) -> Option<LocatedValue>,
    file: &UserConfig,
) -> String {
    if let Some(model) = cli.model.clone() {
        return model;
    }
    if let Some(model) = lookup(JEV_MODEL_ENV) {
        return model.value;
    }
    if let Some(model) = file.jev.model.clone() {
        return model;
    }
    defaults::DEFAULT_MODEL.to_string()
}

/// Pick the API key for the selected source mode.
///
/// `env` never touches the store, `keychain` never touches the environment,
/// and `auto` keeps the historical environment-first-then-Keychain order.
/// A *present but blank* `JEV_API_KEY` counts as missing in every mode and
/// never triggers the Keychain fallback (a copied `.env.example` must not
/// cause an approval dialog).
fn pick_api_key(
    lookup: &dyn Fn(&str) -> Option<LocatedValue>,
    store: &dyn SecretStore,
    mode: KeySourceMode,
) -> (Option<ApiKey>, ApiKeyOrigin) {
    if mode == KeySourceMode::Keychain {
        return pick_keychain_key(store);
    }
    match lookup(JEV_API_KEY_ENV) {
        Some(found) => env_key(found),
        None if mode == KeySourceMode::Auto => pick_keychain_key(store),
        None => (None, ApiKeyOrigin::Missing),
    }
}

/// Convert a located `JEV_API_KEY` value into the resolved key.
fn env_key(found: LocatedValue) -> (Option<ApiKey>, ApiKeyOrigin) {
    if found.value.trim().is_empty() {
        return (None, ApiKeyOrigin::Missing);
    }
    let origin = if found.from_dotenv {
        ApiKeyOrigin::Dotenv
    } else {
        ApiKeyOrigin::Environment
    };
    (Some(ApiKey::new(found.value)), origin)
}

/// Read the Keychain; a failure is carried to diagnostics instead of aborting.
/// Callers that write (`setup`, `auth set-key`) still surface the error.
fn pick_keychain_key(store: &dyn SecretStore) -> (Option<ApiKey>, ApiKeyOrigin) {
    match store.get() {
        Ok(raw) => (Some(ApiKey::new(raw)), ApiKeyOrigin::Keychain),
        Err(AuthError::NotFound) => (None, ApiKeyOrigin::Missing),
        Err(error) => (None, ApiKeyOrigin::Unavailable(error.to_string())),
    }
}

/// Developer/CI compatibility: read one value from the project-local `.env`.
pub fn dotenv_value(name: &str) -> Option<String> {
    dotenv_map().remove(name)
}

fn env_or_dotenv(
    name: &str,
    dotenv: &std::collections::HashMap<String, String>,
) -> Option<LocatedValue> {
    if let Ok(value) = std::env::var(name) {
        return Some(LocatedValue {
            value,
            from_dotenv: false,
        });
    }
    dotenv.get(name).cloned().map(|value| LocatedValue {
        value,
        from_dotenv: true,
    })
}

fn dotenv_map() -> std::collections::HashMap<String, String> {
    // Parsed with `dotenvy` (the maintained fork of `dotenv`) so quoting,
    // comments, `export` prefixes and escapes behave the way the file format
    // defines. The values are deliberately *not* written into the process
    // environment: the resolver treats them as a fallback that real
    // environment variables win over, which is the documented precedence.
    match dotenvy::from_path_iter(".env") {
        Ok(pairs) => pairs.filter_map(Result::ok).collect(),
        Err(_) => std::collections::HashMap::new(),
    }
}

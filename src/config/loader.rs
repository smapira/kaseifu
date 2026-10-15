//! User TOML configuration: schema, loading, validation, atomic saving.
//!
//! The API key is never part of this file. Unknown keys are rejected so
//! typos fail loudly instead of being silently ignored.

use super::storage::{ensure_parent, user_config_path};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level user configuration (every field optional; defaults fill gaps).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserConfig {
    /// Rename response language (BCP47 tag such as `en` or `ja`).
    ///
    /// Top-level on purpose: the user-facing key is exactly `language`
    /// (`kaseifu config set language <tag>`), so the TOML shape mirrors the
    /// key one-to-one instead of hiding behind an alias.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Jev endpoint/model section.
    pub jev: JevSection,
    /// Classifier thresholds section.
    pub classifier: ClassifierSection,
    /// Local operation flags section.
    pub operations: OperationsSection,
    /// Image metadata and privacy section (v3.2).
    pub metadata: MetadataSection,
}

/// `[jev]` section of the user configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JevSection {
    /// API endpoint URL (base URL or full `/v1/systemone` URL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Model alias sent to the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// `[classifier]` section of the user configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClassifierSection {
    /// Threshold values for the policy engine.
    pub thresholds: ThresholdSection,
}

/// `[classifier.thresholds]` section of the user configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThresholdSection {
    /// Minimum confidence for automatic moves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_move_confidence: Option<f64>,
    /// Minimum margin over the second-best candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_move_margin: Option<f64>,
    /// Below this confidence the file is left in place.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_confidence: Option<f64>,
}

/// `[operations]` section of the user configuration.
///
/// Both fields are accepted for compatibility only and have **no effect on
/// behavior**: moves are controlled by `--execute` on the CLI, and every
/// executed move is journaled so it can be undone. The fields stay in the
/// schema (and in `config show`, marked inactive) because removing them would
/// make `deny_unknown_fields` reject existing user files.
///
/// Wiring `dry_run` as a safety interlock was considered and rejected: its
/// default is `true`, so it would stop every `--execute` move unless the user
/// changed the setting first — a silent trap rather than a safety net.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OperationsSection {
    /// Accepted for compatibility; inactive (`--execute` controls moves).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// Accepted for compatibility; inactive (journaling is always on).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_undo: Option<bool>,
}

/// `[metadata]` section of the user configuration (v3.2).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetadataSection {
    /// Image extraction toggles.
    #[serde(default)]
    pub image: MetadataImageSection,
    /// Privacy gates for Jev transmission.
    #[serde(default)]
    pub privacy: MetadataPrivacySection,
    /// Weather extension (Phase 8, reserved; stays disabled).
    #[serde(default)]
    pub weather: MetadataWeatherSection,
}

/// `[metadata.image]` section of the user configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetadataImageSection {
    /// Master switch for image metadata extraction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Use Spotlight as the first source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spotlight: Option<bool>,
    /// Use embedded EXIF as a fallback for missing values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exif_fallback: Option<bool>,
}

/// `[metadata.privacy]` section of the user configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetadataPrivacySection {
    /// Send capture time to Jev (default true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_capture_time_to_jev: Option<bool>,
    /// Send GPS coordinates to Jev (default false).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_gps_to_jev: Option<bool>,
}

/// `[metadata.weather]` section (Phase 8 reserved; no behavior yet).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetadataWeatherSection {
    /// Weather enrichment master switch (not implemented; keep false).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Load the user configuration from its standard location.
///
/// A missing file is not an error: it yields an empty configuration and the
/// application boots with defaults.
pub fn load_user() -> Result<UserConfig> {
    load_from(&user_config_path())
}

/// Load a configuration from an explicit path (missing file yields defaults).
pub fn load_from(path: &Path) -> Result<UserConfig> {
    if !path.exists() {
        return Ok(UserConfig::default());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read configuration {}", path.display()))?;
    let config = parse_toml(&text)?;
    validate(&config)?;
    Ok(config)
}

/// Parse TOML text into a configuration (unknown keys are rejected).
pub fn parse_toml(text: &str) -> Result<UserConfig> {
    toml::from_str(text).context("parse configuration TOML")
}

/// Validate every set value; unset values are always valid.
pub fn validate(config: &UserConfig) -> Result<()> {
    validate_language(config.language.as_deref())?;
    validate_endpoint(config.jev.endpoint.as_deref())?;
    validate_model(config.jev.model.as_deref())?;
    validate_threshold(
        "classifier.thresholds.auto_move_confidence",
        config.classifier.thresholds.auto_move_confidence,
    )?;
    validate_threshold(
        "classifier.thresholds.auto_move_margin",
        config.classifier.thresholds.auto_move_margin,
    )?;
    validate_threshold(
        "classifier.thresholds.review_confidence",
        config.classifier.thresholds.review_confidence,
    )?;
    Ok(())
}

fn validate_language(value: Option<&str>) -> Result<()> {
    let Some(tag) = value.map(str::trim) else {
        return Ok(());
    };
    validate_language_value(tag)
}

/// Check one rename response language tag (shared by the loader and `config set`).
///
/// Accepted shape is BCP47-like: ASCII alphanumeric subtags of up to 8
/// characters joined by single hyphens, starting with a letter (`en`, `ja`,
/// `en-US`, `zh-Hant`). The character budget mirrors the wire contract so a
/// stored tag always fits the request the bridge sends.
///
/// The value must already be trimmed (see [`set_language`]); an empty string
/// is rejected here so hand-edited files fail loudly like every other setting.
pub fn validate_language_value(tag: &str) -> Result<()> {
    if tag.is_empty() {
        anyhow::bail!("invalid language (must not be empty)");
    }
    if tag.chars().count() > crate::semantic::contract::MAX_LANGUAGE_CHARS {
        anyhow::bail!(
            "invalid language `{tag}` (expected 1-{} characters)",
            crate::semantic::contract::MAX_LANGUAGE_CHARS
        );
    }
    if !is_language_tag(tag) {
        anyhow::bail!("invalid language `{tag}` (expected a tag such as en or ja)");
    }
    Ok(())
}

/// True for BCP47-shaped tags: ASCII alphanumeric subtags joined by single
/// hyphens, starting with a letter.
pub fn is_language_tag(tag: &str) -> bool {
    if tag.is_empty() {
        return false;
    }
    let mut first = true;
    for subtag in tag.split('-') {
        if subtag.is_empty() || subtag.len() > 8 {
            return false;
        }
        if !subtag.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return false;
        }
        if first
            && !subtag
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic())
        {
            return false;
        }
        first = false;
    }
    true
}

/// Effective rename response language: the configured tag, or English.
///
/// A hand-edited invalid tag falls back to English here (loading still
/// rejects it, so the fallback only covers callers that read a defaulted
/// configuration instead of failing the run).
pub fn effective_language(config: &UserConfig) -> String {
    match config.language.as_deref().map(str::trim) {
        Some(tag) if validate_language_value(tag).is_ok() => tag.to_string(),
        _ => super::defaults::DEFAULT_LANGUAGE.to_string(),
    }
}

fn validate_endpoint(value: Option<&str>) -> Result<()> {
    let Some(endpoint) = value else {
        return Ok(());
    };
    validate_endpoint_value(endpoint)
}

/// Check one endpoint URL (shared by the loader and `config set`).
pub fn validate_endpoint_value(endpoint: &str) -> Result<()> {
    let url = url::Url::parse(endpoint).with_context(|| "invalid jev.endpoint URL")?;
    if url.scheme() != "https" && url.scheme() != "http" {
        anyhow::bail!("invalid jev.endpoint URL scheme (expected http or https)");
    }
    Ok(())
}

fn validate_model(value: Option<&str>) -> Result<()> {
    if value.is_some_and(|model| model.trim().is_empty()) {
        anyhow::bail!("invalid jev.model (must not be empty)");
    }
    Ok(())
}

fn validate_threshold(name: &str, value: Option<f64>) -> Result<()> {
    if value.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
        anyhow::bail!("invalid {name} (expected a number between 0 and 1)");
    }
    Ok(())
}

/// Save a configuration atomically: write a sibling temp file, then rename.
///
/// Validation runs first so a failed update keeps the existing file intact.
pub fn save_to(path: &Path, config: &UserConfig) -> Result<()> {
    validate(config)?;
    ensure_parent(path)?;
    let text = toml::to_string_pretty(config).context("serialize configuration")?;
    write_atomically(path, text.as_bytes())
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let temp = sibling_temp_path(path)?;
    std::fs::write(&temp, contents)
        .with_context(|| format!("write temporary configuration {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| {
        format!(
            "replace configuration {} (temp file kept at {})",
            path.display(),
            temp.display()
        )
    })?;
    Ok(())
}

fn sibling_temp_path(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| CONFIG_TMP_FALLBACK.to_string());
    Ok(parent.join(format!(".{name}.tmp-{}", std::process::id())))
}

/// Fallback file name used only when the target path has no file name.
const CONFIG_TMP_FALLBACK: &str = "config.toml";

// ---------------------------------------------------------------------------
// `config set` support
// ---------------------------------------------------------------------------

/// Settings writable through `config set` (the API key is never one of them).
pub const SUPPORTED_KEYS: &[&str] = &[
    "jev.endpoint",
    "jev.model",
    "classifier.thresholds.auto_move_confidence",
    "classifier.thresholds.auto_move_margin",
    "classifier.thresholds.review_confidence",
    "operations.dry_run",
    "operations.enable_undo",
    "metadata.image.enabled",
    "metadata.image.spotlight",
    "metadata.image.exif_fallback",
    "metadata.privacy.send_capture_time_to_jev",
    "metadata.privacy.send_gps_to_jev",
    "metadata.weather.enabled",
    "language",
];

/// Apply one `config set` update (typed parsing plus validation).
pub fn apply_setting(config: &mut UserConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "jev.endpoint" => set_endpoint(&mut config.jev.endpoint, value),
        "jev.model" => set_model(&mut config.jev.model, value),
        "classifier.thresholds.auto_move_confidence" => set_threshold(
            &mut config.classifier.thresholds.auto_move_confidence,
            key,
            value,
        ),
        "classifier.thresholds.auto_move_margin" => set_threshold(
            &mut config.classifier.thresholds.auto_move_margin,
            key,
            value,
        ),
        "classifier.thresholds.review_confidence" => set_threshold(
            &mut config.classifier.thresholds.review_confidence,
            key,
            value,
        ),
        "operations.dry_run" => set_flag(&mut config.operations.dry_run, key, value),
        "operations.enable_undo" => set_flag(&mut config.operations.enable_undo, key, value),
        "metadata.image.enabled" => set_flag(&mut config.metadata.image.enabled, key, value),
        "metadata.image.spotlight" => set_flag(&mut config.metadata.image.spotlight, key, value),
        "metadata.image.exif_fallback" => {
            set_flag(&mut config.metadata.image.exif_fallback, key, value)
        }
        "metadata.privacy.send_capture_time_to_jev" => set_flag(
            &mut config.metadata.privacy.send_capture_time_to_jev,
            key,
            value,
        ),
        "metadata.privacy.send_gps_to_jev" => {
            set_flag(&mut config.metadata.privacy.send_gps_to_jev, key, value)
        }
        "metadata.weather.enabled" => set_flag(&mut config.metadata.weather.enabled, key, value),
        _ => apply_other_setting(config, key, value),
    }
}

/// Apply a setting that is not one of the original hierarchical keys.
///
/// Splitting this arm out keeps [`apply_setting`] at its existing complexity:
/// the `_` arm delegates here instead of growing the main match.
fn apply_other_setting(config: &mut UserConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "language" => set_language(&mut config.language, value),
        _ => reject_unknown_key(key),
    }
}

fn set_endpoint(slot: &mut Option<String>, value: &str) -> Result<()> {
    validate_endpoint_value(value)?;
    *slot = Some(value.to_string());
    Ok(())
}

fn set_model(slot: &mut Option<String>, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("invalid jev.model (must not be empty)");
    }
    *slot = Some(value.to_string());
    Ok(())
}

fn set_language(slot: &mut Option<String>, value: &str) -> Result<()> {
    let tag = value.trim();
    validate_language_value(tag)?;
    *slot = Some(tag.to_string());
    Ok(())
}

fn set_threshold(slot: &mut Option<f64>, name: &str, value: &str) -> Result<()> {
    let parsed: f64 = value
        .parse()
        .with_context(|| format!("invalid {name} (expected a number between 0 and 1)"))?;
    validate_threshold(name, Some(parsed))?;
    *slot = Some(parsed);
    Ok(())
}

fn set_flag(slot: &mut Option<bool>, name: &str, value: &str) -> Result<()> {
    let parsed =
        parse_bool(value).with_context(|| format!("invalid {name} (expected true or false)"))?;
    *slot = Some(parsed);
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => anyhow::bail!("expected true or false"),
    }
}

fn reject_unknown_key(key: &str) -> Result<()> {
    if is_secret_key(key) {
        anyhow::bail!(
            "refused: the API key cannot be stored with `config set` (use `auth set-key`)"
        );
    }
    anyhow::bail!(
        "unknown setting `{key}` (supported: {})",
        SUPPORTED_KEYS.join(", ")
    )
}

fn is_secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    normalized.contains("apikey")
}

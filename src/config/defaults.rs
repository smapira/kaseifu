//! Application default values for user-facing settings.
//!
//! These defaults apply when neither CLI overrides, environment variables,
//! nor the user TOML provide a value. They mirror `config/default.toml`.

/// Default Jev API endpoint (full decision endpoint URL).
pub const DEFAULT_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// Default model alias (same value as the Jev adapter default).
pub const DEFAULT_MODEL: &str = crate::classifier::jev::DEFAULT_JEV_MODEL;

/// Default policy thresholds (same values as the policy engine constants).
pub const DEFAULT_AUTO_MOVE_CONFIDENCE: f64 = crate::policy::decision::AUTO_MOVE_CONFIDENCE;
/// Default policy thresholds (same values as the policy engine constants).
pub const DEFAULT_AUTO_MOVE_MARGIN: f64 = crate::policy::decision::AUTO_MOVE_MARGIN;
/// Default policy thresholds (same values as the policy engine constants).
pub const DEFAULT_REVIEW_CONFIDENCE: f64 = crate::policy::decision::REVIEW_CONFIDENCE;

/// Resolved default for `operations.dry_run` (inactive: moves are controlled
/// by `--execute`; the value is kept for `config show` compatibility).
pub const DEFAULT_DRY_RUN: bool = true;

/// Resolved default for `operations.enable_undo` (inactive: every executed
/// move is journaled; the value is kept for `config show` compatibility).
pub const DEFAULT_ENABLE_UNDO: bool = true;

/// Default image metadata extraction (v3.2 `[metadata.image]`).
pub const DEFAULT_METADATA_IMAGE_ENABLED: bool = true;
/// Default Spotlight first-source toggle.
pub const DEFAULT_METADATA_SPOTLIGHT: bool = true;
/// Default EXIF fallback toggle.
pub const DEFAULT_METADATA_EXIF_FALLBACK: bool = true;
/// Default capture-time transmission to Jev (`[metadata.privacy]`).
pub const DEFAULT_SEND_CAPTURE_TIME_TO_JEV: bool = true;
/// Default GPS transmission to Jev: disabled (privacy-first).
pub const DEFAULT_SEND_GPS_TO_JEV: bool = false;
/// Default weather enrichment (Phase 8 reserved; stays disabled).
pub const DEFAULT_WEATHER_ENABLED: bool = false;

/// Default rename response language (BCP47 tag; English).
pub const DEFAULT_LANGUAGE: &str = "en";

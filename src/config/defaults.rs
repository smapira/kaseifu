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

/// Default dry-run behavior (nothing moves unless `--execute` is passed).
pub const DEFAULT_DRY_RUN: bool = true;

/// Default undo availability.
pub const DEFAULT_ENABLE_UNDO: bool = true;

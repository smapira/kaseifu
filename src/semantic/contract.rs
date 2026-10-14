//! The `fm-rename-v1` JSON contract between Rust and the Swift bridge.
//!
//! Same-process data exchange only (original spec §3.4): the request describes
//! the file and the wanted fields, the response carries the semantic fields or
//! an explicit error. Rules that keep the boundary safe:
//!
//! - every envelope carries `schema_version`; a mismatch is an explicit error,
//! - unknown fields are ignored (forward compatible),
//! - a value of the wrong type is treated as missing, never as a reason to
//!   guess,
//! - Rust bounds every field it sends (the Swift side bounds again when it
//!   assembles the prompt).
//!
//! The matching JSON Schema lives in `schemas/fm-rename-v1.json`.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// Version of the request / response / availability envelopes.
pub const SCHEMA_VERSION: &str = "fm-rename-v1";

/// Character budget for `request_id`.
pub const MAX_REQUEST_ID_CHARS: usize = 64;
/// Character budget for the file name sent to the model.
pub const MAX_FILENAME_CHARS: usize = 512;
/// Character budget for the full path (informational only).
pub const MAX_PATH_CHARS: usize = 1024;
/// Character budget for the parent directory name.
pub const MAX_PARENT_CHARS: usize = 256;
/// Maximum number of semantic signals forwarded.
pub const MAX_SIGNALS: usize = 20;
/// Character budget for one semantic signal.
pub const MAX_SIGNAL_CHARS: usize = 120;
/// Character budget for the language tag.
pub const MAX_LANGUAGE_CHARS: usize = 16;

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// What kind of target the request describes (original spec §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Document,
    Image,
    Video,
    Directory,
}

impl TargetKind {
    /// Wire value (`document`, `image`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Image => "image",
            Self::Video => "video",
            Self::Directory => "directory",
        }
    }
}

/// Which semantic fields the caller wants (the model is told to leave the
/// others empty; it never invents values, original spec §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisFlags {
    pub title: bool,
    pub category: bool,
    pub project: bool,
}

impl Default for AnalysisFlags {
    fn default() -> Self {
        Self {
            title: true,
            category: true,
            project: false,
        }
    }
}

/// The bounded observation sent to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmInput {
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
    pub parent_directory: String,
    #[serde(default)]
    pub semantic_signals: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<FmImageFacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perception: Option<FmPerception>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<FmVideoFacts>,
}

/// Video facts already measured in Swift (AVFoundation, Phase 6).
///
/// A compatible extension of `fm-rename-v1` like [`FmPerception`]: the field
/// is optional, no key was removed or retyped, both sides ignore unknown
/// fields, and the envelope version stays the same. Only video targets carry
/// it. Rust never decodes a video: the bridge reports what the container
/// said, and a missing value stays missing (original spec sections 5.3, 6.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmVideoFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<String>,
}

impl From<&crate::models::file_context::VideoInfo> for FmVideoFacts {
    /// Copy the measured facts onto the wire (bounded like every other field).
    fn from(info: &crate::models::file_context::VideoInfo) -> Self {
        Self {
            duration_seconds: info.duration_seconds,
            width: info.width,
            height: info.height,
            frame_rate: info.frame_rate,
            creation_date: info
                .creation_date
                .as_deref()
                .map(|value| bound_chars(value, crate::semantic::video::MAX_VIDEO_DATE_CHARS)),
        }
    }
}

/// Image facts already extracted in Rust (EXIF / Spotlight).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmImageFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// Perception labels already extracted in Rust (Apple Vision, Phase 5b).
///
/// A compatible extension of `fm-rename-v1`: the field is optional, no key
/// was removed or retyped, both sides ignore unknown fields, and the envelope
/// version stays the same. Only image targets carry it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmPerception {
    pub labels: Vec<FmPerceptionLabel>,
}

/// One perception label as it travels to the bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmPerceptionLabel {
    pub label: String,
    pub confidence: f32,
}

impl From<&crate::models::file_context::PerceptionInfo> for FmPerception {
    /// Bound again at the wire: the request never carries more than the
    /// perception budget, whatever the observation held.
    fn from(info: &crate::models::file_context::PerceptionInfo) -> Self {
        let bounded = super::perception::bound_labels(info.labels.iter().cloned());
        Self {
            labels: bounded
                .labels
                .into_iter()
                .map(|entry| FmPerceptionLabel {
                    label: entry.label,
                    confidence: entry.confidence,
                })
                .collect(),
        }
    }
}

/// One generation request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmRequest {
    pub schema_version: String,
    pub request_id: String,
    pub target_type: TargetKind,
    pub path: String,
    pub language: String,
    pub analysis: AnalysisFlags,
    pub input: FmInput,
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// Status of one generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FmStatus {
    Success,
    Error,
    Cancelled,
    /// A status this build does not know (forward compatible).
    #[serde(other)]
    Unknown,
}

/// Error classification of a failed generation (original spec §12).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    Unavailable,
    ModelInitFailed,
    InferenceFailed,
    InvalidOutput,
    Cancelled,
    Internal,
    /// An error kind this build does not know.
    #[serde(other)]
    Unknown,
}

/// The semantic fields. Every field is optional on purpose: a missing value
/// stays missing instead of being invented.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticInfo {
    pub title: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
}

impl SemanticInfo {
    /// True when no field carries a value (the caller holds the proposal back,
    /// original spec §6.5).
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.category.is_none() && self.project.is_none()
    }
}

/// Error payload of a failed generation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FmErrorInfo {
    pub kind: ErrorKind,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

/// One generation response (deserialization side of the contract).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FmResponse {
    pub schema_version: String,
    pub request_id: String,
    pub status: FmStatus,
    #[serde(default)]
    pub semantic: Option<SemanticFields>,
    #[serde(default)]
    pub error: Option<FmErrorInfo>,
}

/// Raw `semantic` object; values of the wrong type are treated as missing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SemanticFields {
    #[serde(default, deserialize_with = "optional_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    pub category: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    pub project: Option<String>,
}

impl From<SemanticFields> for SemanticInfo {
    fn from(fields: SemanticFields) -> Self {
        Self {
            title: clean_optional(fields.title),
            category: clean_optional(fields.category),
            project: clean_optional(fields.project),
        }
    }
}

/// Accept `null`, a string, or anything else (treated as missing).
fn optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(text)) => Some(text),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

/// Whether the on-device model can be used, with the reason when it cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable {
        reason: String,
        detail: Option<String>,
    },
}

impl Availability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    /// The reason string when unavailable (`appleIntelligenceNotEnabled`, ...).
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Available => None,
            Self::Unavailable { reason, .. } => Some(reason),
        }
    }

    /// `available` or `unavailable: <reason>` (stable label for reports).
    pub fn label(&self) -> String {
        match self {
            Self::Available => "available".to_string(),
            Self::Unavailable { reason, .. } => format!("unavailable: {reason}"),
        }
    }
}

#[derive(Deserialize)]
struct AvailabilityEnvelope {
    schema_version: String,
    status: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing and bounds
// ---------------------------------------------------------------------------

/// Why a payload did not match the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    InvalidJson(String),
    VersionMismatch { expected: String, actual: String },
    MissingReason,
    UnknownStatus(String),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(message) => write!(f, "InvalidJson({message})"),
            Self::VersionMismatch { expected, actual } => {
                write!(f, "VersionMismatch(expected {expected}, got {actual})")
            }
            Self::MissingReason => write!(f, "MissingReason(unavailable without a reason)"),
            Self::UnknownStatus(status) => write!(f, "UnknownStatus({status})"),
        }
    }
}

/// Parse a generation response and check its envelope.
pub fn parse_response(json: &str) -> Result<FmResponse, ContractError> {
    let response: FmResponse = serde_json::from_str(json)
        .map_err(|error| ContractError::InvalidJson(error.to_string()))?;
    check_version(&response.schema_version)?;
    Ok(response)
}

/// Parse the availability payload.
pub fn parse_availability(json: &str) -> Result<Availability, ContractError> {
    let envelope: AvailabilityEnvelope = serde_json::from_str(json)
        .map_err(|error| ContractError::InvalidJson(error.to_string()))?;
    check_version(&envelope.schema_version)?;
    match envelope.status.as_str() {
        "available" => Ok(Availability::Available),
        "unavailable" => match clean_optional(envelope.reason) {
            Some(reason) => Ok(Availability::Unavailable {
                reason,
                detail: clean_optional(envelope.detail),
            }),
            None => Err(ContractError::MissingReason),
        },
        other => Err(ContractError::UnknownStatus(other.to_string())),
    }
}

fn check_version(actual: &str) -> Result<(), ContractError> {
    if actual == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ContractError::VersionMismatch {
            expected: SCHEMA_VERSION.to_string(),
            actual: actual.to_string(),
        })
    }
}

/// Trim and turn an empty string into `None`.
pub fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// Truncate to a character budget without splitting a UTF-8 character.
pub fn bound_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        value.chars().take(max).collect()
    }
}

/// Bound the signal list and every signal.
pub fn bound_signals(signals: &[String]) -> Vec<String> {
    signals
        .iter()
        .take(MAX_SIGNALS)
        .map(|signal| bound_chars(signal, MAX_SIGNAL_CHARS))
        .filter(|signal| !signal.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_parses_available_and_unavailable() {
        let available =
            parse_availability(r#"{"schema_version":"fm-rename-v1","status":"available"}"#)
                .expect("available parses");
        assert_eq!(available, Availability::Available);
        assert_eq!(available.label(), "available");

        let unavailable = parse_availability(
            r#"{"schema_version":"fm-rename-v1","status":"unavailable","reason":"appleIntelligenceNotEnabled","detail":"x"}"#,
        )
        .expect("unavailable parses");
        assert_eq!(unavailable.reason(), Some("appleIntelligenceNotEnabled"));
        assert_eq!(
            unavailable.label(),
            "unavailable: appleIntelligenceNotEnabled"
        );
    }

    #[test]
    fn availability_rejects_a_missing_reason_and_unknown_status() {
        let missing =
            parse_availability(r#"{"schema_version":"fm-rename-v1","status":"unavailable"}"#);
        assert_eq!(missing, Err(ContractError::MissingReason));

        let unknown = parse_availability(r#"{"schema_version":"fm-rename-v1","status":"maybe"}"#);
        assert_eq!(
            unknown,
            Err(ContractError::UnknownStatus("maybe".to_string()))
        );
    }

    #[test]
    fn availability_rejects_a_version_mismatch() {
        let mismatch = parse_availability(r#"{"schema_version":"v0","status":"available"}"#);
        assert!(matches!(
            mismatch,
            Err(ContractError::VersionMismatch { .. })
        ));
    }

    #[test]
    fn response_ignores_unknown_fields_and_unknown_error_kinds() {
        let response = parse_response(
            r#"{"schema_version":"fm-rename-v1","request_id":"r1","status":"error",
                "semantic":null,"error":{"kind":"something-new","detail":"d"},"extra":1}"#,
        )
        .expect("unknown fields are ignored");
        assert_eq!(response.status, FmStatus::Error);
        assert_eq!(response.error.expect("error").kind, ErrorKind::Unknown);
    }

    #[test]
    fn response_treats_wrong_types_as_missing() {
        let response = parse_response(
            r#"{"schema_version":"fm-rename-v1","request_id":"r1","status":"success",
                "semantic":{"title":7,"category":"  企画  ","project":null}}"#,
        )
        .expect("wrong types do not fail the parse");
        let info: SemanticInfo = response.semantic.expect("semantic").into();
        assert_eq!(info.title, None);
        assert_eq!(info.category, Some("企画".to_string()));
        assert_eq!(info.project, None);
    }

    #[test]
    fn response_unknown_status_is_unknown() {
        let response = parse_response(
            r#"{"schema_version":"fm-rename-v1","request_id":"r1","status":"weird"}"#,
        )
        .expect("parses");
        assert_eq!(response.status, FmStatus::Unknown);
    }

    #[test]
    fn bounds_truncate_on_character_boundaries() {
        let text = "あいうえお";
        assert_eq!(bound_chars(text, 3), "あいう");
        assert_eq!(bound_chars(text, 99), text);
        let signals: Vec<String> = (0..25).map(|index| format!("signal-{index}")).collect();
        let bounded = bound_signals(&signals);
        assert_eq!(bounded.len(), MAX_SIGNALS);
        assert_eq!(bounded[0], "signal-0");
    }

    #[test]
    fn request_serializes_the_required_envelope_keys() {
        let request = FmRequest {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-0001".to_string(),
            target_type: TargetKind::Document,
            path: "/tmp/a.pdf".to_string(),
            language: "ja".to_string(),
            analysis: AnalysisFlags::default(),
            input: FmInput {
                filename: "a.pdf".to_string(),
                extension: Some("pdf".to_string()),
                size_bytes: 3,
                created_at: None,
                modified_at: None,
                parent_directory: "/tmp".to_string(),
                semantic_signals: vec!["F:a".to_string()],
                image: None,
                perception: None,
                video: None,
            },
        };
        let json = serde_json::to_value(&request).expect("serializes");
        for key in [
            "schema_version",
            "request_id",
            "target_type",
            "path",
            "language",
            "analysis",
            "input",
        ] {
            assert!(json.get(key).is_some(), "request must carry {key}");
        }
        assert_eq!(json["analysis"]["project"], serde_json::json!(false));
        assert!(
            json["input"].get("perception").is_none(),
            "a document request carries no perception key"
        );
    }

    #[test]
    fn perception_travels_under_fm_rename_v1_without_touching_image() {
        let info = crate::models::file_context::PerceptionInfo {
            labels: vec![
                crate::models::file_context::PerceptionLabel {
                    label: "sky".to_string(),
                    confidence: 0.039,
                },
                crate::models::file_context::PerceptionLabel {
                    label: "sunset_sunrise".to_string(),
                    confidence: 0.038,
                },
            ],
        };
        let request = FmRequest {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-0002".to_string(),
            target_type: TargetKind::Image,
            path: "/tmp/IMG_1234.png".to_string(),
            language: "ja".to_string(),
            analysis: AnalysisFlags::default(),
            input: FmInput {
                filename: "IMG_1234.png".to_string(),
                extension: Some("png".to_string()),
                size_bytes: 3,
                created_at: None,
                modified_at: None,
                parent_directory: "/tmp".to_string(),
                semantic_signals: vec![],
                image: Some(FmImageFacts {
                    capture_time: Some("2025-08-10T15:30:00".to_string()),
                    camera: Some("TestMake TestModel".to_string()),
                    width: Some(64),
                    height: Some(48),
                }),
                perception: Some(FmPerception::from(&info)),
                video: None,
            },
        };
        let json = serde_json::to_value(&request).expect("serializes");
        assert_eq!(json["schema_version"], serde_json::json!(SCHEMA_VERSION));
        assert_eq!(json["target_type"], serde_json::json!("image"));
        assert_eq!(json["input"]["perception"]["labels"][0]["label"], "sky");
        assert_eq!(
            json["input"]["perception"]["labels"]
                .as_array()
                .expect("labels")
                .len(),
            2
        );
        // The existing image object keeps its exact key set.
        let mut keys: Vec<&str> = json["input"]["image"]
            .as_object()
            .expect("image")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["camera", "capture_time", "height", "width"]);
    }

    #[test]
    fn video_facts_travel_under_fm_rename_v1_without_touching_the_other_keys() {
        let info = crate::models::file_context::VideoInfo {
            duration_seconds: Some(120.0),
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(29.97),
            creation_date: Some("2026-10-14T09:30:00+0900".to_string()),
        };
        let request = FmRequest {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-0003".to_string(),
            target_type: TargetKind::Video,
            path: "/tmp/clip.mp4".to_string(),
            language: "ja".to_string(),
            analysis: AnalysisFlags::default(),
            input: FmInput {
                filename: "clip.mp4".to_string(),
                extension: Some("mp4".to_string()),
                size_bytes: 9,
                created_at: Some("2026-10-15T00:00:00Z".to_string()),
                modified_at: Some("2026-10-16T00:00:00Z".to_string()),
                parent_directory: "/tmp".to_string(),
                semantic_signals: vec![],
                image: None,
                perception: None,
                video: Some(FmVideoFacts::from(&info)),
            },
        };
        let json = serde_json::to_value(&request).expect("serializes");
        assert_eq!(json["target_type"], serde_json::json!("video"));
        assert_eq!(json["input"]["video"]["duration_seconds"], 120.0);
        assert_eq!(json["input"]["video"]["width"], 1920);
        assert_eq!(json["input"]["video"]["height"], 1080);
        assert_eq!(
            json["input"]["video"]["creation_date"],
            "2026-10-14T09:30:00+0900"
        );
        // Add-only: the video request carries no picture facts, and a request
        // without video facts keeps the exact key set it had before.
        assert!(json["input"].get("image").is_none());
        let mut keys: Vec<&str> = json["input"]
            .as_object()
            .expect("input")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "created_at",
                "extension",
                "filename",
                "modified_at",
                "parent_directory",
                "semantic_signals",
                "size_bytes",
                "video"
            ]
        );
        let empty = FmVideoFacts::from(&crate::models::file_context::VideoInfo::default());
        let value = serde_json::to_value(&empty).expect("serializes");
        assert_eq!(
            value,
            serde_json::json!({}),
            "a silent container says nothing"
        );
    }

    #[test]
    fn perception_is_bounded_again_on_the_wire() {
        let info = crate::models::file_context::PerceptionInfo {
            labels: (0..40)
                .map(|index| crate::models::file_context::PerceptionLabel {
                    label: format!("label-{index}"),
                    confidence: 0.5 + index as f32 / 100.0,
                })
                .collect(),
        };
        let wire = FmPerception::from(&info);
        assert_eq!(
            wire.labels.len(),
            crate::semantic::perception::MAX_PERCEPTION_LABELS
        );
        assert!(wire.labels.iter().all(|entry| {
            entry.label.chars().count() <= crate::semantic::perception::MAX_PERCEPTION_LABEL_CHARS
        }));
    }
}

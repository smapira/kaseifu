//! Image perception through Apple Vision (original spec sections 4.3, 5.2).
//!
//! The perception layer answers one question: what does this picture look
//! like? The answer is a bounded list of English classification labels with
//! their confidences, produced by `VNClassifyImageRequest`. Rust never decodes
//! pixels: the Swift bridge reads the image path with ImageIO (read-only
//! thumbnail bounded by [`MAX_PIXEL_SIDE`], aspect ratio kept) and runs the
//! classifier.
//!
//! The labels are data for the model, never a file name: the model turns them
//! into a title and the naming rule engine stays in Rust. Confidences are kept
//! in Rust (reports, tests) but are **not** sent into the prompt: the
//! classifier reports low absolute values (measured: 0.04-0.11 on a synthetic
//! sunset), and the ranking is what carries the meaning.

use std::cmp::Ordering;

use serde::Deserialize;

use super::contract;
use crate::models::file_context::{PerceptionInfo, PerceptionLabel};

/// Maximum number of labels forwarded (the classifier ranks far more).
///
/// Measured: a synthetic sunset yields 0.106 / 0.105 / 0.105 / 0.042 ... ;
/// twelve keeps the interesting bands (including equal-confidence pairs) and
/// still fits one bounded prompt line.
pub const MAX_PERCEPTION_LABELS: usize = 12;
/// Character budget for one label (Vision identifiers are short ASCII).
pub const MAX_PERCEPTION_LABEL_CHARS: usize = 64;
/// Confidence floor: everything below is the classifier's long tail.
///
/// Measured: the tail runs down to 0.002 ("no idea" territory) while real
/// bands sit at 0.03-0.9, so the floor only drops noise.
pub const MIN_PERCEPTION_CONFIDENCE: f32 = 0.01;
/// Maximum number of OCR text fragments forwarded (still images only).
///
/// Eight fragments of bounded length ride in one short prompt line next to
/// the labels; more would push the labels out of the prompt budget.
pub const MAX_OCR_TEXTS: usize = 8;
/// Character budget for one OCR fragment (signs and headings are short).
pub const MAX_OCR_CHARS: usize = 120;
/// Longest image side the bridge feeds to Vision (aspect ratio kept).
///
/// 2048^2 x 4 byte is about 16 MiB: a memory bound for the decode. Measured
/// classification time through this recipe: 0.078-0.385 s.
pub const MAX_PIXEL_SIDE: u32 = 2048;

/// Why perception did not produce labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerceptionError {
    /// The perception layer cannot run on this machine (explicit reason:
    /// `vision-framework-missing`, `foundation-models-sdk-missing`, ...).
    Unavailable { reason: String },
    /// The image could not be read or classified.
    Failed { detail: String },
    /// The bridge answered something that is not the perception contract.
    Malformed { detail: String },
}

impl std::fmt::Display for PerceptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { reason } => write!(f, "unavailable: {reason}"),
            Self::Failed { detail } => write!(f, "failed: {detail}"),
            Self::Malformed { detail } => write!(f, "malformed: {detail}"),
        }
    }
}

/// One perception envelope.
#[derive(Deserialize)]
struct Envelope {
    schema_version: String,
    status: String,
    #[serde(default)]
    perception: Option<Payload>,
    #[serde(default)]
    error: Option<Failure>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    labels: Vec<RawLabel>,
    /// OCR fragments; missing in older payloads (empty, never an error).
    #[serde(default)]
    text: Vec<String>,
}

/// One raw label; a missing or wrongly typed field is treated as missing.
///
/// Shared with the video path (`super::video`): a frame's labels use exactly
/// the same shape as a picture's labels.
#[derive(Deserialize)]
pub(crate) struct RawLabel {
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
}

/// One raw error payload; shared by the image and the video envelope.
#[derive(Deserialize)]
pub(crate) struct Failure {
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) reason: Option<String>,
    #[serde(default)]
    pub(crate) detail: Option<String>,
}

/// Parse one perception payload and bound its labels.
///
/// The same envelope shape as availability: a version mismatch, an unknown
/// status or a broken payload is an explicit error, never a silent success.
/// A label missing its text or confidence is skipped, never invented.
pub fn parse(json: &str) -> Result<PerceptionInfo, PerceptionError> {
    let envelope: Envelope =
        serde_json::from_str(json).map_err(|error| PerceptionError::Malformed {
            detail: error.to_string(),
        })?;
    if envelope.schema_version != contract::SCHEMA_VERSION {
        return Err(PerceptionError::Malformed {
            detail: format!(
                "expected {}, got {}",
                contract::SCHEMA_VERSION,
                envelope.schema_version
            ),
        });
    }
    match envelope.status.as_str() {
        "success" => {
            let payload = envelope.perception.unwrap_or(Payload {
                labels: Vec::new(),
                text: Vec::new(),
            });
            Ok(PerceptionInfo {
                labels: bound_labels(payload.labels.into_iter().filter_map(usable_label)).labels,
                text: bound_texts(payload.text),
            })
        }
        "error" => Err(classify_failure(envelope.error)),
        other => Err(PerceptionError::Malformed {
            detail: format!("unknown status: {other}"),
        }),
    }
}

/// Keep only labels that carry both a text and a number.
pub(crate) fn usable_label(raw: RawLabel) -> Option<PerceptionLabel> {
    let label = raw.label.unwrap_or_default();
    let confidence = raw.confidence?;
    Some(PerceptionLabel { label, confidence })
}

/// Map one error payload onto a perception error (unknown kinds stay named).
///
/// Shared with the video path: the error classification of the two envelope
/// shapes is the same rule set (original spec section 12).
pub(crate) fn classify_failure(error: Option<Failure>) -> PerceptionError {
    let Some(error) = error else {
        return PerceptionError::Malformed {
            detail: "error status without an error payload".to_string(),
        };
    };
    let detail = error.detail.or(error.reason.clone()).unwrap_or_default();
    match error.kind.as_deref() {
        Some("unavailable") => PerceptionError::Unavailable {
            reason: error.reason.unwrap_or_else(|| "unknown".to_string()),
        },
        Some(kind) => PerceptionError::Failed {
            detail: format!("{kind}: {detail}"),
        },
        None => PerceptionError::Malformed {
            detail: "error payload without a kind".to_string(),
        },
    }
}

/// Sort by confidence, drop the tail, bound the length and de-duplicate.
///
/// Applied to every label set the process handles (the bridge bounds again
/// when it assembles the payload), so no unbounded vision output can reach
/// the prompt, the journal or the report.
pub fn bound_labels(labels: impl IntoIterator<Item = PerceptionLabel>) -> PerceptionInfo {
    let mut cleaned: Vec<PerceptionLabel> = labels
        .into_iter()
        .filter(is_usable)
        .map(bounded_label)
        .collect();
    cleaned.sort_by(by_confidence);
    PerceptionInfo {
        labels: dedupe(cleaned),
        text: Vec::new(),
    }
}

/// Bound OCR fragments: trim, drop empties, keep printable ASCII only, bound
/// the length and de-duplicate case-insensitively up to the budget.
///
/// The CLI output contract is ASCII-only, so non-ASCII characters are
/// stripped from fragments (recorded limitation: a sign in another script
/// contributes only its ASCII remainder). Like the labels, fragments are
/// untrusted data: bounding here does not replace the injection check at
/// plan time.
pub fn bound_texts(texts: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    for fragment in texts {
        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let ascii = trimmed
            .chars()
            .filter(|ch| *ch >= ' ' && *ch <= '~')
            .collect::<String>();
        let cleaned = ascii.trim();
        if cleaned.is_empty() {
            continue;
        }
        let bounded = contract::bound_chars(cleaned, MAX_OCR_CHARS);
        let key = bounded.to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        kept.push(bounded);
        if kept.len() == MAX_OCR_TEXTS {
            break;
        }
    }
    kept
}

/// True for a label that survives the floor and carries text.
fn is_usable(entry: &PerceptionLabel) -> bool {
    !entry.label.trim().is_empty()
        && entry.confidence.is_finite()
        && entry.confidence >= MIN_PERCEPTION_CONFIDENCE
}

/// Trim and bound one label.
fn bounded_label(entry: PerceptionLabel) -> PerceptionLabel {
    PerceptionLabel {
        label: contract::bound_chars(entry.label.trim(), MAX_PERCEPTION_LABEL_CHARS),
        confidence: entry.confidence,
    }
}

/// Highest confidence first (a NaN never occurs here: the floor rejects it).
fn by_confidence(left: &PerceptionLabel, right: &PerceptionLabel) -> Ordering {
    right
        .confidence
        .partial_cmp(&left.confidence)
        .unwrap_or(Ordering::Equal)
}

/// Keep the first occurrence of every label text, up to the budget.
fn dedupe(sorted: Vec<PerceptionLabel>) -> Vec<PerceptionLabel> {
    let mut seen: Vec<String> = Vec::new();
    let mut kept: Vec<PerceptionLabel> = Vec::new();
    for entry in sorted {
        let key = entry.label.to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        kept.push(entry);
        if kept.len() == MAX_PERCEPTION_LABELS {
            break;
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(text: &str, confidence: f32) -> PerceptionLabel {
        PerceptionLabel {
            label: text.to_string(),
            confidence,
        }
    }

    #[test]
    fn a_success_payload_parses_into_bounded_labels() {
        let parsed = parse(
            r#"{"schema_version":"fm-rename-v1","status":"success",
                "perception":{"labels":[{"label":"sky","confidence":0.039},
                {"label":"sunset_sunrise","confidence":0.038}]}}"#,
        )
        .expect("a success payload parses");
        assert_eq!(
            parsed.labels,
            vec![label("sky", 0.039), label("sunset_sunrise", 0.038)]
        );
    }

    #[test]
    fn labels_are_ranked_bounded_and_deduped() {
        let mut labels: Vec<PerceptionLabel> = (0..15)
            .map(|index| label(&format!("label-{index}"), 0.02 + index as f32 * 0.01))
            .collect();
        labels.push(label("Sunset_Sunrise", 0.99));
        labels.push(label("sunset_sunrise", 0.98));
        labels.push(label("   ", 0.97));
        labels.push(label("below-floor", 0.001));
        labels.push(label(&"x".repeat(200), 0.96));
        let bounded = bound_labels(labels);
        assert_eq!(bounded.labels.len(), MAX_PERCEPTION_LABELS);
        assert_eq!(bounded.labels[0].label, "Sunset_Sunrise");
        assert_eq!(
            bounded
                .labels
                .iter()
                .filter(|entry| entry.label.eq_ignore_ascii_case("sunset_sunrise"))
                .count(),
            1,
            "the case-insensitive duplicate is dropped"
        );
        assert!(
            bounded
                .labels
                .iter()
                .all(|entry| entry.confidence >= MIN_PERCEPTION_CONFIDENCE),
            "the long tail stays out"
        );
        assert!(
            bounded
                .labels
                .iter()
                .all(|entry| entry.label.chars().count() <= MAX_PERCEPTION_LABEL_CHARS),
            "every label is bounded"
        );
        let ranked: Vec<f32> = bounded
            .labels
            .iter()
            .map(|entry| entry.confidence)
            .collect();
        let mut expected = ranked.clone();
        expected.sort_by(|left, right| right.partial_cmp(left).unwrap_or(Ordering::Equal));
        assert_eq!(ranked, expected, "the ranking is highest confidence first");
    }

    #[test]
    fn ocr_text_parses_alongside_labels_and_old_payloads_stay_empty() {
        let parsed = parse(
            r#"{"schema_version":"fm-rename-v1","status":"success",
                "perception":{"labels":[{"label":"sign","confidence":0.5}],
                "text":["OPEN", "9:00 - 18:00"]}}"#,
        )
        .expect("a text payload parses");
        assert_eq!(parsed.labels, vec![label("sign", 0.5)]);
        assert_eq!(parsed.text, vec!["OPEN", "9:00 - 18:00"]);
        assert!(!parsed.is_empty());
        // Older bridge answers without the key keep working (empty text).
        let legacy = parse(
            r#"{"schema_version":"fm-rename-v1","status":"success",
                "perception":{"labels":[{"label":"sky","confidence":0.9}]}}"#,
        )
        .expect("a legacy payload parses");
        assert!(legacy.text.is_empty());
    }

    #[test]
    fn ocr_text_is_trimmed_ascii_bounded_and_deduped() {
        let fragments = vec![
            "  OPEN  ".to_string(),
            "open".to_string(),
            String::new(),
            "   ".to_string(),
            "caf\u{e9}".to_string(),
            "x".repeat(500),
        ];
        let mut many = fragments;
        many.extend((0..20).map(|index| format!("sign-{index}")));
        let bounded = bound_texts(many);
        assert_eq!(bounded.len(), MAX_OCR_TEXTS, "the budget holds");
        assert_eq!(bounded[0], "OPEN", "trimmed, first spelling wins");
        assert!(
            bounded.iter().all(|fragment| fragment.is_ascii()),
            "non-ASCII contributes nothing"
        );
        assert!(
            bounded
                .iter()
                .all(|fragment| fragment.chars().count() <= MAX_OCR_CHARS),
            "every fragment is bounded"
        );
        assert_eq!(
            bounded
                .iter()
                .filter(|fragment| fragment.eq_ignore_ascii_case("open"))
                .count(),
            1,
            "the case-insensitive duplicate is dropped"
        );
    }

    #[test]
    fn failure_payloads_are_classified() {
        let unavailable = parse(
            r#"{"schema_version":"fm-rename-v1","status":"error",
                "error":{"kind":"unavailable","reason":"vision-framework-missing"}}"#,
        );
        assert_eq!(
            unavailable,
            Err(PerceptionError::Unavailable {
                reason: "vision-framework-missing".to_string()
            })
        );
        let failed = parse(
            r#"{"schema_version":"fm-rename-v1","status":"error",
                "error":{"kind":"image-undecodable","detail":"x.png"}}"#,
        );
        assert_eq!(
            failed,
            Err(PerceptionError::Failed {
                detail: "image-undecodable: x.png".to_string()
            })
        );
    }

    #[test]
    fn malformed_payloads_never_become_a_silent_success() {
        assert!(matches!(
            parse("not json"),
            Err(PerceptionError::Malformed { .. })
        ));
        assert!(matches!(
            parse(r#"{"schema_version":"v0","status":"success"}"#),
            Err(PerceptionError::Malformed { .. })
        ));
        assert!(matches!(
            parse(r#"{"schema_version":"fm-rename-v1","status":"maybe"}"#),
            Err(PerceptionError::Malformed { .. })
        ));
        assert!(matches!(
            parse(r#"{"schema_version":"fm-rename-v1","status":"error"}"#),
            Err(PerceptionError::Malformed { .. })
        ));
        assert!(matches!(
            parse(r#"{"schema_version":"fm-rename-v1","status":"error","error":{"reason":"r"}}"#),
            Err(PerceptionError::Malformed { .. })
        ));
        // A label missing its confidence is skipped, not invented.
        let partial = parse(
            r#"{"schema_version":"fm-rename-v1","status":"success",
                "perception":{"labels":[{"label":"sun"},{"confidence":0.5},
                {"label":"sky","confidence":0.9}]}}"#,
        )
        .expect("partial labels do not fail the parse");
        assert_eq!(partial.labels, vec![label("sky", 0.9)]);
    }
}

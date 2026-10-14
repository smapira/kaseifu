//! Video perception through AVFoundation (original spec sections 4.4, 5.3 and
//! 11.4).
//!
//! The video file never crosses the FFI: Rust passes the path, the Swift
//! bridge opens it read-only, measures the metadata (duration, resolution,
//! frame rate, creation date) and classifies a bounded set of representative
//! frames with the same Vision recipe the image path uses. The frames stay in
//! memory: nothing is written, re-encoded or thumbnailed to disk.
//!
//! Rust owns the merge: per-frame labels become one bounded label set
//! ([`merge_frames`]), which then feeds the existing `input.perception` path,
//! so the Foundation Models prompt and the naming rules stay exactly the ones
//! the image round established. The original spec section 5.3 asks for this
//! shape explicitly: when several images cannot be given to the model at
//! once, analyze the frames individually and integrate the results.

use serde::Deserialize;

use super::contract;
use super::perception::{self, PerceptionError};
use crate::models::file_context::{PerceptionInfo, PerceptionLabel, VideoInfo};

/// Maximum number of representative frames Rust accepts from the bridge.
///
/// The bridge itself sends 3-5 frames (original spec section 5.3) and never
/// more than [`MAX_VIDEO_FRAMES`]; a bridge that sends more is cut here so the
/// frame budget can never grow the prompt, the report or the journal
/// (original spec section 11.4).
pub const MAX_VIDEO_FRAMES: usize = 5;
/// Character budget for the video creation timestamp.
///
/// QuickTime creation dates are short ASCII (`2026-10-14T09:30:00+0900`); the
/// bound only keeps a broken container from carrying an unbounded string into
/// the prompt.
pub const MAX_VIDEO_DATE_CHARS: usize = 64;

/// One frame's classification result.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameLabels {
    /// Position of the frame in the video, in seconds.
    pub time_seconds: f64,
    /// Bounded labels the classifier reported for this frame.
    pub labels: Vec<PerceptionLabel>,
}

/// One video perception answer: metadata plus the per-frame labels.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoPerception {
    pub info: VideoInfo,
    pub frames: Vec<FrameLabels>,
}

impl VideoPerception {
    /// The labels every frame was merged into.
    pub fn merged_labels(&self) -> PerceptionInfo {
        merge_frames(&self.frames)
    }
}

/// Merge per-frame labels into one bounded set.
///
/// A label the classifier reports in several frames is the same visual
/// content, so the strongest single observation (the maximum confidence) is
/// kept: summing would grow with the frame count, and averaging would punish
/// evidence only one frame carries. The merged set then goes through the
/// shared [`perception::bound_labels`], so ranking, the confidence floor,
/// de-duplication, the count budget and the length budget are exactly the
/// rules the image path fixed.
///
/// Nothing here decides a file name: the merged labels are data for the
/// model, and the naming rule engine stays in Rust.
pub fn merge_frames(frames: &[FrameLabels]) -> PerceptionInfo {
    let mut best: Vec<PerceptionLabel> = Vec::new();
    for frame in frames {
        for entry in &frame.labels {
            keep_strongest(&mut best, entry);
        }
    }
    perception::bound_labels(best)
}

/// Keep `entry` as the strongest observation of its label text.
fn keep_strongest(best: &mut Vec<PerceptionLabel>, entry: &PerceptionLabel) {
    let key = entry.label.to_ascii_lowercase();
    let existing = best
        .iter_mut()
        .find(|current| current.label.to_ascii_lowercase() == key);
    match existing {
        Some(current) if entry.confidence > current.confidence => {
            current.confidence = entry.confidence;
        }
        Some(_) => {}
        None => best.push(entry.clone()),
    }
}

/// Parse one video perception payload and bound its frames.
///
/// Same envelope rules as the image path: a version mismatch, an unknown
/// status or a payload that is not the contract is an explicit error, never a
/// silent success; a value of the wrong type is treated as missing (labels
/// keep the image rule: a missing field is skipped, a wrongly typed one is an
/// explicit error, never an invented value).
pub fn parse(json: &str) -> Result<VideoPerception, PerceptionError> {
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
        "success" => assemble(envelope),
        "error" => Err(perception::classify_failure(envelope.error)),
        other => Err(PerceptionError::Malformed {
            detail: format!("unknown status: {other}"),
        }),
    }
}

/// One parsed envelope (the shapes the bridge answers with).
#[derive(Deserialize)]
struct Envelope {
    schema_version: String,
    status: String,
    #[serde(default)]
    video: Option<RawVideo>,
    #[serde(default)]
    perception: Option<RawFrames>,
    #[serde(default)]
    error: Option<perception::Failure>,
}

#[derive(Deserialize)]
struct RawVideo {
    #[serde(default)]
    duration_seconds: Option<serde_json::Value>,
    #[serde(default)]
    width: Option<serde_json::Value>,
    #[serde(default)]
    height: Option<serde_json::Value>,
    #[serde(default)]
    frame_rate: Option<serde_json::Value>,
    #[serde(default)]
    creation_date: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawFrames {
    #[serde(default)]
    frames: Vec<RawFrame>,
}

#[derive(Deserialize)]
struct RawFrame {
    #[serde(default)]
    time_seconds: Option<serde_json::Value>,
    #[serde(default)]
    labels: Vec<perception::RawLabel>,
}

/// Turn a success envelope into bounded video facts.
///
/// A success without one usable frame violates the contract: the bridge
/// answers an error for a video it could not perceive, so no proposal is
/// built on an empty observation.
fn assemble(envelope: Envelope) -> Result<VideoPerception, PerceptionError> {
    let frames: Vec<FrameLabels> = envelope
        .perception
        .unwrap_or(RawFrames { frames: Vec::new() })
        .frames
        .into_iter()
        .take(MAX_VIDEO_FRAMES)
        .map(usable_frame)
        .collect();
    if frames.is_empty() {
        return Err(PerceptionError::Malformed {
            detail: "a successful answer carried no representative frame".to_string(),
        });
    }
    Ok(VideoPerception {
        info: sanitize(envelope.video),
        frames,
    })
}

/// Bound one frame's labels with the shared image rules.
fn usable_frame(raw: RawFrame) -> FrameLabels {
    FrameLabels {
        time_seconds: as_positive(raw.time_seconds.as_ref()).unwrap_or(0.0),
        labels: perception::bound_labels(
            raw.labels.into_iter().filter_map(perception::usable_label),
        )
        .labels,
    }
}

/// Keep only the values the container actually reported.
///
/// A value of the wrong type is treated as missing (the contract rule), never
/// as a reason to guess or to fail the whole answer: metadata is
/// informational, so a broken field drops out and the frames still carry the
/// perception.
fn sanitize(raw: Option<RawVideo>) -> VideoInfo {
    let Some(raw) = raw else {
        return VideoInfo::default();
    };
    VideoInfo {
        duration_seconds: as_positive(raw.duration_seconds.as_ref()),
        width: as_dimension(raw.width.as_ref()),
        height: as_dimension(raw.height.as_ref()),
        frame_rate: as_positive(raw.frame_rate.as_ref())
            .and_then(|value| (value as f32).is_finite().then_some(value as f32)),
        creation_date: as_text(raw.creation_date.as_ref())
            .map(|value| contract::bound_chars(&value, MAX_VIDEO_DATE_CHARS)),
    }
}

/// Keep a reported positive number (anything else stays missing).
fn as_positive(value: Option<&serde_json::Value>) -> Option<f64> {
    let number = value?.as_f64()?;
    (number.is_finite() && number > 0.0).then_some(number)
}

/// Keep a reported positive integer dimension.
fn as_dimension(value: Option<&serde_json::Value>) -> Option<u32> {
    let number = value?.as_u64()?;
    u32::try_from(number).ok().filter(|value| *value > 0)
}

/// Keep a reported non-empty text value.
fn as_text(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?.trim().to_string();
    (!text.is_empty()).then_some(text)
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

    fn frame(time: f64, labels: Vec<PerceptionLabel>) -> FrameLabels {
        FrameLabels {
            time_seconds: time,
            labels,
        }
    }

    fn success(frames: &str, video: &str) -> String {
        format!(
            r#"{{"schema_version":"fm-rename-v1","status":"success","video":{video},
                "perception":{{"frames":[{frames}]}}}}"#
        )
    }

    #[test]
    fn a_success_payload_parses_metadata_and_frames() {
        let json = success(
            r#"{"time_seconds":12.0,"labels":[{"label":"ocean","confidence":0.71},{"label":"beach","confidence":0.4}]},
               {"time_seconds":36.0,"labels":[{"label":"beach","confidence":0.62}]}"#,
            r#"{"duration_seconds":120.0,"width":1920,"height":1080,"frame_rate":29.97,
                "creation_date":"2026-10-14T09:30:00+0900"}"#,
        );
        let parsed = parse(&json).expect("a success payload parses");
        assert_eq!(parsed.info.duration_seconds, Some(120.0));
        assert_eq!(parsed.info.width, Some(1920));
        assert_eq!(parsed.info.height, Some(1080));
        assert_eq!(parsed.info.frame_rate, Some(29.97));
        assert_eq!(
            parsed.info.creation_date.as_deref(),
            Some("2026-10-14T09:30:00+0900")
        );
        assert_eq!(parsed.frames.len(), 2);
        assert_eq!(parsed.frames[0].time_seconds, 12.0);
        assert_eq!(
            parsed.frames[0].labels,
            vec![label("ocean", 0.71), label("beach", 0.4)]
        );
    }

    #[test]
    fn merging_keeps_the_strongest_observation_of_every_label() {
        let merged = merge_frames(&[
            frame(0.0, vec![label("sky", 0.30), label("moon", 0.10)]),
            frame(
                1.0,
                vec![
                    label("sky", 0.42),
                    label("sunset_sunrise", 0.20),
                    label("Moon", 0.05),
                ],
            ),
        ]);
        let texts: Vec<&str> = merged
            .labels
            .iter()
            .map(|entry| entry.label.as_str())
            .collect();
        assert_eq!(texts, vec!["sky", "sunset_sunrise", "moon"]);
        assert_eq!(merged.labels[0].confidence, 0.42, "the maximum wins");
        assert_eq!(merged.labels[2].confidence, 0.10);
    }

    #[test]
    fn merging_bounds_the_union_of_many_frames() {
        let frames: Vec<FrameLabels> = (0..MAX_VIDEO_FRAMES)
            .map(|frame_index| {
                frame(
                    frame_index as f64,
                    (0..13)
                        .map(|index| {
                            label(
                                &format!("label-{index}"),
                                0.5 + (frame_index as f32) / 100.0,
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        let merged = merge_frames(&frames);
        assert_eq!(merged.labels.len(), perception::MAX_PERCEPTION_LABELS);
        let ranked: Vec<f32> = merged.labels.iter().map(|entry| entry.confidence).collect();
        assert!(
            ranked.windows(2).all(|pair| pair[0] >= pair[1]),
            "the merged set stays ranked: {ranked:?}"
        );
    }

    #[test]
    fn merge_without_frames_is_empty() {
        assert!(merge_frames(&[]).labels.is_empty());
    }

    #[test]
    fn the_frame_budget_cuts_a_greedy_bridge() {
        let frames = (0..8)
            .map(|index| format!(r#"{{"time_seconds":{index}.0,"labels":[]}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let parsed = parse(&success(&frames, "{}")).expect("parses");
        assert_eq!(parsed.frames.len(), MAX_VIDEO_FRAMES);
    }

    #[test]
    fn wrong_types_are_missing_and_values_are_sanitized() {
        let json = success(
            r#"{"time_seconds":"soon","labels":[{"label":"sky"},{"label":"ocean","confidence":0.5}]}"#,
            r#"{"duration_seconds":0,"width":"wide","height":0,"frame_rate":-1,"creation_date":"  "}"#,
        );
        let parsed = parse(&json).expect("wrong types do not fail the parse");
        assert!(parsed.info.is_empty(), "{:?}", parsed.info);
        assert_eq!(parsed.frames[0].time_seconds, 0.0);
        assert_eq!(parsed.frames[0].labels, vec![label("ocean", 0.5)]);
    }

    #[test]
    fn a_wrongly_typed_label_never_becomes_a_guessed_value() {
        // Labels keep the image rule: a missing field is skipped, a value of
        // the wrong type is an explicit contract error, never an invention.
        let json = success(
            r#"{"time_seconds":1.0,"labels":[{"label":"sky","confidence":"high"}]}"#,
            "{}",
        );
        assert!(matches!(
            parse(&json),
            Err(PerceptionError::Malformed { .. })
        ));
    }

    #[test]
    fn a_success_without_frames_is_malformed() {
        let json = success("", "{}");
        assert!(matches!(
            parse(&json),
            Err(PerceptionError::Malformed { .. })
        ));
    }

    #[test]
    fn failure_payloads_are_classified() {
        let audio_only = parse(
            r#"{"schema_version":"fm-rename-v1","status":"error",
                "error":{"kind":"video-no-video-track","detail":"clip.mp4"}}"#,
        );
        assert_eq!(
            audio_only,
            Err(PerceptionError::Failed {
                detail: "video-no-video-track: clip.mp4".to_string()
            })
        );
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
    }
}

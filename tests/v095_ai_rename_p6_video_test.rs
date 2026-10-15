//! v0.9.5 AI rename Phase 6 (video rename through AVFoundation + Vision).
//!
//! Model-independent tests only: the bridge is always a `FakeFmBridge` unless
//! a test says otherwise, so nothing here needs Apple Intelligence. The real
//! extraction path runs against two committed synthetic clips under
//! `tests/fixtures/video` (see the README there); those tests accept an
//! explicit `unavailable` answer from a stub build (no Swift toolchain or no
//! Foundation Models SDK) the same way the Phase 5b suite does.
//!
//! Filesystem fixtures live under the system temp directory; the developer's
//! videos, journal, Keychain and completion directories are never touched.

use kaseifu::models::file_context::{PerceptionLabel, VideoInfo};
use kaseifu::operations::undo::undo_last_move;
use kaseifu::rename::document::classify_document;
use kaseifu::rename::rules::resolve_video_date;
use kaseifu::rename::service::{
    PerceptionReport, PlanOutcome, RenameOptions, execute_plan, plan_rename,
};
use kaseifu::rename::store::RenameStore;
use kaseifu::rename::video::{classify_video, is_video};
use kaseifu::semantic::bridge::{FmBridge, SwiftFmBridge};
use kaseifu::semantic::contract::SemanticInfo;
use kaseifu::semantic::fake::{FakeFmBridge, FakeSemanticProvider};
use kaseifu::semantic::perception::{MAX_PERCEPTION_LABELS, PerceptionError};
use kaseifu::semantic::provider::{FoundationModelsProvider, SemanticProvider};
use kaseifu::semantic::video::{FrameLabels, MAX_VIDEO_FRAMES, merge_frames};
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

// ---------------------------------------------------------------------------
// Fixtures (synthetic only, temp dir only)
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v095-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("fixture file");
    path
}

/// One committed synthetic clip (read-only: nothing here writes to it).
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(name)
}

/// The bytes of the tiny clip (copied into temp dirs for plan tests).
fn video_bytes() -> Vec<u8> {
    std::fs::read(fixture("tiny.mp4")).expect("the tiny fixture is committed")
}

/// A minimal PNG (signature plus IHDR): enough for the image path, which
/// never decodes pixels in Rust and uses the fake bridge here.
fn fixture_png() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&64u32.to_be_bytes());
    out.extend_from_slice(&48u32.to_be_bytes());
    out.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

fn info(title: &str) -> SemanticInfo {
    SemanticInfo {
        title: Some(title.to_string()),
        category: None,
        project: None,
    }
}

fn pattern_options(pattern: &str) -> RenameOptions {
    RenameOptions {
        pattern: Some(pattern.to_string()),
        ..RenameOptions::default()
    }
}

fn labels(pairs: &[(&str, f32)]) -> Vec<PerceptionLabel> {
    pairs
        .iter()
        .map(|(label, confidence)| PerceptionLabel {
            label: (*label).to_string(),
            confidence: *confidence,
        })
        .collect()
}

fn frame(time: f64, pairs: &[(&str, f32)]) -> FrameLabels {
    FrameLabels {
        time_seconds: time,
        labels: labels(pairs),
    }
}

/// What the bridge reports for a 120 second clip (the original spec example).
fn clip_info() -> VideoInfo {
    VideoInfo {
        duration_seconds: Some(120.0),
        width: Some(1920),
        height: Some(1080),
        frame_rate: Some(29.97),
        creation_date: Some("2026-10-14T09:30:00+0900".to_string()),
    }
}

/// Three frames whose labels overlap: the merged set must keep the strongest
/// observation of every label (ocean 0.7, beach 0.6, sky 0.3).
fn clip_frames() -> Vec<FrameLabels> {
    vec![
        frame(12.0, &[("ocean", 0.5), ("sky", 0.2)]),
        frame(36.0, &[("ocean", 0.7), ("beach", 0.4)]),
        frame(60.0, &[("beach", 0.6), ("sky", 0.3)]),
    ]
}

// ---------------------------------------------------------------------------
// W1: the metadata and the merged labels reach the model and the report
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v095_a_video_plan_sends_metadata_and_merged_labels() {
    let dir = work_dir("plan");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge =
        FakeFmBridge::answering(info("海辺の風景")).perceiving_video(clip_info(), clip_frames());
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "2026-10-14_海辺の風景.mp4");
    assert_eq!(plan.date_source, "recorded", "the container date names it");
    let video = plan.video.expect("a video report");
    assert_eq!(
        video.frames, 3,
        "every frame the bridge classified is counted"
    );
    assert_eq!(video.info.duration_seconds, Some(120.0));
    assert_eq!(video.info.width, Some(1920));
    assert_eq!(video.info.height, Some(1080));
    assert!(
        video
            .summary()
            .contains("120.0s, 1920x1080, 30.0 fps, frames 3"),
        "{}",
        video.summary()
    );
    assert_eq!(
        plan.perception,
        Some(PerceptionReport::Labels {
            labels: labels(&[("ocean", 0.7), ("beach", 0.6), ("sky", 0.3)]),
            text: Vec::new(),
        }),
        "the frames merged into one ranked set"
    );
    assert_eq!(bridge.video_perceived_paths(), vec![target.clone()]);
    assert!(
        bridge.perceived_paths().is_empty(),
        "a video is never handed to the image classifier"
    );
    let requests = bridge.requests();
    assert_eq!(requests.len(), 1);
    let request: serde_json::Value = serde_json::from_str(&requests[0]).expect("request JSON");
    assert_eq!(request["schema_version"], serde_json::json!("fm-rename-v1"));
    assert_eq!(request["target_type"], serde_json::json!("video"));
    assert_eq!(
        request["input"]["video"]["duration_seconds"],
        serde_json::json!(120.0)
    );
    assert_eq!(request["input"]["video"]["width"], serde_json::json!(1920));
    assert_eq!(
        request["input"]["video"]["creation_date"],
        serde_json::json!("2026-10-14T09:30:00+0900")
    );
    assert!(
        request["input"].get("image").is_none(),
        "a video request carries no picture facts"
    );
    let sent: Vec<&str> = request["input"]["perception"]["labels"]
        .as_array()
        .expect("labels travel to the model")
        .iter()
        .map(|entry| entry["label"].as_str().expect("label"))
        .collect();
    assert_eq!(sent, vec!["ocean", "beach", "sky"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v095_merging_reuses_the_image_label_budget() {
    let mut frames: Vec<FrameLabels> = Vec::new();
    for frame_index in 0..MAX_VIDEO_FRAMES {
        let entries: Vec<PerceptionLabel> = (0..20)
            .map(|index| PerceptionLabel {
                label: format!("label-{index}"),
                confidence: 0.5 + frame_index as f32 / 100.0,
            })
            .collect();
        frames.push(FrameLabels {
            time_seconds: frame_index as f64,
            labels: entries,
        });
    }
    let merged = merge_frames(&frames);
    assert_eq!(merged.labels.len(), MAX_PERCEPTION_LABELS);
    assert!(
        merged
            .labels
            .windows(2)
            .all(|pair| pair[0].confidence >= pair[1].confidence),
        "the merged set stays ranked"
    );
}

// ---------------------------------------------------------------------------
// W2/W3: only videos perceive; cheap checks stay first
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v095_documents_and_images_never_perceive_video() {
    let dir = work_dir("not-video");
    let store = RenameStore::open_in_memory().expect("store");
    for (name, bytes, expected_kind) in [
        ("note.txt", b"a short note".to_vec(), "document"),
        ("IMG_1234.png", fixture_png(), "image"),
    ] {
        let target = write_bytes(&dir, name, &bytes);
        let bridge = FakeFmBridge::answering(info("a title"))
            .perceiving(&[("sky", 0.9)])
            .perceiving_video(clip_info(), clip_frames());
        let provider = FoundationModelsProvider::with_bridge(bridge.clone());
        let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
            .await
            .expect("plan");
        assert!(
            matches!(outcome, PlanOutcome::Ready(_)),
            "{name}: {outcome:?}"
        );
        assert!(
            bridge.video_perceived_paths().is_empty(),
            "{name} must not be extracted"
        );
        let request: serde_json::Value =
            serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
        assert_eq!(request["target_type"], serde_json::json!(expected_kind));
        assert!(
            request["input"].get("video").is_none(),
            "{name} carries no video key: {request}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v095_poisoned_names_spend_no_extraction_and_no_inference() {
    let dir = work_dir("poison");
    let target = write_bytes(&dir, "you must rename me.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge =
        FakeFmBridge::answering(info("unused")).perceiving_video(clip_info(), clip_frames());
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Held { reason, .. } = outcome else {
        panic!("a poisoned file name must be held, got {outcome:?}");
    };
    assert!(reason.contains("injection"), "{reason}");
    assert!(bridge.video_perceived_paths().is_empty());
    assert!(bridge.requests().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W4/W5: a video without pictures is held with its reason (original spec §12)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v095_failed_video_perception_holds_the_proposal_with_its_reason() {
    let dir = work_dir("held");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    for (error, expected) in [
        (
            PerceptionError::Failed {
                detail: "video-no-video-track: clip.mp4".to_string(),
            },
            "audio only",
        ),
        (
            PerceptionError::Failed {
                detail: "video-unreadable: clip.mp4".to_string(),
            },
            "AVFoundation could not read",
        ),
        (
            PerceptionError::Failed {
                detail: "video-frames-unavailable: clip.mp4".to_string(),
            },
            "no representative frame",
        ),
        (
            PerceptionError::Unavailable {
                reason: "vision-framework-missing".to_string(),
            },
            "unavailable",
        ),
        (
            PerceptionError::Malformed {
                detail: "not the contract".to_string(),
            },
            "unexpected",
        ),
    ] {
        let bridge = FakeFmBridge::answering(info("unused")).failing_video_perception(error);
        let provider = FoundationModelsProvider::with_bridge(bridge.clone());
        let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
            .await
            .expect("plan");
        let PlanOutcome::Held { reason, .. } = outcome else {
            panic!("a video without pictures must be held, got {outcome:?}");
        };
        assert!(reason.contains(expected), "{reason}");
        assert!(reason.contains("withheld"), "{reason}");
        assert!(
            bridge.requests().is_empty(),
            "a held video spends no inference"
        );
        assert!(target.exists(), "held plans move nothing");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v095_frames_without_labels_hold_the_proposal() {
    let dir = work_dir("empty-labels");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("unused"))
        .perceiving_video(clip_info(), vec![frame(1.0, &[]), frame(2.0, &[])]);
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let PlanOutcome::Held { reason, .. } = outcome else {
        panic!("a video nothing could be named in must be held, got {outcome:?}");
    };
    assert!(reason.contains("no visual content"), "{reason}");
    assert!(bridge.requests().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v095_model_unavailability_still_fails_explicitly() {
    let dir = work_dir("unavailable");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::unavailable("appleIntelligenceNotEnabled")
        .with_video(clip_info(), clip_frames());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02")).await;
    assert!(
        matches!(
            outcome,
            Err(kaseifu::rename::service::RenameError::ModelUnavailable(_))
        ),
        "got {outcome:?}"
    );
    assert!(target.exists(), "failed plans move nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W6/W7: the copy guard and the date rule
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v095_a_copied_label_is_held_and_a_description_passes() {
    let dir = work_dir("echo");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    for copied in ["ocean", "Ocean", "ocean_"] {
        let bridge =
            FakeFmBridge::answering(info(copied)).perceiving_video(clip_info(), clip_frames());
        let provider = FoundationModelsProvider::with_bridge(bridge.clone());
        let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
            .await
            .expect("plan");
        let PlanOutcome::Held { reason, .. } = outcome else {
            panic!("a copied label must be held, got {outcome:?} for {copied}");
        };
        assert!(reason.contains("perception label"), "{reason}");
        assert!(target.exists(), "held plans move nothing");
    }
    let bridge =
        FakeFmBridge::answering(info("海辺の風景")).perceiving_video(clip_info(), clip_frames());
    let provider = FoundationModelsProvider::with_bridge(bridge);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    assert!(matches!(outcome, PlanOutcome::Ready(_)), "got {outcome:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v095_a_video_without_a_recording_date_does_not_claim_one() {
    let dir = work_dir("no-date");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let mut reported = clip_info();
    reported.creation_date = None;
    let bridge =
        FakeFmBridge::answering(info("海辺の風景")).perceiving_video(reported, clip_frames());
    let provider = FoundationModelsProvider::with_bridge(bridge);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_ne!(
        plan.date_source, "recorded",
        "nothing was recorded in the container"
    );
    assert!(plan.proposal.starts_with("20"), "{}", plan.proposal);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W8: the non-destructive rules hold on the video path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v095_execute_journals_a_move_row_and_undo_restores() {
    let dir = work_dir("execute");
    let target = write_bytes(&dir, "clip.mp4", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("journal");
    let bridge =
        FakeFmBridge::answering(info("海辺の風景")).perceiving_video(clip_info(), clip_frames());
    let provider = FoundationModelsProvider::with_bridge(bridge);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    let operation_id = execute_plan(&plan, &history).expect("execute");
    assert!(!operation_id.is_empty());
    assert!(!target.exists(), "source moved away");
    assert!(plan.destination.exists(), "destination created");
    assert!(plan.destination.to_string_lossy().ends_with(".mp4"));
    let rows = history.entries().expect("journal rows");
    assert_eq!(rows[0].operation, "move");
    assert_eq!(rows[0].result, "success");
    let restored = undo_last_move(&history).expect("undo");
    assert!(
        target.exists(),
        "undo restores {}",
        restored.restored_to.display()
    );
    assert!(!plan.destination.exists(), "undo removes the renamed file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v095_the_extension_and_the_stem_budget_are_untouched() {
    let dir = work_dir("extension");
    let target = write_bytes(&dir, "VID_0012.mov", &video_bytes());
    let store = RenameStore::open_in_memory().expect("store");
    let long = format!("{}ocean", "word ".repeat(30));
    let bridge = FakeFmBridge::answering(info(&long)).perceiving_video(clip_info(), clip_frames());
    let provider = FoundationModelsProvider::with_bridge(bridge);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert!(plan.proposal.ends_with(".mov"), "{}", plan.proposal);
    assert!(
        plan.proposal.chars().count() <= 80 + ".mov".len(),
        "{}",
        plan.proposal
    );
    assert!(!plan.proposal.contains(' '), "{}", plan.proposal);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W10 and the type predicates
// ---------------------------------------------------------------------------

#[test]
fn v095_non_video_paths_are_refused_before_avfoundation() {
    let provider = FoundationModelsProvider::production();
    for path in ["note.txt", "IMG_1234.png", "noext"] {
        let outcome = provider.perceive_video(Path::new(path));
        let Err(PerceptionError::Failed { detail }) = outcome else {
            panic!("expected a refusal for {path}, got {outcome:?}");
        };
        assert!(detail.contains("not a video"), "{detail}");
    }
}

#[test]
fn v095_video_extensions_route_and_others_do_not() {
    assert!(is_video(Path::new("VID_0012.MOV")));
    assert!(is_video(Path::new("a.m4v")));
    assert!(!is_video(Path::new("note.txt")));
    assert!(!is_video(Path::new("noext")));
    assert_eq!(classify_video(Path::new("clip.mp4")), Ok(()));
    assert!(classify_video(Path::new("clip.webm")).is_err());
}

#[test]
fn v095_the_document_classifier_names_videos_without_a_stale_claim() {
    let error = classify_document(Path::new("clip.mp4")).expect_err("a video is not a document");
    let text = error.to_string();
    assert!(text.contains("is a video, not a document"), "{text}");
    assert!(
        !text.contains("not available"),
        "the pre-Phase-6 claim is gone: {text}"
    );
}

#[test]
fn v095_video_dates_keep_the_recorded_wall_clock() {
    // QuickTime writes `+0900`; the calendar day of the recording stays the
    // day the camera saw, and no zone is invented (original spec section 6.5).
    let (date, preference) = resolve_video_date(
        Some("2026-10-15T00:00:00Z"),
        Some("2026-10-16T00:00:00Z"),
        Some("2026-10-14T23:30:00+0900"),
    );
    assert!(matches!(
        preference,
        kaseifu::rename::rules::DatePreference::Recorded
    ));
    assert_eq!(date.source, "recorded");
    assert_eq!(date.format("YYYY-MM-DD").as_deref(), Some("2026-10-14"));
}

// ---------------------------------------------------------------------------
// V4: the real bridge against the committed synthetic clips
// ---------------------------------------------------------------------------

#[test]
fn v095_the_real_bridge_measures_and_perceives_the_video_fixture() {
    let target = fixture("tiny.mp4");
    assert!(
        target.exists(),
        "the fixture is committed: {}",
        target.display()
    );
    match SwiftFmBridge::new().video_perceive(&target) {
        Ok(perceived) => {
            let duration = perceived.info.duration_seconds.expect("a duration");
            assert!((duration - 2.0).abs() <= 0.2, "duration {duration}");
            assert_eq!(perceived.info.width, Some(64));
            assert_eq!(perceived.info.height, Some(48));
            let rate = perceived.info.frame_rate.expect("a frame rate");
            assert!((rate - 10.0).abs() <= 0.5, "frame rate {rate}");
            let recorded = perceived
                .info
                .creation_date
                .clone()
                .expect("the recorded date");
            assert!(recorded.starts_with("2026-10-14T09:30:00"), "{recorded}");
            assert_eq!(perceived.frames.len(), 3, "a 2 second clip gets 3 frames");
            for frame in &perceived.frames {
                assert!(
                    frame.time_seconds > 0.0 && frame.time_seconds < duration,
                    "a frame sits inside the clip: {}",
                    frame.time_seconds
                );
                assert!(frame.labels.len() <= MAX_PERCEPTION_LABELS);
                assert!(
                    frame
                        .labels
                        .windows(2)
                        .all(|pair| pair[0].confidence >= pair[1].confidence),
                    "every frame stays ranked"
                );
                assert!(
                    frame
                        .labels
                        .iter()
                        .all(|entry| entry.label.chars().count() <= 64)
                );
            }
            let merged = perceived.merged_labels();
            assert!(merged.labels.len() <= MAX_PERCEPTION_LABELS);
            assert!(
                merged
                    .labels
                    .windows(2)
                    .all(|pair| pair[0].confidence >= pair[1].confidence),
                "the merged set stays ranked"
            );
        }
        Err(PerceptionError::Unavailable { reason }) => {
            // The stub build (no Swift toolchain, or no Vision): the video
            // layer reports itself unavailable, never a silent empty answer.
            assert!(!reason.is_empty(), "an unavailable layer names its reason");
        }
        Err(error) => panic!("the real bridge failed on the fixture: {error}"),
    }
}

#[test]
fn v095_the_real_bridge_refuses_an_audio_only_fixture() {
    let target = fixture("audio-only.mp4");
    match SwiftFmBridge::new().video_perceive(&target) {
        Err(PerceptionError::Failed { detail }) => {
            assert!(detail.starts_with("video-no-video-track"), "{detail}");
        }
        Err(PerceptionError::Unavailable { reason }) => {
            assert!(!reason.is_empty(), "an unavailable layer names its reason");
        }
        other => panic!("an audio-only container must not be perceived: {other:?}"),
    }
}

#[test]
fn v095_the_real_bridge_refuses_garbage_under_a_video_name() {
    let dir = work_dir("garbage");
    let target = write_bytes(&dir, "broken.mp4", b"not a video at all");
    match SwiftFmBridge::new().video_perceive(&target) {
        Err(PerceptionError::Failed { detail }) => {
            assert!(detail.starts_with("video-unreadable"), "{detail}");
            assert!(
                !detail.contains('\n'),
                "the detail stays one line: {detail}"
            );
        }
        Err(PerceptionError::Unavailable { reason }) => {
            assert!(!reason.is_empty(), "an unavailable layer names its reason");
        }
        other => panic!("garbage must not be perceived: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// S4: the Swift bridge stays read-only and bounded; the prompt stays ordered
// ---------------------------------------------------------------------------

fn swift_bridge_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("swift/kaseifu_fm_bridge.swift");
    std::fs::read_to_string(&path).expect("swift bridge source is readable")
}

#[test]
fn v095_the_swift_bridge_reads_videos_read_only_and_bounded() {
    let source = swift_bridge_source();
    for required in [
        "@_cdecl(\"kaseifu_video_perceive\")",
        "AVAssetImageGenerator(asset: asset)",
        "generator.appliesPreferredTrackTransform = true",
        "generator.maximumSize = CGSize(width: kfVisionMaxPixelSize, height: kfVisionMaxPixelSize)",
        "generator.requestedTimeToleranceBefore = CMTime(seconds: kfFrameToleranceSeconds, preferredTimescale: 600)",
        "generator.requestedTimeToleranceAfter = CMTime(seconds: kfFrameToleranceSeconds, preferredTimescale: 600)",
        "private let kfMinVideoFrames = 3",
        "private let kfMaxVideoFrames = 5",
        "private let kfFrameToleranceSeconds = 1.0",
        "private func kfVideoFrameCount(",
        "private func kfFrameSeconds(",
        "\"video-no-video-track\"",
        "\"video-unreadable\"",
        "\"video-frames-unavailable\"",
    ] {
        assert!(
            source.contains(required),
            "swift bridge must keep: {required}"
        );
    }
    // The video path never writes: no encoder, no destination, no reader loop.
    for forbidden in [
        "AVAssetWriter",
        "CGImageDestination",
        "FileManager",
        "writeToFile",
        "AVAssetReader",
        "write(",
    ] {
        assert!(
            !source.contains(forbidden),
            "the bridge must not contain {forbidden}"
        );
    }
    // One allocation rule for every string that crosses the boundary and one
    // label recipe shared by the image path and every video frame.
    assert_eq!(
        source
            .matches("@_cdecl(\"kaseifu_fm_free_result\")")
            .count(),
        1,
        "video perception reuses the existing free function"
    );
    assert_eq!(
        source.matches("kfLabels(request)").count(),
        2,
        "the image path and the frame path share one classifier recipe"
    );
}

#[test]
fn v095_the_prompt_carries_video_facts_before_the_signals() {
    let source = swift_bridge_source();
    let video = source
        .find("lines.append(\"video: ")
        .expect("the prompt carries the video facts");
    let signals = source
        .find("lines.append(\"signals:")
        .expect("the signals line stays");
    assert!(
        video < signals,
        "the video metadata stays before signals so the prompt budget cannot cut it"
    );
    assert!(
        source.contains("was sampled from a few representative frames of the video"),
        "the prompt says the labels came from sampled frames"
    );
    assert!(
        source.contains("Never state motion, sound or an event"),
        "the prompt forbids inventing motion or sound"
    );
    assert!(
        !source.contains("visual_labels: \\(kfData(entries"),
        "the prompt never renders the raw label objects"
    );
}

#[test]
fn v095_the_schema_documents_video_without_requiring_it() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas/fm-rename-v1.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("schema file"))
            .expect("valid JSON");
    let branches = schema["oneOf"].as_array().expect("oneOf");
    let request = branches
        .iter()
        .find(|branch| branch["title"] == serde_json::json!("Request"))
        .expect("the request branch exists");
    let required: Vec<&str> = request["required"]
        .as_array()
        .expect("required")
        .iter()
        .filter_map(|entry| entry.as_str())
        .collect();
    assert!(
        !required.contains(&"video"),
        "video stays an optional, compatible extension"
    );
    let video = &request["properties"]["input"]["properties"]["video"];
    assert_eq!(video["type"], serde_json::json!("object"));
    assert_eq!(
        video["properties"]["duration_seconds"]["type"],
        serde_json::json!("number")
    );
    assert_eq!(schema["$id"], serde_json::json!("fm-rename-v1"));
}

// ---------------------------------------------------------------------------
// W9: the CLI (isolated HOME, temp dir, no model calls on these paths)
// ---------------------------------------------------------------------------

fn isolated_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("kaseifu-v095-home-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("home dir");
    home
}

fn cli(home: &Path, args: &[&str]) -> Output {
    ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"))
        .args(args)
        .env("HOME", home)
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .output()
        .expect("kaseifu runs")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn v095_cli_holds_an_unreadable_video_with_a_reason() {
    let home = isolated_home("held");
    let dir = work_dir("cli-held");
    let clip = write_bytes(&dir, "clip.mp4", b"not a video at all");
    let output = cli(&home, &["rename", clip.to_str().expect("utf8")]);
    assert!(
        output.status.success(),
        "a held proposal is not an error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout_of(&output);
    assert!(text.contains("status: held"), "{text}");
    assert!(text.contains("withheld"), "{text}");
    assert!(
        text.contains("AVFoundation could not read the video")
            || text.contains("video perception is unavailable"),
        "the reason names the failure: {text}"
    );
    assert!(text.is_ascii(), "the report stays ASCII:\n{text}");
    assert!(!text.contains("Phase"), "no internal phase words:\n{text}");
    assert!(clip.exists(), "held plans move nothing");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v095_cli_json_keeps_its_keys_and_adds_video() {
    let home = isolated_home("json");
    let dir = work_dir("cli-json");
    let clip = write_bytes(&dir, "clip.mp4", b"not a video at all");
    let output = cli(
        &home,
        &["rename", clip.to_str().expect("utf8"), "--format", "json"],
    );
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_str(stdout_of(&output).trim()).expect("JSON");
    let rename = &value["rename"];
    for key in [
        "target",
        "current_name",
        "proposal",
        "status",
        "reason",
        "pattern",
        "date_source",
        "operation_id",
        "perception",
        "video",
    ] {
        assert!(rename.get(key).is_some(), "key {key} stays: {rename}");
    }
    assert_eq!(rename["status"], serde_json::json!("held"));
    assert!(
        rename["video"].is_null(),
        "a held video reports no facts yet"
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v095_cli_guidance_and_help_name_the_video_types() {
    let home = isolated_home("guidance");
    let output = cli(&home, &["rename"]);
    assert!(output.status.success());
    let guidance = stdout_of(&output);
    assert!(
        guidance.contains("videos: mp4, mov, m4v, 3gp, 3g2"),
        "{guidance}"
    );
    assert!(
        !guidance.contains("arrive later"),
        "the stale promise is gone: {guidance}"
    );
    let help = stdout_of(&cli(&home, &["rename", "--help"]));
    assert!(
        help.contains("Videos are mp4, mov, m4v, 3gp and 3g2"),
        "{help}"
    );
    assert!(help.is_ascii() && !help.contains('\u{2014}'), "{help}");
    let _ = std::fs::remove_dir_all(&home);
}

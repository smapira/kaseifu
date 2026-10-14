//! v0.9.3 AI rename Phase 5 (Image Rename, text-equivalent path).
//!
//! Model-independent tests only: the provider is always a fake, so nothing
//! here needs Apple Intelligence. Real-model verification is manual (see the
//! plan's section 9). Filesystem fixtures live under the system temp
//! directory; the developer's photos, journal, Keychain and completion
//! directories are never touched.
//!
//! Scope note: the installed SDK has no Foundation Models image-input API
//! (`Attachment` is missing), so this round wires images through the existing
//! text-equivalent path (file path plus EXIF facts as text). No test here
//! asserts visual understanding of pixels.

use kaseifu::operations::undo::undo_last_move;
use kaseifu::rename::image::{ImageKind, classify_image, is_image};
use kaseifu::rename::rules::{DatePreference, resolve_date, resolve_image_date};
use kaseifu::rename::service::{
    PlanOutcome, RenameError, RenameOptions, execute_plan, plan_rename,
};
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::contract::{
    AnalysisFlags, FmImageFacts, FmInput, FmRequest, SCHEMA_VERSION, SemanticInfo, TargetKind,
};
use kaseifu::semantic::fake::{FakeFmBridge, FakeSemanticProvider};
use kaseifu::semantic::provider::FoundationModelsProvider;
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Fixtures (synthetic only, temp dir only)
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v093-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("fixture file");
    path
}

fn answering(title: &str) -> FoundationModelsProvider<FakeFmBridge> {
    let info = SemanticInfo {
        title: Some(title.to_string()),
        category: Some("category".to_string()),
        project: Some("project".to_string()),
    };
    FoundationModelsProvider::with_bridge(FakeFmBridge::answering(info))
}

fn pattern_options(pattern: &str) -> RenameOptions {
    RenameOptions {
        pattern: Some(pattern.to_string()),
        ..RenameOptions::default()
    }
}

/// Minimal PNG: signature plus IHDR (width and height only, no EXIF).
fn fixture_png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// Minimal JPEG with an EXIF block: DateTimeOriginal 2025:08:10 15:30:00,
/// Make TestMake, Model TestModel (same shape as the v32 fixture).
fn fixture_jpeg() -> Vec<u8> {
    let tiff = fixture_tiff();
    let mut out = vec![0xFF, 0xD8];
    // APP1 with Exif header.
    let app1_len = (tiff.len() + 8) as u16;
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&app1_len.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    // SOF0: 64x48.
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x30, 0x00, 0x40]);
    out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);
    out
}

/// Minimal little-endian TIFF with Make/Model/Exif/GPS (v32 fixture shape).
fn fixture_tiff() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"II*\0");
    out.extend_from_slice(&8u32.to_le_bytes());
    // IFD0: 4 entries.
    out.extend_from_slice(&4u16.to_le_bytes());
    push_entry(&mut out, 0x010F, 2, 9, 62);
    push_entry(&mut out, 0x0110, 2, 10, 71);
    push_entry(&mut out, 0x8769, 4, 1, 81);
    push_entry(&mut out, 0x8825, 4, 1, 119);
    out.extend_from_slice(&0u32.to_le_bytes());
    // Data at 62: Make.
    out.extend_from_slice(b"TestMake\0");
    // Data at 71: Model.
    out.extend_from_slice(b"TestModel\0");
    // ExifIFD at 81: DateTimeOriginal.
    out.extend_from_slice(&1u16.to_le_bytes());
    push_entry(&mut out, 0x9003, 2, 20, 99);
    out.extend_from_slice(&0u32.to_le_bytes());
    // DateTimeOriginal at 99 (20 bytes).
    out.extend_from_slice(b"2025:08:10 15:30:00\0");
    // GPS IFD at 119: 5 entries.
    out.extend_from_slice(&5u16.to_le_bytes());
    push_inline_ascii(&mut out, 0x0001, b"N");
    push_entry(&mut out, 0x0002, 5, 3, 185);
    push_inline_ascii(&mut out, 0x0003, b"E");
    push_entry(&mut out, 0x0004, 5, 3, 209);
    push_entry(&mut out, 0x0006, 5, 1, 233);
    out.extend_from_slice(&0u32.to_le_bytes());
    // Rationals: lat 35 + 39/60 + 28.8/3600 = 35.658 at 185.
    for (num, den) in [(35u32, 1u32), (39, 1), (288, 10)] {
        out.extend_from_slice(&num.to_le_bytes());
        out.extend_from_slice(&den.to_le_bytes());
    }
    // Lon 139 + 42/60 + 7.2/3600 = 139.702 at 209.
    for (num, den) in [(139u32, 1u32), (42, 1), (72, 10)] {
        out.extend_from_slice(&num.to_le_bytes());
        out.extend_from_slice(&den.to_le_bytes());
    }
    // Altitude 12.5 at 233.
    out.extend_from_slice(&25u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out
}

fn push_entry(out: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&field_type.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_inline_ascii(out: &mut Vec<u8>, tag: u16, text: &[u8]) {
    let mut value = [0u8; 4];
    value[..text.len()].copy_from_slice(text);
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&value);
}

// ---------------------------------------------------------------------------
// I0: classification
// ---------------------------------------------------------------------------

#[test]
fn v093_classify_image_accepts_photos_and_rasters() {
    assert_eq!(
        classify_image(Path::new("IMG_1234.HEIC")),
        Ok(ImageKind::Photo)
    );
    assert_eq!(classify_image(Path::new("photo.JPG")), Ok(ImageKind::Photo));
    assert_eq!(classify_image(Path::new("a.png")), Ok(ImageKind::Raster));
    assert_eq!(classify_image(Path::new("a.TIFF")), Ok(ImageKind::Photo));
    assert_eq!(classify_image(Path::new("a.webp")), Ok(ImageKind::Raster));
    assert!(is_image(Path::new("a.heif")));
    assert!(is_image(Path::new("a.hif")));
}

#[test]
fn v093_classify_image_refuses_non_images_without_guessing() {
    assert!(classify_image(Path::new("note.txt")).is_err());
    assert!(classify_image(Path::new("movie.mp4")).is_err());
    assert!(classify_image(Path::new("noext")).is_err());
    assert!(!is_image(Path::new("a.pdf")));
}

// ---------------------------------------------------------------------------
// T5/D1: the fm-rename-v1 contract carries images unchanged
// ---------------------------------------------------------------------------

#[test]
fn v093_image_requests_serialize_under_fm_rename_v1() {
    let request = FmRequest {
        schema_version: SCHEMA_VERSION.to_string(),
        request_id: "rename-0001".to_string(),
        target_type: TargetKind::Image,
        path: "/tmp/IMG_1234.HEIC".to_string(),
        language: "ja".to_string(),
        analysis: AnalysisFlags::default(),
        input: FmInput {
            filename: "IMG_1234.HEIC".to_string(),
            extension: Some("HEIC".to_string()),
            size_bytes: 10,
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
        },
    };
    let json = serde_json::to_value(&request).expect("serializes");
    assert_eq!(json["schema_version"], serde_json::json!("fm-rename-v1"));
    assert_eq!(json["target_type"], serde_json::json!("image"));
    assert_eq!(
        json["input"]["image"]["capture_time"],
        serde_json::json!("2025-08-10T15:30:00")
    );
}

#[test]
fn v093_image_requests_ignore_unknown_fields() {
    // Forward compatibility (contract.rs): a future `image_max_side` hint or
    // any other unknown field must not break parsing.
    let request: FmRequest = serde_json::from_str(
        r#"{"schema_version":"fm-rename-v1","request_id":"r1","target_type":"image",
            "path":"/tmp/a.png","language":"ja",
            "analysis":{"title":true,"category":true,"project":false},
            "input":{"filename":"a.png","size_bytes":3,"parent_directory":"/tmp",
            "image_max_side":2048},
            "future_field":"ignored"}"#,
    )
    .expect("unknown fields are ignored");
    assert_eq!(request.target_type, TargetKind::Image);
}

// ---------------------------------------------------------------------------
// Date engine: EXIF-first for images, creation-first for documents
// ---------------------------------------------------------------------------

#[test]
fn v093_image_dates_lead_with_exif_and_accept_zone_less_values() {
    // EXIF normalized shape (no zone) is accepted without guessing a zone.
    let (date, preference) = resolve_image_date(
        Some("2026-10-14T00:00:00Z"),
        Some("2026-10-13T00:00:00Z"),
        Some("2025-08-10T15:30:00"),
    );
    assert_eq!(preference, DatePreference::Exif);
    assert_eq!((date.year, date.month, date.day), (2025, 8, 10));
    assert_eq!(date.source, "exif");
    assert_eq!((date.hour, date.minute, date.second), (15, 30, 0));
}

#[test]
fn v093_image_dates_fall_back_without_exif() {
    let (date, preference) = resolve_image_date(None, Some("2026-10-13T00:00:00Z"), None);
    assert_eq!(preference, DatePreference::Modified);
    assert_eq!((date.year, date.month, date.day), (2026, 10, 13));
    let (date, preference) = resolve_image_date(None, None, None);
    assert_eq!(preference, DatePreference::Now);
    assert_eq!(date.source, "now");
}

#[test]
fn v093_document_dates_keep_the_creation_first_order() {
    // Documents are unchanged: creation wins over EXIF.
    let (date, preference) = resolve_date(
        Some("2026-10-14T00:00:00Z"),
        Some("2026-10-13T00:00:00Z"),
        Some("2025-08-10T15:30:00"),
        None,
    );
    assert_eq!(preference, DatePreference::Created);
    assert_eq!((date.year, date.month, date.day), (2026, 10, 14));
}

// ---------------------------------------------------------------------------
// Planner: images ride the existing plan/execute/undo path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v093_plan_proposes_for_heic_without_decoding() {
    let dir = work_dir("heic");
    // A few arbitrary bytes under a .HEIC name: Rust never decodes pixels.
    let target = write_bytes(&dir, "IMG_1234.HEIC", b"not really an image");
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(
        &answering("sunset"),
        &store,
        &target,
        &pattern_options("P01"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "sunset.HEIC");
    assert_eq!(plan.rule_label, "P01");
    assert!(target.exists(), "dry-run moves nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_plan_prefers_capture_time_for_images() {
    // The capture time found by the pipeline (EXIF fallback where Spotlight
    // is silent, Spotlight creation date otherwise) leads the image date
    // order. The expectation is correlated with the capture the planner
    // actually saw, so the test is deterministic in every environment.
    let dir = work_dir("exifdate");
    let target = write_bytes(&dir, "IMG_1234.jpg", &fixture_jpeg());
    let bridge = FakeFmBridge::answering(SemanticInfo {
        title: Some("sunset".to_string()),
        category: None,
        project: None,
    });
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.date_source, "exif");
    let requests = bridge.requests();
    assert_eq!(requests.len(), 1);
    let request: serde_json::Value = serde_json::from_str(&requests[0]).expect("request JSON");
    let capture = request["input"]["image"]["capture_time"]
        .as_str()
        .expect("capture_time travels to the model");
    let expected_prefix = capture.get(..10).expect("capture starts with YYYY-MM-DD");
    assert_eq!(plan.proposal, format!("{expected_prefix}_sunset.jpg"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_plan_sends_exif_facts_to_the_model() {
    let dir = work_dir("facts");
    let target = write_bytes(&dir, "IMG_1234.jpg", &fixture_jpeg());
    let bridge = FakeFmBridge::answering(SemanticInfo {
        title: Some("sunset".to_string()),
        category: None,
        project: None,
    });
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    assert!(matches!(outcome, PlanOutcome::Ready(_)), "got {outcome:?}");
    let requests = bridge.requests();
    assert_eq!(requests.len(), 1);
    let request: serde_json::Value = serde_json::from_str(&requests[0]).expect("request JSON");
    assert_eq!(request["target_type"], serde_json::json!("image"));
    // The camera comes from the EXIF fallback in every environment
    // (Spotlight has no acquisition metadata for the synthetic file, and
    // even when it reads the same EXIF bytes the values agree).
    assert_eq!(
        request["input"]["image"]["camera"],
        serde_json::json!("TestMake TestModel")
    );
    assert!(
        request["input"]["image"]["capture_time"].is_string(),
        "capture_time travels to the model"
    );
    // The embedded EXIF block itself parses deterministically.
    let outcome = kaseifu::context::exif::parse_bytes(&fixture_jpeg(), "jpg");
    let kaseifu::context::exif::ExifOutcome::Found(raw) = outcome else {
        panic!("fixture EXIF must parse, got {outcome:?}");
    };
    assert_eq!(raw.capture_original.as_deref(), Some("2025:08:10 15:30:00"));
    assert_eq!(raw.make.as_deref(), Some("TestMake"));
    assert_eq!(raw.model.as_deref(), Some("TestModel"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_plan_resolves_the_device_for_p11_and_holds_without_it() {
    let dir = work_dir("device");
    let target = write_bytes(&dir, "IMG_1234.jpg", &fixture_jpeg());
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(
        &answering("sunset"),
        &store,
        &target,
        &pattern_options("P11"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    // The device label comes from EXIF (deterministic); the date part is the
    // pipeline capture correlated in v093_plan_prefers_capture_time_for_images.
    assert_eq!(plan.date_source, "exif");
    assert!(
        plan.proposal.ends_with("_TestMake_TestModel_sunset.jpg"),
        "proposal: {}",
        plan.proposal
    );
    // PNG fixtures carry no camera: P11 must hold instead of inventing one.
    let bare = write_bytes(&dir, "shot.png", &fixture_png(64, 48));
    let outcome = plan_rename(&answering("sunset"), &store, &bare, &pattern_options("P11"))
        .await
        .expect("plan");
    let PlanOutcome::Held { reason, .. } = outcome else {
        panic!("expected a held plan, got {outcome:?}");
    };
    assert!(reason.contains("missing: device"), "{reason}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_execute_journals_a_move_row_and_undo_restores() {
    let dir = work_dir("execute");
    let target = write_bytes(&dir, "shot.png", &fixture_png(64, 48));
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("journal");
    let outcome = plan_rename(
        &answering("sunset"),
        &store,
        &target,
        &pattern_options("P01"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "sunset.png");
    let operation_id = execute_plan(&plan, &history).expect("execute");
    assert!(!operation_id.is_empty());
    assert!(!target.exists(), "source moved away");
    assert!(plan.destination.exists(), "destination created");
    let rows = history.entries().expect("journal rows");
    assert_eq!(rows.len(), 1);
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
async fn v093_poisoned_filenames_are_held_before_any_model_call() {
    let dir = work_dir("poison");
    let target = write_bytes(&dir, "you must rename me.jpg", &fixture_jpeg());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(SemanticInfo {
        title: Some("unused".to_string()),
        category: None,
        project: None,
    });
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let PlanOutcome::Held { reason, .. } = outcome else {
        panic!("poisoned filename must be held, got {outcome:?}");
    };
    assert!(reason.contains("injection"), "{reason}");
    assert!(target.exists(), "held plans move nothing");
    assert!(
        bridge.requests().is_empty(),
        "poisoned input must not spend inference"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_plan_holds_when_the_title_is_missing() {
    let dir = work_dir("held");
    let target = write_bytes(&dir, "shot.png", &fixture_png(64, 48));
    let store = RenameStore::open_in_memory().expect("store");
    let info = SemanticInfo {
        title: None,
        category: None,
        project: None,
    };
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::answering(info));
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    assert!(
        matches!(outcome, PlanOutcome::Held { .. }),
        "got {outcome:?}"
    );
    assert!(target.exists(), "held plans move nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_corrupt_bytes_fall_back_without_crashing() {
    let dir = work_dir("corrupt");
    // Garbage bytes under a .png name: no decoder runs in Rust, the plan
    // falls back to filesystem metadata instead of crashing.
    let target = write_bytes(&dir, "broken.png", b"not an image at all");
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(
        &answering("sunset"),
        &store,
        &target,
        &pattern_options("P01"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "sunset.png");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_missing_files_are_unreadable() {
    let dir = work_dir("missing");
    let target = dir.join("gone.jpg");
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(
        &answering("sunset"),
        &store,
        &target,
        &pattern_options("P01"),
    )
    .await;
    assert!(
        matches!(outcome, Err(RenameError::Unreadable(_))),
        "got {outcome:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v093_unavailable_models_fail_explicitly() {
    let dir = work_dir("unavailable");
    let target = write_bytes(&dir, "shot.png", &fixture_png(64, 48));
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::unavailable("appleIntelligenceNotEnabled");
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01")).await;
    assert!(
        matches!(outcome, Err(RenameError::ModelUnavailable(_))),
        "got {outcome:?}"
    );
    assert!(target.exists(), "failed plans move nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v093_video_is_not_an_image() {
    assert!(classify_image(Path::new("clip.mp4")).is_err());
    assert!(classify_image(Path::new("clip.MOV")).is_err());
}

// ---------------------------------------------------------------------------
// Person descriptions: no tool-side restriction (user decision)
// ---------------------------------------------------------------------------
//
// Meaning and attributes are the model's domain: the tool does not restrict
// what the model may describe, including names and appearance attributes.
// Only the accuracy rule stays (uncertain visual content is left empty).
// Rust-side data still carries no person slot (`FmImageFacts` is capture
// time, camera and dimensions only): the tool never extracts person
// attributes itself, it just does not forbid the model's descriptions.

fn swift_bridge_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("swift/kaseifu_fm_bridge.swift");
    std::fs::read_to_string(&path).expect("swift bridge source is readable")
}

#[test]
fn v093_image_prompt_has_no_person_restrictions() {
    let source = swift_bridge_source();
    for banned in [
        "Never output a person's name",
        "gender, age, ethnicity",
        "non-person elements",
        "when only people could be described",
    ] {
        assert!(
            !source.contains(banned),
            "person restriction must stay lifted: {banned}"
        );
    }
    assert!(
        source.contains("Do not state uncertain visual content as fact"),
        "the accuracy rule stays for image prompts"
    );
    // The image gate itself stays (prompt builder plus session
    // instructions); document prompts are byte-identical to before.
    assert_eq!(
        source
            .matches("request[\"target_type\"] as? String == \"image\"")
            .count(),
        2,
        "image gating stays in prompt + instructions"
    );
}

#[test]
fn v093_image_facts_carry_no_person_fields() {
    // Person attributes have no wire slot: the image object is capture time,
    // camera and dimensions only, so they cannot leak through Rust-side data
    // (signals, journal, display, logs).
    let input = FmInput {
        filename: "IMG_1234.HEIC".to_string(),
        extension: Some("HEIC".to_string()),
        size_bytes: 10,
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
    };
    let value = serde_json::to_value(&input).expect("serializes");
    let image = value.get("image").expect("image object present");
    let mut keys: Vec<&str> = image
        .as_object()
        .expect("image is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["camera", "capture_time", "height", "width"]);
}

#[tokio::test]
async fn v093_image_requests_carry_no_body_signals() {
    // Images never get a body scan (`T:` signals are document-only), so the
    // model request carries filename/metadata signals plus EXIF facts, and
    // nothing read out of pixels or text bodies on the Rust side.
    let dir = work_dir("nosignals");
    let target = write_bytes(&dir, "IMG_1234.jpg", &fixture_jpeg());
    let bridge = FakeFmBridge::answering(SemanticInfo {
        title: Some("sunset".to_string()),
        category: None,
        project: None,
    });
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    assert!(matches!(outcome, PlanOutcome::Ready(_)), "got {outcome:?}");
    let requests = bridge.requests();
    assert_eq!(requests.len(), 1);
    let request: serde_json::Value = serde_json::from_str(&requests[0]).expect("request JSON");
    let signals = request["input"]["semantic_signals"]
        .as_array()
        .expect("signals travel as an array");
    assert!(
        signals
            .iter()
            .all(|signal| !signal.as_str().unwrap_or_default().starts_with("T:")),
        "image requests carry no body-text signals: {signals:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

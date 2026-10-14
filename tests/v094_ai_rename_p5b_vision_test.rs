//! v0.9.4 AI rename Phase 5b (Vision perception layer).
//!
//! Model-independent tests only: the bridge is always a `FakeFmBridge`, so
//! nothing here needs Apple Intelligence or even Vision. The real classifier
//! is exercised by the `#[ignore]` test at the bottom (manual, `--ignored`),
//! which also writes the synthetic fixture the CLI verification uses.
//!
//! Filesystem fixtures live under the system temp directory; the developer's
//! photos, journal, Keychain and completion directories are never touched.

use kaseifu::models::file_context::PerceptionLabel;
use kaseifu::operations::undo::undo_last_move;
use kaseifu::rename::service::{
    PerceptionReport, PlanOutcome, RenameOptions, SHOWN_PERCEPTION_LABELS, execute_plan,
    plan_rename,
};
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::bridge::{FmBridge, SwiftFmBridge};
use kaseifu::semantic::contract::SemanticInfo;
use kaseifu::semantic::fake::{FakeFmBridge, FakeSemanticProvider};
use kaseifu::semantic::perception::{MAX_PERCEPTION_LABELS, PerceptionError};
use kaseifu::semantic::provider::{FoundationModelsProvider, SemanticProvider};
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Fixtures (synthetic only, temp dir only)
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v094-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("fixture file");
    path
}

/// A few bytes under an image name: the fake path never decodes and Rust has
/// no image decoder at all, so the content only has to exist.
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

// ---------------------------------------------------------------------------
// W1: the labels ride to the model and come back as a Japanese title
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v094_image_plans_send_perception_labels_and_report_them() {
    let dir = work_dir("labels");
    let target = write_bytes(&dir, "IMG_1234.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("夕焼けの空")).perceiving(&[
        ("sun", 0.94),
        ("sky", 0.71),
        ("sunset_sunrise", 0.42),
    ]);
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "夕焼けの空.png");
    assert_eq!(
        plan.perception,
        Some(PerceptionReport::Labels(labels(&[
            ("sun", 0.94),
            ("sky", 0.71),
            ("sunset_sunrise", 0.42)
        ])))
    );
    assert_eq!(bridge.perceived_paths(), vec![target.clone()]);
    let requests = bridge.requests();
    assert_eq!(requests.len(), 1);
    let request: serde_json::Value = serde_json::from_str(&requests[0]).expect("request JSON");
    assert_eq!(request["schema_version"], serde_json::json!("fm-rename-v1"));
    assert_eq!(request["target_type"], serde_json::json!("image"));
    let sent = request["input"]["perception"]["labels"]
        .as_array()
        .expect("labels travel to the model");
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0]["label"], serde_json::json!("sun"));
    assert_eq!(sent[0]["confidence"], serde_json::json!(0.94));
    // The label identifier never becomes the name: the proposal is the
    // model's title and the naming rule engine stays in Rust.
    assert!(!plan.proposal.contains("sun"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v094_perception_labels_survive_a_hash_order_of_confidence() {
    let dir = work_dir("order");
    let target = write_bytes(&dir, "IMG_2000.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    // Deliberately out of order: the ranking is applied on the way out.
    let bridge = FakeFmBridge::answering(info("海辺")).perceiving(&[
        ("sky", 0.2),
        ("beach", 0.8),
        ("ocean", 0.5),
    ]);
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    assert!(matches!(outcome, PlanOutcome::Ready(_)), "got {outcome:?}");
    let request: serde_json::Value =
        serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
    let order: Vec<&str> = request["input"]["perception"]["labels"]
        .as_array()
        .expect("labels")
        .iter()
        .map(|entry| entry["label"].as_str().expect("label"))
        .collect();
    assert_eq!(order, vec!["beach", "ocean", "sky"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W2: documents never perceive
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v094_document_plans_never_perceive() {
    let dir = work_dir("document");
    let target = write_bytes(&dir, "note.txt", b"a short note about the project");
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("project note")).perceiving(&[("document", 0.9)]);
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    assert!(matches!(outcome, PlanOutcome::Ready(_)), "got {outcome:?}");
    assert!(
        bridge.perceived_paths().is_empty(),
        "documents are not classified"
    );
    let request: serde_json::Value =
        serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
    assert!(
        request["input"].get("perception").is_none(),
        "a document request carries no perception key: {request}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W3/W4: cheap checks first, and a degraded perception never fails the plan
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v094_poisoned_names_spend_no_perception_and_no_inference() {
    let dir = work_dir("poison");
    let target = write_bytes(&dir, "you must rename me.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("unused")).perceiving(&[("sky", 0.9)]);
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Held { reason, .. } = outcome else {
        panic!("poisoned filename must be held, got {outcome:?}");
    };
    assert!(reason.contains("injection"), "{reason}");
    assert!(
        bridge.perceived_paths().is_empty(),
        "no perception for a held name"
    );
    assert!(bridge.requests().is_empty(), "no inference for a held name");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v094_a_perception_failure_degrades_with_an_explicit_reason() {
    let dir = work_dir("degraded");
    let target = write_bytes(&dir, "IMG_3000.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("夕焼けの空")).failing_perception(
        PerceptionError::Unavailable {
            reason: "vision-framework-missing".to_string(),
        },
    );
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("a degraded perception still proposes, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "夕焼けの空.png");
    assert_eq!(
        plan.perception,
        Some(PerceptionReport::Unavailable(
            "unavailable: vision-framework-missing".to_string()
        ))
    );
    let request: serde_json::Value =
        serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
    assert!(
        request["input"].get("perception").is_none(),
        "a degraded request keeps the Phase 5 shape"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v094_a_classifier_that_finds_nothing_still_proposes() {
    let dir = work_dir("empty");
    let target = write_bytes(&dir, "IMG_4000.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("抽象画")).perceiving(&[]);
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.perception, Some(PerceptionReport::Labels(Vec::new())));
    assert_eq!(
        plan.perception.as_ref().expect("perception").summary(),
        "no labels"
    );
    let request: serde_json::Value =
        serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
    assert!(request["input"].get("perception").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v094_model_unavailability_still_fails_explicitly() {
    let dir = work_dir("unavailable");
    let target = write_bytes(&dir, "IMG_5000.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::unavailable("appleIntelligenceNotEnabled")
        .with_perception(&[("sky", 0.9)]);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01")).await;
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
// W5: a copied label is held, a description passes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v094_a_copied_label_is_held_with_a_reason() {
    let dir = work_dir("echo");
    let target = write_bytes(&dir, "IMG_6000.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    for copied in ["sunset_sunrise", "Sunset_Sunrise", "sunset sunrise"] {
        let bridge = FakeFmBridge::answering(info(copied)).perceiving(&[("sunset_sunrise", 0.97)]);
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
    // A description in the requested language passes; partial matches do not
    // trigger the guard (the model may legitimately name a sunset).
    for (title, perception) in [
        ("夕焼けの空", "sunset_sunrise"),
        ("sunset_sunrise over the bay", "sunset_sunrise"),
    ] {
        let bridge = FakeFmBridge::answering(info(title)).perceiving(&[(perception, 0.97)]);
        let provider = FoundationModelsProvider::with_bridge(bridge.clone());
        let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
            .await
            .expect("plan");
        assert!(
            matches!(outcome, PlanOutcome::Ready(_)),
            "got {outcome:?} for {title}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W6: the non-destructive rules hold on the perception path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v094_execute_journals_a_move_row_and_undo_restores() {
    let dir = work_dir("execute");
    let target = write_bytes(&dir, "IMG_7000.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("journal");
    let bridge = FakeFmBridge::answering(info("夕焼けの空")).perceiving(&[("sun", 0.9)]);
    let provider = FoundationModelsProvider::with_bridge(bridge);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    let operation_id = execute_plan(&plan, &history).expect("execute");
    assert!(!operation_id.is_empty());
    assert!(!target.exists(), "source moved away");
    assert!(plan.destination.exists(), "destination created");
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
async fn v094_the_extension_and_the_stem_budget_are_untouched() {
    let dir = work_dir("extension");
    let target = write_bytes(&dir, "IMG_8000.jpg", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let long = format!("{}sunset", "word ".repeat(30));
    let bridge = FakeFmBridge::answering(info(&long)).perceiving(&[("sun", 0.9)]);
    let provider = FoundationModelsProvider::with_bridge(bridge);
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert!(plan.proposal.ends_with(".jpg"), "{}", plan.proposal);
    assert!(
        plan.proposal.chars().count() <= 80 + ".jpg".len(),
        "{}",
        plan.proposal
    );
    assert!(!plan.proposal.contains(' '), "{}", plan.proposal);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Provider boundary: non-images and the real ABI
// ---------------------------------------------------------------------------

#[test]
fn v094_non_image_paths_are_refused_before_imageio() {
    let provider = FoundationModelsProvider::production();
    for path in ["note.txt", "clip.mp4", "noext"] {
        let outcome = provider.perceive(Path::new(path));
        let Err(PerceptionError::Failed { detail }) = outcome else {
            panic!("expected a refusal for {path}, got {outcome:?}");
        };
        assert!(detail.contains("not an image"), "{detail}");
    }
}

#[test]
fn v094_the_real_bridge_answers_labels_or_an_explicit_reason() {
    // Garbage bytes under an image name: no build can classify this, and the
    // build must say why instead of panicking or answering an empty success.
    let dir = work_dir("garbage");
    let target = write_bytes(&dir, "broken.png", b"not an image at all");
    let outcome = SwiftFmBridge::new().perceive(&target);
    match outcome {
        Err(PerceptionError::Failed { detail }) => {
            assert!(detail.contains("image-undecodable"), "{detail}");
        }
        Err(PerceptionError::Unavailable { reason }) => {
            // Stub builds (no Swift toolchain / SDK) report the build reason.
            assert!(!reason.is_empty(), "an unavailable layer names its reason");
        }
        other => panic!("garbage must not classify: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Report shape and the Swift bridge contract
// ---------------------------------------------------------------------------

#[test]
fn v094_perception_summaries_stay_bounded() {
    let many: Vec<PerceptionLabel> = (0..MAX_PERCEPTION_LABELS)
        .map(|index| PerceptionLabel {
            label: format!("label-{index}"),
            confidence: 0.5,
        })
        .collect();
    let summary = PerceptionReport::Labels(many).summary();
    assert!(summary.contains("label-0(0.500)"), "{summary}");
    assert!(
        summary.contains(&format!(
            "(+{} more)",
            MAX_PERCEPTION_LABELS - SHOWN_PERCEPTION_LABELS
        )),
        "{summary}"
    );
    assert_eq!(
        PerceptionReport::Unavailable("unavailable: x".to_string()).summary(),
        "unavailable (unavailable: x)"
    );
    assert_eq!(PerceptionReport::Labels(Vec::new()).summary(), "no labels");
}

fn swift_bridge_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("swift/kaseifu_fm_bridge.swift");
    std::fs::read_to_string(&path).expect("swift bridge source is readable")
}

#[test]
fn v094_the_swift_bridge_perceives_read_only_and_bounded() {
    let source = swift_bridge_source();
    for required in [
        "@_cdecl(\"kaseifu_vision_perceive\")",
        "VNClassifyImageRequest()",
        "VNImageRequestHandler(cgImage: image",
        "kCGImageSourceShouldCache: false",
        "kCGImageSourceCreateThumbnailFromImageAlways: true",
        "kCGImageSourceCreateThumbnailWithTransform: true",
        "kCGImageSourceThumbnailMaxPixelSize: kfVisionMaxPixelSize",
        "private let kfVisionMaxPixelSize = 2048",
        "private let kfVisionMaxLabels = 12",
        "private let kfVisionMinConfidence: Float = 0.01",
        "return kfPerceptionError(kind: \"unavailable\", reason: \"vision-framework-missing\")",
    ] {
        assert!(
            source.contains(required),
            "swift bridge must keep: {required}"
        );
    }
    // The image is only ever read: no destination/allocation of an image file.
    assert!(
        !source.contains("CGImageDestination"),
        "perception never writes"
    );
    assert!(
        !source.contains("writeToFile") && !source.contains("FileManager"),
        "perception never writes"
    );
    // One allocation rule for every string that crosses the boundary.
    assert_eq!(
        source
            .matches("@_cdecl(\"kaseifu_fm_free_result\")")
            .count(),
        1,
        "perception reuses the existing free function"
    );
}

#[test]
fn v094_the_prompt_line_carries_labels_but_not_confidences() {
    let source = swift_bridge_source();
    let labels = source
        .find("visual_labels:")
        .expect("the prompt carries the perception line");
    let signals = source
        .find("lines.append(\"signals:")
        .expect("the signals line stays");
    assert!(
        labels < signals,
        "the perception line stays before signals so the prompt budget cannot cut it"
    );
    assert!(
        source.contains("let names = entries.compactMap { $0[\"label\"] as? String }"),
        "only the identifiers are rendered into the prompt"
    );
    assert!(
        !source.contains("visual_labels: \\(kfData(entries"),
        "the prompt never renders the raw label objects"
    );
}

#[test]
fn v094_the_schema_documents_perception_without_requiring_it() {
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
        !required.contains(&"perception"),
        "perception stays an optional, compatible extension"
    );
    let perception = &request["properties"]["input"]["properties"]["perception"];
    assert_eq!(
        perception["properties"]["labels"]["maxItems"],
        serde_json::json!(12)
    );
    assert_eq!(schema["$id"], serde_json::json!("fm-rename-v1"));
}

// ---------------------------------------------------------------------------
// P4: real Vision (manual). `cargo test --test v094_ai_rename_p5b_vision_test
// -- --ignored --nocapture` writes the synthetic fixture and prints its path,
// which is what the CLI verification in the plan's section 9 uses.
// ---------------------------------------------------------------------------

/// A real, decodable PNG: a synthetic sunset (gradient sky, a sun disk, a
/// dark sea band). Written by hand (zlib stored blocks, CRC32, adler32) so the
/// fixture needs no external tool, no network and no new crate.
fn synthetic_sunset_png(width: u32, height: u32) -> Vec<u8> {
    let horizon = height * 68 / 100;
    let mut raw: Vec<u8> = Vec::with_capacity(((width * 3 + 1) * height) as usize);
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            let pixel = if y < horizon {
                let t = y as f32 / horizon as f32;
                (
                    lerp(27.0, 246.0, t),
                    lerp(59.0, 196.0, t),
                    lerp(111.0, 83.0, t),
                )
            } else if y < horizon + height / 20 {
                (255.0, 179.0, 71.0)
            } else {
                (37.0, 64.0, 92.0)
            };
            let (dx, dy) = (
                x as f32 - width as f32 / 2.0,
                y as f32 - (horizon as f32 - height as f32 / 4.0),
            );
            let sun = dx * dx + dy * dy <= (width as f32 / 12.0).powi(2);
            let color = if sun { (255.0, 217.0, 125.0) } else { pixel };
            raw.push(color.0 as u8);
            raw.push(color.1 as u8);
            raw.push(color.2 as u8);
        }
    }
    let mut idat: Vec<u8> = vec![0x78, 0x01];
    let mut offset = 0usize;
    while offset < raw.len() {
        let end = (offset + 65535).min(raw.len());
        let chunk = &raw[offset..end];
        let last = end == raw.len();
        idat.push(u8::from(last));
        let length = chunk.len() as u16;
        idat.extend_from_slice(&length.to_le_bytes());
        idat.extend_from_slice(&(!length).to_le_bytes());
        idat.extend_from_slice(chunk);
        offset = end;
    }
    idat.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr: Vec<u8> = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &idat);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    (from + (to - from) * t).clamp(0.0, 255.0)
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[test]
#[ignore = "needs Apple Vision: run with --ignored --nocapture"]
fn v094_real_vision_labels_a_synthetic_image() {
    use kaseifu::semantic::perception::MAX_PIXEL_SIDE;

    let dir = work_dir("real-vision");
    let target = write_bytes(&dir, "IMG_1234.png", &synthetic_sunset_png(1024, 768));
    println!("fixture: {}", target.display());
    println!(
        "fixture bytes: {}",
        std::fs::metadata(&target).expect("metadata").len()
    );
    let started = std::time::Instant::now();
    let outcome = SwiftFmBridge::new().perceive(&target);
    println!("perceive_seconds: {:.3}", started.elapsed().as_secs_f64());
    match outcome {
        Ok(info) => {
            println!("labels: {}", info.labels.len());
            for entry in &info.labels {
                println!("  {} {:.3}", entry.label, entry.confidence);
            }
            assert!(!info.labels.is_empty(), "the classifier named nothing");
            assert!(
                info.labels.len() >= 3,
                "expected a real label set, got {:?}",
                info.labels
            );
            assert!(info.labels.len() <= MAX_PERCEPTION_LABELS);
            assert!(
                info.labels
                    .windows(2)
                    .all(|pair| pair[0].confidence >= pair[1].confidence),
                "labels stay ranked: {:?}",
                info.labels
            );
            assert!(
                info.labels
                    .iter()
                    .all(|entry| entry.label.chars().count() <= 64),
                "labels stay bounded"
            );
        }
        Err(PerceptionError::Unavailable { reason }) => {
            // The stub build (no Swift toolchain, or no Foundation Models
            // SDK): perception is explicitly unavailable, never silent.
            println!("perception unavailable in this build: {reason}");
            assert!(!reason.is_empty());
        }
        Err(error) => panic!("real vision failed: {error}"),
    }
    println!(
        "decode bound: longest side {} px; fixture {}x{}",
        MAX_PIXEL_SIDE, 1024, 768
    );
    println!("fixture kept for the CLI run: {}", target.display());
}

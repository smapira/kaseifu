//! v0.9.0 Phase 2: the semantic provider contract, with injected fakes.
//!
//! Every test here is model-independent: the bridge is a
//! [`FakeFmBridge`](kaseifu::semantic::fake::FakeFmBridge) that answers with
//! configured data, so the suite runs on CI (no Apple Intelligence, no
//! Foundation Models SDK) and on a developer machine alike.
//!
//! Nothing here touches the filesystem, the journal or the configuration.

use std::time::Duration;

use kaseifu::models::file_context::{
    CameraInfo, CaptureTime, ImageMetadata, NormalizedFileContext, TargetFile,
};
use kaseifu::semantic::bridge::FmBridgeError;
use kaseifu::semantic::contract::{
    self, AnalysisFlags, Availability, ErrorKind, SemanticInfo, TargetKind,
};
use kaseifu::semantic::fake::{FakeFmBridge, FakeSemanticProvider};
use kaseifu::semantic::{FoundationModelsProvider, SemanticError, SemanticProvider};

/// A synthetic observation (no filesystem access).
fn document_context() -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: "/tmp/proposal.pdf".into(),
            filename: "proposal.pdf".to_string(),
            extension: Some("pdf".to_string()),
            size_bytes: 4096,
            modified_at: Some("2026-10-14T09:00:00Z".to_string()),
        },
        parent_directory: "/tmp".to_string(),
        size_bytes: 4096,
        creation_date: Some("2026-10-13T08:00:00Z".to_string()),
        modification_date: None,
        semantic_signals: vec!["F:proposal".to_string()],
        ..NormalizedFileContext::default()
    }
}

/// A synthetic image observation with EXIF facts.
fn image_context() -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: "/tmp/IMG_1234.HEIC".into(),
            filename: "IMG_1234.HEIC".to_string(),
            extension: Some("HEIC".to_string()),
            size_bytes: 2_000_000,
            modified_at: None,
        },
        parent_directory: "/tmp".to_string(),
        size_bytes: 2_000_000,
        semantic_signals: Vec::new(),
        image_metadata: Some(ImageMetadata {
            capture_time: Some(CaptureTime {
                value: "2026-10-01T10:00:00".to_string(),
                timezone: None,
                source: "exif".to_string(),
                timezone_known: false,
            }),
            camera: CameraInfo {
                make: Some("Apple".to_string()),
                model: Some("iPhone 17".to_string()),
                source: None,
            },
            ..ImageMetadata::default()
        }),
        ..NormalizedFileContext::default()
    }
}

fn info(title: Option<&str>, category: Option<&str>) -> SemanticInfo {
    SemanticInfo {
        title: title.map(str::to_string),
        category: category.map(str::to_string),
        project: None,
    }
}

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

#[test]
fn availability_is_forwarded_from_the_bridge() {
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::unavailable(
        "appleIntelligenceNotEnabled",
    ));
    let availability = provider.availability().expect("availability");
    assert_eq!(availability.reason(), Some("appleIntelligenceNotEnabled"));
    assert_eq!(
        availability.label(),
        "unavailable: appleIntelligenceNotEnabled"
    );

    let provider =
        FoundationModelsProvider::with_bridge(FakeFmBridge::answering(info(Some("x"), None)));
    assert!(
        provider
            .availability()
            .expect("availability")
            .is_available()
    );
}

#[test]
fn the_production_provider_reports_availability_without_needing_a_model() {
    // Real bridge, no inference: available or an explicit reason, never a panic.
    let provider = FoundationModelsProvider::production();
    match provider.availability().expect("availability") {
        Availability::Available => {}
        Availability::Unavailable { reason, .. } => assert!(!reason.is_empty()),
    }
}

// ---------------------------------------------------------------------------
// Request building
// ---------------------------------------------------------------------------

#[tokio::test]
async fn analyze_sends_a_bounded_contract_request() {
    let bridge = FakeFmBridge::answering(info(Some("提案書"), Some("企画")));
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let result = provider
        .analyze(&document_context())
        .await
        .expect("analysis");
    assert_eq!(result.title.as_deref(), Some("提案書"));
    assert_eq!(result.category.as_deref(), Some("企画"));

    let requests = bridge.requests();
    assert_eq!(requests.len(), 1, "one request per analysis");
    let request: serde_json::Value = serde_json::from_str(&requests[0]).expect("request JSON");
    assert_eq!(request["schema_version"], "fm-rename-v1");
    assert_eq!(request["request_id"], "rename-0001");
    assert_eq!(request["target_type"], "document");
    assert_eq!(request["language"], "ja");
    assert_eq!(request["analysis"]["title"], true);
    assert_eq!(request["analysis"]["project"], false);
    assert_eq!(request["input"]["filename"], "proposal.pdf");
    assert_eq!(request["input"]["parent_directory"], "/tmp");
    assert_eq!(request["input"]["size_bytes"], 4096);
    assert_eq!(request["input"]["created_at"], "2026-10-13T08:00:00Z");
    // The provider default: title + category, never project.
    assert_eq!(
        AnalysisFlags {
            title: true,
            category: true,
            project: false
        },
        AnalysisFlags::default()
    );
}

#[tokio::test]
async fn analyze_bounds_the_signals_and_the_filename() {
    let bridge = FakeFmBridge::answering(info(Some("t"), None));
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let mut context = document_context();
    context.semantic_signals = (0..40).map(|index| format!("signal-{index}")).collect();
    context.target.filename = "あ".repeat(900);
    provider.analyze(&context).await.expect("analysis");

    let request: serde_json::Value =
        serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
    let signals = request["input"]["semantic_signals"]
        .as_array()
        .expect("signals array");
    assert_eq!(signals.len(), contract::MAX_SIGNALS);
    assert_eq!(
        request["input"]["filename"]
            .as_str()
            .expect("filename")
            .chars()
            .count(),
        contract::MAX_FILENAME_CHARS
    );
}

#[tokio::test]
async fn analyze_recognizes_images_and_carries_the_exif_facts() {
    let bridge = FakeFmBridge::answering(info(Some("海辺の夕日"), None));
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    provider.analyze(&image_context()).await.expect("analysis");
    let request: serde_json::Value =
        serde_json::from_str(&bridge.requests()[0]).expect("request JSON");
    assert_eq!(request["target_type"], "image");
    assert_eq!(
        request["input"]["image"]["capture_time"],
        "2026-10-01T10:00:00"
    );
    assert_eq!(request["input"]["image"]["camera"], "Apple iPhone 17");
}

#[tokio::test]
async fn request_ids_increase_per_call() {
    let bridge = FakeFmBridge::answering(info(Some("t"), None));
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    provider.analyze(&document_context()).await.expect("first");
    provider.analyze(&document_context()).await.expect("second");
    let ids: Vec<String> = bridge
        .requests()
        .iter()
        .map(|request| {
            serde_json::from_str::<serde_json::Value>(request).expect("JSON")["request_id"]
                .as_str()
                .expect("id")
                .to_string()
        })
        .collect();
    assert_eq!(ids, vec!["rename-0001", "rename-0002"]);
}

// ---------------------------------------------------------------------------
// Errors (original spec 12)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unavailable_model_is_an_explicit_error_without_a_fallback() {
    let bridge = FakeFmBridge::unavailable("modelNotReady");
    let provider = FoundationModelsProvider::with_bridge(bridge.clone());
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("unavailable");
    assert_eq!(
        error,
        SemanticError::Unavailable {
            reason: "modelNotReady".to_string()
        }
    );
    assert!(error.is_unavailable());
    assert!(
        !error.is_skippable(),
        "unavailable stops the run, it does not skip"
    );
    assert_eq!(
        bridge.requests().len(),
        1,
        "exactly one attempt, no fallback"
    );
}

#[tokio::test]
async fn a_timeout_is_skippable_and_comes_from_the_bridge() {
    let provider =
        FoundationModelsProvider::with_bridge(FakeFmBridge::failing(FmBridgeError::Timeout))
            .with_timeout(Duration::from_millis(1));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("timeout");
    assert_eq!(error, SemanticError::Timeout);
    assert!(error.is_skippable());
}

#[tokio::test]
async fn a_rejected_request_is_a_bridge_error() {
    let provider =
        FoundationModelsProvider::with_bridge(FakeFmBridge::failing(FmBridgeError::Rejected {
            code: 4,
            detail: "a generation is already active on this context".to_string(),
        }));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("rejected");
    assert!(matches!(error, SemanticError::Bridge(_)), "{error}");
    assert!(!error.is_skippable());
}

#[tokio::test]
async fn a_malformed_response_is_invalid_result() {
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::responding("not json"));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("malformed");
    assert!(matches!(error, SemanticError::InvalidResult(_)), "{error}");
    assert!(error.is_skippable());
}

#[tokio::test]
async fn a_response_for_another_request_is_invalid() {
    let response = serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": "rename-9999",
        "status": "success",
        "semantic": {"title": "x"}
    })
    .to_string();
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::responding(response));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("mismatched id");
    assert!(matches!(error, SemanticError::InvalidResult(_)), "{error}");
}

#[tokio::test]
async fn a_model_init_failure_stops_the_run() {
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::reporting(
        ErrorKind::ModelInitFailed,
        Some("session refused to start".to_string()),
    ));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("init failure");
    assert!(
        matches!(error, SemanticError::ModelInitFailed(_)),
        "{error}"
    );
    assert!(
        !error.is_skippable(),
        "an init failure aborts, it does not skip"
    );
}

#[tokio::test]
async fn an_inference_failure_and_a_cancellation_are_skippable() {
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::reporting(
        ErrorKind::InferenceFailed,
        None,
    ));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("inference failure");
    assert!(
        matches!(error, SemanticError::InferenceFailed(_)),
        "{error}"
    );
    assert!(error.is_skippable());

    let provider =
        FoundationModelsProvider::with_bridge(FakeFmBridge::reporting(ErrorKind::Cancelled, None));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("cancelled");
    assert_eq!(error, SemanticError::Cancelled);
    assert!(error.is_skippable());
}

// ---------------------------------------------------------------------------
// Payload tolerance (forward compatibility)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_fields_and_kinds_are_tolerated() {
    let response = serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": "rename-0001",
        "status": "success",
        "semantic": {"title": "提案書", "category": null, "project": 7, "extra": true},
        "future_field": {"nested": 1}
    })
    .to_string();
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::responding(response));
    let info = provider
        .analyze(&document_context())
        .await
        .expect("analysis");
    assert_eq!(info.title.as_deref(), Some("提案書"));
    assert_eq!(info.category, None);
    assert_eq!(info.project, None, "a wrong type is treated as missing");

    let unknown_kind = serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": "rename-0001",
        "status": "error",
        "error": {"kind": "something-new", "detail": "d"}
    })
    .to_string();
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::responding(unknown_kind));
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("unknown kind");
    assert!(matches!(error, SemanticError::InvalidResult(_)), "{error}");
}

#[tokio::test]
async fn a_missing_title_stays_missing_instead_of_being_invented() {
    let response = serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": "rename-0001",
        "status": "success",
        "semantic": {"title": null, "category": "   ", "project": null}
    })
    .to_string();
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::responding(response));
    let info = provider
        .analyze(&document_context())
        .await
        .expect("analysis");
    assert!(info.is_empty(), "nothing may be invented: {info:?}");
}

#[tokio::test]
async fn the_target_kind_helper_matches_the_image_predicate() {
    assert_eq!(
        kaseifu::semantic::provider::target_kind(std::path::Path::new("a.HEIC")),
        TargetKind::Image
    );
    assert_eq!(
        kaseifu::semantic::provider::target_kind(std::path::Path::new("a.pdf")),
        TargetKind::Document
    );
}

// ---------------------------------------------------------------------------
// The provider-level fake itself (Phase 3 callers depend on it)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_fake_provider_returns_configured_values() {
    let provider = FakeSemanticProvider::returning(info(Some("configured"), Some("test")));
    let result = provider
        .analyze(&document_context())
        .await
        .expect("analysis");
    assert_eq!(result.title.as_deref(), Some("configured"));
    assert!(
        provider
            .availability()
            .expect("availability")
            .is_available()
    );

    let provider = FakeSemanticProvider::unavailable("deviceNotEligible");
    let error = provider
        .analyze(&document_context())
        .await
        .expect_err("unavailable");
    assert!(
        matches!(error, SemanticError::Unavailable { .. }),
        "{error}"
    );
    assert_eq!(
        provider.availability().expect("availability").reason(),
        Some("deviceNotEligible")
    );
}

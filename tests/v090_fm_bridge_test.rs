//! v0.9.0 Phase 0-2: the Foundation Models C ABI, end to end.
//!
//! These tests run against whatever the build linked: the Swift bridge on a
//! machine with a macOS 26 SDK, or the Rust fallback (`KASEIFU_FM_FORCE_STUB`,
//! CI). The default suite never depends on a model being available:
//!
//! - the availability check, the immediate rejections, the timeout/cancel path
//!   and the destroy rules run everywhere,
//! - the tests that would need a real inference step aside when the model is
//!   available (CI has no model; a developer machine with Apple Intelligence
//!   enabled runs them through `--ignored` and `examples/fm_bridge_smoke`).
//!
//! Nothing here touches the filesystem, the journal or the configuration.

use std::ffi::{CString, c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};
use kaseifu::semantic::bridge::SwiftFmBridge;
use kaseifu::semantic::ffi::{self, raw};
use kaseifu::semantic::{
    Availability, BRIDGE_KIND, ContractError, FfiError, FmBridge, FmContext,
    FoundationModelsProvider, SemanticProvider, contract,
};

/// Reasons this build may report; anything else is a bug or a new contract
/// value that must be added here deliberately.
const KNOWN_REASONS: [&str; 6] = [
    "appleIntelligenceNotEnabled",
    "deviceNotEligible",
    "modelNotReady",
    "unsupported-os",
    "foundation-models-sdk-missing",
    "swift-toolchain-missing",
];

/// Counts callback invocations for the raw ABI tests.
struct Counter {
    calls: AtomicUsize,
}

unsafe extern "C" fn counting_callback(result: *mut c_char, user_data: *mut c_void) {
    let counter = unsafe { &*(user_data as *const Counter) };
    counter.calls.fetch_add(1, Ordering::SeqCst);
    if !result.is_null() {
        unsafe { raw::kaseifu_fm_free_result(result) };
    }
}

fn counter() -> Arc<Counter> {
    Arc::new(Counter {
        calls: AtomicUsize::new(0),
    })
}

fn calls(counter: &Arc<Counter>) -> usize {
    counter.calls.load(Ordering::SeqCst)
}

/// Wait until the counter reaches `expected`, or give up after `deadline`.
fn wait_for_calls(counter: &Arc<Counter>, expected: usize, deadline: Duration) -> usize {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if calls(counter) >= expected {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    calls(counter)
}

/// A valid `fm-rename-v1` request.
fn request_json(request_id: &str) -> String {
    serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": request_id,
        "target_type": "document",
        "path": "/tmp/example.pdf",
        "language": "ja",
        "analysis": {"title": true, "category": true, "project": false},
        "input": {
            "filename": "example.pdf",
            "size_bytes": 42,
            "parent_directory": "/tmp",
            "semantic_signals": ["F:example"]
        }
    })
    .to_string()
}

fn c_string(value: &str) -> CString {
    CString::new(value).expect("no NUL in the test strings")
}

/// The context pointer of a live context (raw ABI callers own that lifetime).
fn context_pointer(context: &FmContext) -> *mut c_void {
    context.as_raw_pointer()
}

/// The model availability as this build sees it.
fn availability() -> Availability {
    SwiftFmBridge::new()
        .available()
        .expect("the bridge answers")
}

/// True when the test should step aside: with a live model, completing a
/// request means a real inference, which the default suite must not require
/// (the ignored tests and the smoke example cover that path).
fn skip_with_a_live_model(test: &str) -> bool {
    if availability().is_available() {
        println!(
            "{test}: the model is available, so this test steps aside; run it with \
             `cargo test -- --ignored` (or the smoke example) to exercise the inference path"
        );
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Availability (original spec 4.2 / acceptance 15)
// ---------------------------------------------------------------------------

#[test]
fn availability_is_an_explicit_reason_not_a_fallback() {
    match availability() {
        Availability::Available => {
            println!("availability: available (this machine can run the model)");
        }
        Availability::Unavailable { reason, detail } => {
            println!("availability: unavailable: {reason} (detail: {detail:?})");
            assert!(
                KNOWN_REASONS.contains(&reason.as_str()),
                "unexpected reason: {reason}"
            );
        }
    }
}

#[test]
fn the_availability_payload_is_a_contract_envelope() {
    let json = ffi::available().expect("the bridge answers");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(value["schema_version"], "fm-rename-v1");
    assert!(value["status"] == "available" || value["status"] == "unavailable");
    if value["status"] == "unavailable" {
        let reason = value["reason"]
            .as_str()
            .expect("unavailable names a reason");
        assert!(!reason.is_empty());
        let parsed = contract::parse_availability(&json).expect("parses");
        assert_eq!(parsed.reason(), Some(reason));
    } else {
        assert_eq!(
            contract::parse_availability(&json).expect("parses"),
            Availability::Available
        );
    }
}

#[test]
fn an_unknown_availability_status_is_rejected() {
    let error =
        contract::parse_availability(r#"{"schema_version":"fm-rename-v1","status":"soon"}"#);
    assert!(matches!(error, Err(ContractError::UnknownStatus(_))));
}

// ---------------------------------------------------------------------------
// Immediate rejection (original spec 13.1 FFI05)
// ---------------------------------------------------------------------------

#[test]
fn a_null_context_is_rejected_without_a_callback() {
    let counter = counter();
    let request = c_string(&request_json("rename-0001"));
    let code = unsafe {
        raw::kaseifu_fm_generate(
            std::ptr::null_mut(),
            request.as_ptr(),
            Some(counting_callback),
            Arc::as_ptr(&counter) as *mut c_void,
        )
    };
    assert_eq!(code, 1, "null context must be refused");
    assert_eq!(calls(&counter), 0, "a refused request never calls back");
}

#[test]
fn invalid_requests_are_rejected_before_any_generation() {
    let context = FmContext::create().expect("context");
    let counter = counter();
    let user_data = Arc::as_ptr(&counter) as *mut c_void;

    let cases: [(&str, i32); 3] = [
        ("not json at all", 2),
        (r#"{"schema_version":"other","request_id":"r"}"#, 3),
        (r#"{"schema_version":"fm-rename-v1"}"#, 2),
    ];
    for (payload, expected) in cases {
        let request = c_string(payload);
        let code = unsafe {
            raw::kaseifu_fm_generate(
                context_pointer(&context),
                request.as_ptr(),
                Some(counting_callback),
                user_data,
            )
        };
        assert_eq!(code, expected, "payload {payload}");
    }
    assert_eq!(calls(&counter), 0, "no rejected request may call back");
}

#[test]
fn an_empty_request_id_is_rejected() {
    let context = FmContext::create().expect("context");
    let counter = counter();
    let payload = serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": "",
    })
    .to_string();
    let request = c_string(&payload);
    let code = unsafe {
        raw::kaseifu_fm_generate(
            context_pointer(&context),
            request.as_ptr(),
            Some(counting_callback),
            Arc::as_ptr(&counter) as *mut c_void,
        )
    };
    assert_eq!(code, 2);
    assert_eq!(calls(&counter), 0);
}

// ---------------------------------------------------------------------------
// Accepted requests: exactly one callback (FFI02 / FFI03)
// ---------------------------------------------------------------------------

#[test]
fn an_accepted_request_calls_back_exactly_once() {
    if skip_with_a_live_model("an_accepted_request_calls_back_exactly_once") {
        return;
    }
    let context = FmContext::create().expect("context");
    let counter = counter();
    let request = c_string(&request_json("rename-0001"));
    let code = unsafe {
        raw::kaseifu_fm_generate(
            context_pointer(&context),
            request.as_ptr(),
            Some(counting_callback),
            Arc::as_ptr(&counter) as *mut c_void,
        )
    };
    assert_eq!(code, 0, "a valid request is accepted");
    assert_eq!(
        wait_for_calls(&counter, 1, Duration::from_secs(10)),
        1,
        "the callback must arrive"
    );
    // Cancelling after completion must not produce a second callback.
    context.cancel();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(calls(&counter), 1, "completion happens exactly once");
}

#[test]
fn the_response_payload_is_the_contract_envelope() {
    if skip_with_a_live_model("the_response_payload_is_the_contract_envelope") {
        return;
    }
    let request = request_json("rename-0001");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let raw = runtime.block_on(async {
        let context = FmContext::create().expect("context");
        context
            .generate(&request, Duration::from_secs(10))
            .await
            .expect("the bridge answers")
    });
    let response = contract::parse_response(&raw).expect("contract response");
    assert_eq!(response.request_id, "rename-0001");
    match response.status {
        contract::FmStatus::Success => {
            println!("model answered: {:?}", response.semantic);
        }
        contract::FmStatus::Error => {
            let error = response.error.expect("an error payload");
            println!("model unavailable: {:?}", error);
            assert_eq!(error.kind, contract::ErrorKind::Unavailable);
            let reason = error.reason.expect("unavailable names a reason");
            assert!(KNOWN_REASONS.contains(&reason.as_str()), "{reason}");
        }
        other => panic!("unexpected status: {other:?}"),
    }
}

#[test]
fn a_zero_timeout_cancels_and_discards_any_late_result() {
    let request = request_json("rename-0001");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let outcome = runtime.block_on(async {
        let context = FmContext::create().expect("context");
        context.generate(&request, Duration::ZERO).await
    });
    match outcome {
        Err(FfiError::Timeout) => {
            // The generation was cancelled; give a late result time to arrive
            // and prove that discarding it is safe (no crash, no double free).
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(payload) => {
            // The fallback answers synchronously, so the result can win the
            // race; the payload must still be the contract envelope.
            contract::parse_response(&payload).expect("contract response");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Concurrency and destruction (FFI07 / FFI08)
// ---------------------------------------------------------------------------

#[test]
fn concurrent_calls_are_either_accepted_or_refused_as_busy() {
    if skip_with_a_live_model("concurrent_calls_are_either_accepted_or_refused_as_busy") {
        return;
    }
    let context = Arc::new(FmContext::create().expect("context"));
    let counter = counter();
    let mut handles = Vec::new();
    for index in 0..2 {
        let context = Arc::clone(&context);
        let counter = Arc::clone(&counter);
        handles.push(std::thread::spawn(move || {
            let request = c_string(&request_json(&format!("rename-000{index}")));
            let code = unsafe {
                raw::kaseifu_fm_generate(
                    context_pointer(&context),
                    request.as_ptr(),
                    Some(counting_callback),
                    Arc::as_ptr(&counter) as *mut c_void,
                )
            };
            assert!(code == 0 || code == 4, "unexpected code {code}");
            code
        }));
    }
    let codes: Vec<i32> = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect();
    let accepted = codes.iter().filter(|code| **code == 0).count();
    assert_eq!(
        wait_for_calls(&counter, accepted, Duration::from_secs(10)),
        accepted,
        "every accepted call completes exactly once (codes: {codes:?})"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(calls(&counter), accepted, "refused calls never call back");
}

#[test]
fn destroy_never_lets_a_callback_run_afterwards() {
    let context = FmContext::create().expect("context");
    let counter = counter();
    let request = c_string(&request_json("rename-0001"));
    let code = unsafe {
        raw::kaseifu_fm_generate(
            context_pointer(&context),
            request.as_ptr(),
            Some(counting_callback),
            Arc::as_ptr(&counter) as *mut c_void,
        )
    };
    assert_eq!(code, 0);
    context.cancel();
    drop(context);
    let after_destroy = calls(&counter);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        calls(&counter),
        after_destroy,
        "no callback may run after destroy returned"
    );
}

#[test]
fn create_rejects_malformed_options_and_destroy_accepts_null() {
    let options = c_string("not json");
    let context = unsafe { raw::kaseifu_fm_create(options.as_ptr()) };
    assert!(
        context.is_null(),
        "malformed options must not create a context"
    );
    unsafe { raw::kaseifu_fm_destroy(std::ptr::null_mut()) };
    unsafe { raw::kaseifu_fm_cancel(std::ptr::null_mut()) };
    unsafe { raw::kaseifu_fm_free_result(std::ptr::null_mut()) };
}

// ---------------------------------------------------------------------------
// Link evidence: the framework is weak-linked so macOS < 26 still starts
// ---------------------------------------------------------------------------

#[test]
fn the_binary_weak_links_foundation_models() {
    if BRIDGE_KIND != "swift" {
        println!("bridge kind {BRIDGE_KIND}: nothing to check (the fallback links no framework)");
        return;
    }
    let binary = std::env::current_exe().expect("current exe");
    let output = std::process::Command::new("otool")
        .arg("-L")
        .arg(&binary)
        .output();
    let Ok(output) = output else {
        println!("otool is unavailable; skipping the weak-link check");
        return;
    };
    let listing = String::from_utf8_lossy(&output.stdout);
    let line = listing
        .lines()
        .find(|line| line.contains("FoundationModels"))
        .unwrap_or_else(|| panic!("FoundationModels must be linked:\n{listing}"));
    assert!(
        line.contains("weak"),
        "FoundationModels must be weak-linked (macOS < 26 must still start): {line}"
    );
    println!("link: {line}");
}

// ---------------------------------------------------------------------------
// The JSON contract file matches the payloads
// ---------------------------------------------------------------------------

#[test]
fn the_schema_file_matches_the_payloads() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schemas/fm-rename-v1.json");
    let schema_text = std::fs::read_to_string(path).expect("schema file");
    let schema: serde_json::Value = serde_json::from_str(&schema_text).expect("valid JSON");
    assert_eq!(schema["$id"], "fm-rename-v1");
    assert_eq!(schema["type"], "object");
    assert!(
        schema["required"]
            .as_array()
            .expect("required")
            .iter()
            .any(|key| key == "schema_version"),
        "schema_version is mandatory"
    );

    // The availability payload the bridge just produced.
    let availability: serde_json::Value =
        serde_json::from_str(&ffi::available().expect("the bridge answers")).expect("JSON");
    for key in branch_required(
        &schema,
        if availability["status"] == "available" {
            "Available"
        } else {
            "Availability"
        },
    ) {
        assert!(
            availability.get(&key).is_some(),
            "availability must carry {key}"
        );
    }

    // A request carries the Request branch's keys.
    let request: serde_json::Value =
        serde_json::from_str(&request_json("rename-0001")).expect("JSON");
    for key in branch_required(&schema, "Request") {
        assert!(request.get(&key).is_some(), "request must carry {key}");
    }

    // A response carries the Response branch's keys.
    let response = serde_json::json!({
        "schema_version": "fm-rename-v1",
        "request_id": "rename-0001",
        "status": "success",
        "semantic": {"title": "x"}
    });
    for key in branch_required(&schema, "Response") {
        assert!(response.get(&key).is_some(), "response must carry {key}");
    }
}

/// The `required` list of the `oneOf` branch with this title.
fn branch_required(schema: &serde_json::Value, title: &str) -> Vec<String> {
    let branches = schema["oneOf"].as_array().expect("oneOf branches");
    let branch = branches
        .iter()
        .find(|branch| branch["title"] == title)
        .unwrap_or_else(|| panic!("branch {title} exists"));
    branch["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|key| key.as_str().expect("string").to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Manual runs (real inference; not part of the default suite)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs a live model: a second call during a running generation (manual)"]
fn a_second_generate_while_one_is_running_is_refused_as_busy() {
    if !availability().is_available() {
        println!("no live model: the fallback answers synchronously, so nothing can be busy");
        return;
    }
    let context = FmContext::create().expect("context");
    let counter = counter();
    let request = c_string(&request_json("rename-0001"));
    let user_data = Arc::as_ptr(&counter) as *mut c_void;
    let first = unsafe {
        raw::kaseifu_fm_generate(
            context_pointer(&context),
            request.as_ptr(),
            Some(counting_callback),
            user_data,
        )
    };
    assert_eq!(first, 0, "the first call is accepted");
    let second = unsafe {
        raw::kaseifu_fm_generate(
            context_pointer(&context),
            request.as_ptr(),
            Some(counting_callback),
            user_data,
        )
    };
    assert_eq!(second, 4, "a running generation must refuse a second one");
    let started = Instant::now();
    context.cancel();
    println!("cancel returned after {} ms", started.elapsed().as_millis());
    assert_eq!(
        wait_for_calls(&counter, 1, Duration::from_secs(30)),
        1,
        "the refused call adds no callback"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(calls(&counter), 1, "still exactly one completion");
}

/// A synthetic observation (no filesystem access).
fn sample_context() -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: "/tmp/2026-Q4-budget-draft.pdf".into(),
            filename: "2026-Q4-budget-draft.pdf".to_string(),
            extension: Some("pdf".to_string()),
            size_bytes: 84_000,
            modified_at: None,
        },
        parent_directory: "/tmp".to_string(),
        size_bytes: 84_000,
        creation_date: None,
        modification_date: None,
        semantic_signals: vec!["F:budget".to_string(), "F:Q4".to_string()],
        ..NormalizedFileContext::default()
    }
}

#[tokio::test]
#[ignore = "runs a real Foundation Models inference; needs Apple Intelligence (manual)"]
async fn inference_round_trip_reports_semantic_fields_and_latency() {
    let provider = FoundationModelsProvider::production();
    let started = Instant::now();
    let outcome = provider.analyze(&sample_context()).await;
    let elapsed = started.elapsed();
    match outcome {
        Ok(info) => println!(
            "semantic: title={:?} category={:?} project={:?} ({} ms)",
            info.title,
            info.category,
            info.project,
            elapsed.as_millis()
        ),
        Err(error) => {
            println!("semantic error: {error} ({} ms)", elapsed.as_millis());
            assert!(
                error.is_unavailable(),
                "without a model the only acceptable failure is an explicit unavailable"
            );
        }
    }
}

#[tokio::test]
#[ignore = "waits out the destroy grace period during a real inference (manual)"]
async fn destroy_during_a_generation_suppresses_the_callback() {
    let context = FmContext::create().expect("context");
    let counter = counter();
    let request = c_string(&request_json("rename-0001"));
    let code = unsafe {
        raw::kaseifu_fm_generate(
            context_pointer(&context),
            request.as_ptr(),
            Some(counting_callback),
            Arc::as_ptr(&counter) as *mut c_void,
        )
    };
    assert_eq!(code, 0);
    let started = Instant::now();
    drop(context);
    println!(
        "destroy returned after {} ms",
        started.elapsed().as_millis()
    );
    let after_destroy = calls(&counter);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        calls(&counter),
        after_destroy,
        "no callback may run after destroy returned"
    );
}

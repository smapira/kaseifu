//! v0.4.1 patch regression tests for the v3.2 Discovery contamination bug.
//!
//! Two rules are protected here:
//!
//! 1. Image metadata never changes the mdfind search terms (Discovery): a
//!    fully-tagged photo and an untagged file produce the same term set.
//! 2. The Jev `/v1/systemone` `state` payload stays identical to v0.4.0 for
//!    all four privacy combinations (capture on/off x GPS on/off). The
//!    goldens below were captured from the pre-fix build with this same test
//!    code; only the library changed.
//!
//! Mock server: local, std-only, no live API, no new dependencies.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kaseifu::classifier::jev::{JevIntegration, RealJevIntegration};
use kaseifu::context::image_metadata::MetadataOptions;
use kaseifu::discovery::discover_with;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::DecisionContext;
use kaseifu::models::file_context::{
    CaptureTime, Evidence, EvidenceSource, FileContext, ImageMetadata, NormalizedFileContext,
    TargetFile,
};

// ---------------------------------------------------------------------------
// Fixtures and helpers
// ---------------------------------------------------------------------------

fn temp_path(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-v0401-{}", std::process::id()));
    dir.push(name);
    dir
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create temp dir");
    }
    std::fs::write(&path, bytes).expect("write temp file");
    path
}

fn spotlight_evidence(key: &str, value: &str) -> Evidence {
    Evidence {
        source: EvidenceSource::SpotlightMeta,
        key: key.to_string(),
        value: serde_json::Value::String(value.to_string()),
        attribute: Some(key.to_string()),
    }
}

/// Full image metadata as Spotlight would report it for a geotagged photo.
fn full_image_evidence() -> Vec<Evidence> {
    vec![
        spotlight_evidence("kMDItemContentType", "public.heic"),
        spotlight_evidence("kMDItemEXIFDateTimeOriginal", "2025-08-10 15:30:00 +0000"),
        spotlight_evidence("kMDItemLatitude", "35.658"),
        spotlight_evidence("kMDItemLongitude", "139.702"),
        spotlight_evidence("kMDItemAltitude", "12.5"),
        spotlight_evidence("kMDItemAcquisitionMake", "Apple"),
        spotlight_evidence("kMDItemAcquisitionModel", "iPhone"),
        spotlight_evidence("kMDItemPixelWidth", "4032"),
        spotlight_evidence("kMDItemPixelHeight", "3024"),
    ]
}

/// The same file with no image-specific attributes.
fn plain_evidence() -> Vec<Evidence> {
    vec![spotlight_evidence("kMDItemContentType", "public.jpeg")]
}

fn file_context(path: &Path, evidence: Vec<Evidence>) -> FileContext {
    FileContext {
        schema_version: "file-context-v1".to_string(),
        target: TargetFile {
            path: path.to_path_buf(),
            filename: path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            extension: path
                .extension()
                .map(|ext| ext.to_string_lossy().to_string()),
            size_bytes: 3_200_000,
            modified_at: Some("2025-08-10T15:30:00+00:00".to_string()),
        },
        directory_context: Default::default(),
        evidence,
    }
}

/// Minimal JPEG bytes without EXIF (nothing for the fallback to parse).
fn minimal_jpeg() -> Vec<u8> {
    vec![0xFF, 0xD8, 0xFF, 0xD9]
}

fn normalize_for(path: &Path, evidence: Vec<Evidence>) -> NormalizedFileContext {
    kaseifu::normalize::normalize_with_options(
        file_context(path, evidence),
        &MetadataOptions::default(),
    )
    .expect("normalize")
}

/// Capture the mdfind query Discovery would run for this context.
fn captured_query(normalized: NormalizedFileContext) -> String {
    let queries: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let result = discover_with(normalized, |query, _limit| {
        queries.borrow_mut().push(query.to_string());
        Ok(Vec::new())
    });
    result.expect("discover_with injectable search");
    let queries = queries.borrow();
    assert_eq!(queries.len(), 1, "exactly one search must be issued");
    queries[0].clone()
}

fn sorted_terms(terms: &[String]) -> Vec<String> {
    let mut sorted = terms.to_vec();
    sorted.sort();
    sorted
}

fn terms_of(normalized: &NormalizedFileContext) -> Vec<String> {
    sorted_terms(&kaseifu::discovery::normalize_terms(
        &kaseifu::discovery::extract_terms(normalized),
    ))
}

fn word_set(query: &str) -> Vec<String> {
    let mut words: Vec<String> = query.split(' ').map(ToString::to_string).collect();
    words.sort();
    words
}

// ---------------------------------------------------------------------------
// T4 (acceptance 1): image metadata must not change the search terms
// ---------------------------------------------------------------------------

#[test]
fn v0401_discovery_query_is_identical_with_and_without_image_metadata() {
    let path = write_temp("IMG_3020.jpeg", &minimal_jpeg());

    let with_metadata = normalize_for(&path, full_image_evidence());
    let without_metadata = normalize_for(&path, plain_evidence());

    // Guard: the tagged case must actually produce metadata, otherwise this
    // test could pass vacuously.
    assert!(
        with_metadata.image_metadata.is_some(),
        "fixture must assemble image metadata"
    );
    assert!(
        without_metadata.image_metadata.is_none(),
        "fixture must stay metadata-free"
    );

    // The rule itself: the term list is the same with and without metadata.
    assert_eq!(
        terms_of(&with_metadata),
        terms_of(&without_metadata),
        "image metadata must not change the search terms"
    );
    assert_eq!(
        terms_of(&with_metadata),
        vec!["3020".to_string(), "IMG".to_string()],
        "filename terms only"
    );

    // End-to-end: the mdfind query Discovery would run.
    let with_query = captured_query(with_metadata);
    let without_query = captured_query(without_metadata);
    assert_eq!(
        word_set(&with_query),
        word_set(&without_query),
        "mdfind query must not change with metadata"
    );
    assert!(
        !with_query.contains("available"),
        "availability markers must never become search terms: {with_query}"
    );
}

#[test]
fn v0401_normalized_signals_carry_no_availability_markers() {
    let normalized = production_normalized();
    assert!(
        normalized.image_metadata.is_some(),
        "image metadata is still assembled"
    );
    assert_eq!(
        normalized.semantic_signals,
        vec!["F:IMG".to_string(), "F:1234".to_string()],
        "content signals only"
    );
}

// ---------------------------------------------------------------------------
// T5 (acceptance 2): Jev wire goldens (captured from the pre-fix build)
// ---------------------------------------------------------------------------

type Privacy = (bool, bool);

fn production_normalized() -> NormalizedFileContext {
    let path = write_temp("wire/IMG_1234.HEIC", b"heic-fixture");
    normalize_for(&path, full_image_evidence())
}

fn production_context() -> DecisionContext {
    let normalized = production_normalized();
    let candidates = vec![CandidateDestination {
        id: "D1".to_string(),
        path: PathBuf::from("/tmp/photos"),
        score: 1.0,
        evidence_count: 1,
        reasons: vec![],
    }];
    DecisionContext::from_normalized(&normalized, &candidates)
}

/// DecisionContext with explicit signals and metadata (non-production shapes
/// are only used to pin the adapter's single-source rule).
fn context_with_signals_and_metadata(
    signals: Vec<String>,
    meta: Option<ImageMetadata>,
) -> DecisionContext {
    let normalized = NormalizedFileContext {
        target: TargetFile {
            filename: "IMG_1234.HEIC".to_string(),
            extension: Some("heic".to_string()),
            ..Default::default()
        },
        semantic_signals: signals,
        image_metadata: meta,
        ..Default::default()
    };
    let candidates = vec![CandidateDestination {
        id: "D1".to_string(),
        path: PathBuf::from("/tmp/photos"),
        score: 1.0,
        evidence_count: 1,
        reasons: vec![],
    }];
    DecisionContext::from_normalized(&normalized, &candidates)
}

fn response_body(choice: &str) -> String {
    format!(
        r#"{{"model":"jev-latest","answers":{{"destination":{{"type":"choice","choice":"{choice}","confidence":0.9,"probabilities":{{"D1":0.9,"NONE":0.1}}}}}},"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    )
}

/// One-shot HTTP mock server recording the raw request bytes (std-only).
struct WireTap {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl WireTap {
    fn start(body: String) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let address = listener.local_addr().expect("address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut data = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                if let Some(end) = find_header_end(&data) {
                    let headers = String::from_utf8_lossy(&data[..end]).to_string();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => data.extend_from_slice(&buffer[..read]),
                }
            }
            captured
                .lock()
                .expect("log")
                .push(String::from_utf8_lossy(&data).into_owned());
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        });
        Self {
            endpoint: format!("http://{address}"),
            requests,
        }
    }

    fn request_body(&self) -> String {
        let requests = self.requests.lock().expect("log");
        assert_eq!(requests.len(), 1, "exactly one request");
        let request = &requests[0];
        let (_, body) = request.split_once("\r\n\r\n").expect("request body");
        body.to_string()
    }
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn wire_state_for(context: &DecisionContext, privacy: Privacy) -> serde_json::Value {
    let server = WireTap::start(response_body("D1"));
    let adapter =
        RealJevIntegration::new(server.endpoint.clone(), None).with_privacy(privacy.0, privacy.1);
    adapter.decide(context).await.expect("decision");
    let body: serde_json::Value =
        serde_json::from_str(&server.request_body()).expect("request body is JSON");
    body["state"].clone()
}

async fn wire_state(privacy: Privacy) -> serde_json::Value {
    wire_state_for(&production_context(), privacy).await
}

/// Pre-fix goldens captured from the v0.4.0 build with this same test code
/// (see the plan document §T5 for the capture run). Key order is irrelevant:
/// the comparison is on parsed JSON values.
const GOLDEN_CAPTURE_ON_GPS_ON: &str = r#"{"semantic_signals":["F:IMG","F:1234","M:Capture date available","M:GPS available","M:Camera available"],"target":{"camera":"Apple iPhone","capture_date":"2025-08-10 15:30:00 +0000","dimensions":{"height":3024,"width":4032},"extension":"HEIC","filename":"IMG_1234.HEIC","location":{"altitude_m":12.5,"latitude":35.658,"longitude":139.702},"type":"public.heic"}}"#;
const GOLDEN_CAPTURE_ON_GPS_OFF: &str = r#"{"semantic_signals":["F:IMG","F:1234","M:Capture date available","M:Camera available"],"target":{"camera":"Apple iPhone","capture_date":"2025-08-10 15:30:00 +0000","dimensions":{"height":3024,"width":4032},"extension":"HEIC","filename":"IMG_1234.HEIC","type":"public.heic"}}"#;
const GOLDEN_CAPTURE_OFF_GPS_ON: &str = r#"{"semantic_signals":["F:IMG","F:1234","M:GPS available","M:Camera available"],"target":{"camera":"Apple iPhone","dimensions":{"height":3024,"width":4032},"extension":"HEIC","filename":"IMG_1234.HEIC","location":{"altitude_m":12.5,"latitude":35.658,"longitude":139.702},"type":"public.heic"}}"#;
const GOLDEN_CAPTURE_OFF_GPS_OFF: &str = r#"{"semantic_signals":["F:IMG","F:1234","M:Camera available"],"target":{"camera":"Apple iPhone","dimensions":{"height":3024,"width":4032},"extension":"HEIC","filename":"IMG_1234.HEIC","type":"public.heic"}}"#;

/// The invariant: the `state` sent to Jev is identical to v0.4.0 for every
/// privacy combination (capture on/off x GPS on/off); the availability
/// markers are recreated by the adapter from `image_metadata`.
#[tokio::test]
async fn v0401_jev_wire_state_matches_v0400_goldens() {
    let cases: [(&str, Privacy, &str); 4] = [
        ("capture=on gps=on", (true, true), GOLDEN_CAPTURE_ON_GPS_ON),
        (
            "capture=on gps=off",
            (true, false),
            GOLDEN_CAPTURE_ON_GPS_OFF,
        ),
        (
            "capture=off gps=on",
            (false, true),
            GOLDEN_CAPTURE_OFF_GPS_ON,
        ),
        (
            "capture=off gps=off",
            (false, false),
            GOLDEN_CAPTURE_OFF_GPS_OFF,
        ),
    ];

    for (label, privacy, golden) in cases {
        let state = wire_state(privacy).await;
        let expected: serde_json::Value = serde_json::from_str(golden).expect("golden JSON");
        assert_eq!(state, expected, "wire state changed for {label}: {state}");
    }
}

/// Privacy gating stays observable: a withheld field leaves no marker behind.
#[tokio::test]
async fn v0401_jev_markers_respect_privacy_gating() {
    let redacted = wire_state((false, false)).await;
    let signals = redacted["semantic_signals"]
        .as_array()
        .expect("signal list")
        .iter()
        .map(|signal| signal.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(signals, vec!["F:IMG", "F:1234", "M:Camera available"]);
    assert!(
        redacted["target"].get("capture_date").is_none(),
        "capture withheld leaves no trace: {redacted}"
    );
    assert!(
        redacted["target"].get("location").is_none(),
        "GPS withheld leaves no trace: {redacted}"
    );
}

/// Single source of truth: markers are derived from `image_metadata`, never
/// forwarded from the incoming signal list (a stale marker without backing
/// metadata is not sent, and nothing is sent twice).
#[tokio::test]
async fn v0401_markers_are_derived_from_metadata_only() {
    let capture_only = ImageMetadata {
        capture_time: Some(CaptureTime {
            value: "2025-08-10T15:30:00".to_string(),
            timezone: None,
            source: "exif".to_string(),
            timezone_known: false,
        }),
        ..Default::default()
    };
    let context = context_with_signals_and_metadata(
        vec![
            "F:IMG".to_string(),
            "M:Capture date available".to_string(),
            "M:GPS available".to_string(),
            "M:Camera available".to_string(),
        ],
        Some(capture_only),
    );

    let state = wire_state_for(&context, (true, true)).await;
    let signals = state["semantic_signals"]
        .as_array()
        .expect("signal list")
        .iter()
        .map(|signal| signal.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        signals,
        vec!["F:IMG", "M:Capture date available"],
        "stale markers without backing metadata must not be forwarded: {state}"
    );
}

//! Phase 4D/4E - RealJevIntegration adapter tests.
//!
//! These tests never touch a live API. They run against a local mock HTTP
//! server (std-only, no extra dependencies) and protect the adapter
//! boundary against the **real TypeSafe /v1/systemone contract**:
//!
//! ```text
//! DecisionContext -> SystemOne request DTO -> HTTP
//!                 -> SystemOne response (answers.<name>.choice/probabilities)
//!                 -> JevResponse
//! ```
//!
//! Domain concerns (candidate membership, probability validation,
//! confidence/margin, decision construction) are NOT tested here; they are
//! owned by `tests/phase4_jev_decision_test.rs` and the pipeline tests.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use kaseifu::classifier::jev::{JevError, JevIntegration, RealJevIntegration};
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::DecisionContext;
use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};

const SYSTEMONE_PATH: &str = "/v1/systemone";

// ---------------------------------------------------------------------------
// Minimal HTTP mock server
// ---------------------------------------------------------------------------

enum MockReply {
    /// Answer with the given status code and JSON body.
    Status { code: u16, body: String },
    /// Accept the connection, then close it without a response.
    Abort,
}

struct MockJevServer {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    _handle: JoinHandle<()>,
}

impl MockJevServer {
    fn start(replies: Vec<MockReply>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
        let address = listener.local_addr().expect("mock server address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);

        let handle = std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let request = read_http_request(&mut stream);
                captured.lock().expect("request log").push(request);

                match reply {
                    MockReply::Status { code, body } => {
                        write_http_response(&mut stream, code, &body)
                    }
                    MockReply::Abort => {
                        // Dropping the stream closes the connection without a response.
                    }
                }
            }
        });

        Self {
            endpoint: format!("http://{address}"),
            requests,
            _handle: handle,
        }
    }

    fn captured_requests(&self) -> Vec<String> {
        self.requests.lock().expect("request log").clone()
    }
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

    let mut data = Vec::new();
    let mut buffer = [0u8; 1024];

    loop {
        if let Some(header_end) = find_subslice(&data, b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..header_end]).to_string();
            let content_length = headers
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

            if data.len() >= header_end + 4 + content_length {
                break;
            }
        }

        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    }

    String::from_utf8_lossy(&data).into_owned()
}

fn write_http_response(stream: &mut TcpStream, code: u16, body: &str) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        _ => "Status",
    };

    let response = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---------------------------------------------------------------------------
// Fixtures and helpers
// ---------------------------------------------------------------------------

fn context() -> DecisionContext {
    DecisionContext::from_candidates(vec!["D1".to_string(), "D2".to_string()])
}

fn adapter(server: &MockJevServer) -> RealJevIntegration {
    RealJevIntegration::new(server.endpoint.clone(), None)
}

/// A real TypeSafe `/v1/systemone` success envelope with one `choice` answer
/// named `destination`.
fn systemone_choice_response(choice: &str, probabilities: &str) -> String {
    format!(
        r#"{{"model":"jev-latest","answers":{{"destination":{{"type":"choice","choice":"{choice}","confidence":0.9,"probabilities":{probabilities}}}}},"usage":{{"input_tokens":120,"output_tokens":12}}}}"#
    )
}

/// A populated, production-shaped decision context.
fn populated_context() -> DecisionContext {
    let normalized = NormalizedFileContext {
        target: TargetFile {
            filename: "CBX（カンナビノイド）とは？｜NATURECAN.png".to_string(),
            extension: Some("png".to_string()),
            ..TargetFile::default()
        },
        semantic_signals: vec!["X:CBX".to_string()],
        file_type: Some("public.png".to_string()),
        ..NormalizedFileContext::default()
    };
    let candidates = vec![CandidateDestination {
        id: "D1".to_string(),
        path: PathBuf::from("/tmp/candidate-dir"),
        score: 15.0,
        evidence_count: 2,
        reasons: vec!["found 2 items".to_string()],
    }];

    DecisionContext::from_normalized(&normalized, &candidates)
}

// ---------------------------------------------------------------------------
// Success path: real response envelope -> JevResponse
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase4_real_jev_adapter_valid_candidate_response_maps_to_jev_response() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D1", r#"{"D1":0.91,"D2":0.06,"NONE":0.03}"#),
    }]);

    let response = adapter(&server)
        .decide(&context())
        .await
        .expect("valid response");

    assert_eq!(response.destination_id, Some("D1".to_string()));
    assert_eq!(response.probabilities.get("D1"), Some(&0.91));
    assert_eq!(response.probabilities.len(), 3);

    // Confidence/margin are domain-owned: the API's own confidence is NOT copied.
    assert_eq!(response.confidence, 0.0);
    assert_eq!(response.margin, 0.0);
}

#[tokio::test]
async fn phase4_real_jev_adapter_none_response_is_a_decision_not_an_error() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("NONE", r#"{"NONE":0.70,"D1":0.20,"D2":0.10}"#),
    }]);

    let response = adapter(&server)
        .decide(&context())
        .await
        .expect("NONE is a decision");

    assert_eq!(response.destination_id, Some("NONE".to_string()));
    assert_eq!(response.probabilities.get("NONE"), Some(&0.70));
}

#[tokio::test]
async fn phase4_real_jev_adapter_multiple_candidate_probabilities_are_preserved() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D2", r#"{"D1":0.11,"D2":0.82,"NONE":0.07}"#),
    }]);

    let response = adapter(&server)
        .decide(&context())
        .await
        .expect("valid response");

    let mut expected = HashMap::new();
    expected.insert("D1".to_string(), 0.11);
    expected.insert("D2".to_string(), 0.82);
    expected.insert("NONE".to_string(), 0.07);

    assert_eq!(response.destination_id, Some("D2".to_string()));
    assert_eq!(response.probabilities, expected);
}

#[tokio::test]
async fn phase4_real_jev_adapter_unknown_candidate_passes_through_for_domain_rejection() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D99", r#"{"D99":0.95}"#),
    }]);

    let response = adapter(&server)
        .decide(&context())
        .await
        .expect("adapter maps, domain validates");
    assert_eq!(response.destination_id, Some("D99".to_string()));

    // The domain is the authority that rejects it.
    assert!(context().validate_decision("D99").is_err());
}

#[tokio::test]
async fn phase4_real_jev_adapter_arbitrary_path_response_never_becomes_a_destination() {
    let server = MockJevServer::start(vec![
        MockReply::Status {
            code: 200,
            body: systemone_choice_response("/Users/foo/Documents", r#"{}"#),
        },
        MockReply::Status {
            code: 200,
            body: systemone_choice_response("../../etc/passwd", r#"{}"#),
        },
        MockReply::Status {
            code: 200,
            body: systemone_choice_response("file:///etc/passwd", r#"{}"#),
        },
    ]);

    let adapter = adapter(&server);
    let decision_context = context();

    for attempt in 0..3 {
        let response = adapter
            .decide(&decision_context)
            .await
            .unwrap_or_else(|error| panic!("attempt {attempt} should map, got {error}"));

        let choice = response.destination_id.expect("choice is present");
        // The adapter passes the string through verbatim; it never converts it
        // into a filesystem destination.
        assert!(
            choice.contains('/') || choice.contains(':'),
            "expected a path-like choice, got {choice}"
        );
        // The domain refuses to treat it as a candidate.
        assert!(
            decision_context.validate_decision(&choice).is_err(),
            "domain must reject {choice}"
        );
    }
}

// ---------------------------------------------------------------------------
// Request shape: real /v1/systemone schema
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase4_real_jev_adapter_request_uses_systemone_schema() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D1", r#"{"D1":0.9,"NONE":0.1}"#),
    }]);

    let _ = adapter(&server)
        .decide(&context())
        .await
        .expect("valid response");

    let requests = server.captured_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];

    assert!(
        request.starts_with(&format!("POST {SYSTEMONE_PATH}")),
        "request was: {request}"
    );
    assert!(
        request
            .to_lowercase()
            .contains("content-type: application/json"),
        "request was: {request}"
    );
    // Real schema: questions.<name> is a choice question with criteria.
    assert!(request.contains("\"questions\""), "request was: {request}");
    assert!(
        request.contains("\"destination\""),
        "request was: {request}"
    );
    assert!(
        request.contains("\"type\":\"choice\""),
        "request was: {request}"
    );
    assert!(request.contains("\"criteria\""), "request was: {request}");
    assert!(request.contains("\"model\""), "request was: {request}");
    assert!(request.contains("\"state\""), "request was: {request}");
    // Candidates become criteria names, and NONE is always offered.
    assert!(request.contains("\"D1\""), "request was: {request}");
    assert!(request.contains("\"D2\""), "request was: {request}");
    assert!(request.contains("\"NONE\""), "request was: {request}");
}

#[tokio::test]
async fn phase4_real_jev_adapter_accepts_a_full_endpoint_url() {
    // JEV_API_URL may be configured as the full endpoint URL (as in .env):
    // the adapter must not append the path a second time.
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D1", r#"{"D1":0.9,"NONE":0.1}"#),
    }]);

    let adapter = RealJevIntegration::new(format!("{}/v1/systemone", server.endpoint), None);
    let _ = adapter.decide(&context()).await.expect("valid response");

    let requests = server.captured_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with(&format!("POST {SYSTEMONE_PATH} HTTP")),
        "endpoint path must not be duplicated: {}",
        requests[0]
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_api_key_is_sent_as_bearer_header_when_configured() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D1", r#"{"D1":0.9}"#),
    }]);

    let adapter = RealJevIntegration::new(server.endpoint.clone(), Some("test-key".to_string()));
    let _ = adapter.decide(&context()).await.expect("valid response");

    let requests = server.captured_requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].to_lowercase().contains("bearer test-key"),
        "auth header missing in: {}",
        requests[0]
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_serializes_populated_context_fields() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: systemone_choice_response("D1", r#"{"D1":0.9,"NONE":0.1}"#),
    }]);

    let response = adapter(&server)
        .decide(&populated_context())
        .await
        .expect("valid response");
    assert_eq!(response.destination_id, Some("D1".to_string()));

    let requests = server.captured_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];

    // The adapter serializes what the domain provides; it never re-derives it.
    assert!(
        request.contains("NATURECAN"),
        "filename missing in request: {request}"
    );
    assert!(
        request.contains("\"png\""),
        "extension missing in request: {request}"
    );
    assert!(
        request.contains("public.png"),
        "file type missing in request: {request}"
    );
    assert!(
        request.contains("X:CBX"),
        "semantic signals missing in request: {request}"
    );
    // Criteria descriptions carry the real candidate destination.
    assert!(
        request.contains("/tmp/candidate-dir"),
        "candidate path missing in request: {request}"
    );
    assert!(
        request.contains("2 related file"),
        "candidate evidence missing in request: {request}"
    );
}

// ---------------------------------------------------------------------------
// Response schema failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase4_real_jev_adapter_malformed_json_is_rejected() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: r#"{"answers": "#.to_string(),
    }]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("malformed JSON must be rejected");

    assert!(
        matches!(error, JevError::InvalidResponse(_)),
        "expected InvalidResponse, got {error:?}"
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_missing_answer_is_rejected() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: r#"{"model":"jev-latest","answers":{},"usage":{"input_tokens":1,"output_tokens":1}}"#
            .to_string(),
    }]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("a response without the destination answer must be rejected");

    assert!(
        matches!(error, JevError::InvalidResponse(_)),
        "expected InvalidResponse, got {error:?}"
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_non_choice_answer_is_rejected() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: r#"{"model":"jev-latest","answers":{"destination":{"type":"noul","noul":0.5}},"usage":{"input_tokens":1,"output_tokens":1}}"#
            .to_string(),
    }]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("a non-choice answer must be rejected");

    assert!(
        matches!(error, JevError::InvalidResponse(_)),
        "expected InvalidResponse, got {error:?}"
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_answer_without_choice_is_rejected() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 200,
        body: r#"{"model":"jev-latest","answers":{"destination":{"type":"choice","probabilities":{"D1":0.9}}},"usage":{"input_tokens":1,"output_tokens":1}}"#
            .to_string(),
    }]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("a choice answer without a selection must be rejected");

    assert!(
        matches!(error, JevError::InvalidResponse(_)),
        "expected InvalidResponse, got {error:?}"
    );
}

// ---------------------------------------------------------------------------
// HTTP status failures (ERROR is never converted to NONE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase4_real_jev_adapter_http_401_is_an_authentication_error() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 401,
        body: r#"{"detail":"unauthorized"}"#.to_string(),
    }]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("401 must fail");

    assert!(
        matches!(error, JevError::Authentication(_)),
        "expected Authentication, got {error:?}"
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_http_422_is_an_api_error() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 422,
        body:
            r#"{"detail":[{"loc":["body","questions"],"msg":"Field required","type":"missing"}]}"#
                .to_string(),
    }]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("422 must fail");

    assert!(
        matches!(error, JevError::ApiError(_)),
        "expected ApiError, got {error:?}"
    );
}

#[tokio::test]
async fn phase4_real_jev_adapter_http_500_is_a_server_error_and_never_none() {
    let server = MockJevServer::start(vec![MockReply::Status {
        code: 500,
        body: r#"{"detail":"boom"}"#.to_string(),
    }]);

    let result = adapter(&server).decide(&context()).await;

    match result {
        Err(JevError::ServerError(message)) => assert!(
            message.contains("500"),
            "error should carry the status: {message}"
        ),
        Err(other) => panic!("expected ServerError, got {other:?}"),
        Ok(response) => panic!(
            "a failed decision must never become a response (NONE or otherwise): {response:?}"
        ),
    }
}

#[tokio::test]
async fn phase4_real_jev_adapter_transport_failure_is_a_transport_error() {
    let server = MockJevServer::start(vec![MockReply::Abort]);

    let error = adapter(&server)
        .decide(&context())
        .await
        .expect_err("a dropped connection must fail");

    assert!(
        matches!(error, JevError::TransportError(_)),
        "expected TransportError, got {error:?}"
    );
}

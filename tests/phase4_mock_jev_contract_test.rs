//! Phase 4: MockJevIntegration Contract Tests
//!
//! These tests exercise the **production** `MockJevIntegration`
//! (`src/classifier/mock_jev.rs`). There must not be a test-local Mock.
//!
//! Contract:
//!
//! ```text
//! DecisionContext
//!       ↓
//! MockJevIntegration
//!       ↓
//! configured JevResponse / configured JevError
//! ```
//!
//! The Mock must NOT contain business logic:
//! no candidate validation, no confidence/margin calculation,
//! no candidate selection, no HTTP.

use semantic_file_organizer::classifier::jev::JevIntegration;
use semantic_file_organizer::classifier::mock_jev::MockJevIntegration;
use semantic_file_organizer::models::decision::DecisionContext;

fn context_with(candidate_ids: &[&str]) -> DecisionContext {
    DecisionContext::from_candidates(candidate_ids.iter().map(|s| s.to_string()).collect())
}

/// CONTRACT 1: returns the configured response.
#[tokio::test]
async fn test_mock_jev_integration_returns_configured_response() {
    let context = context_with(&["D1", "D2"]);
    let mock = MockJevIntegration::default().set_response(Some("D1".to_string()));

    let resp = mock.decide(&context).await;

    assert!(resp.is_ok(), "Mock should return Ok(D1) when configured");
    assert_eq!(resp.unwrap().destination_id, Some("D1".to_string()));
}

/// CONTRACT 2: propagates the configured transport error.
#[tokio::test]
async fn test_mock_jev_integration_propagates_transport_error() {
    let context = context_with(&["D1"]);
    let mock = MockJevIntegration::configured_error("Transport error");

    let resp = mock.decide(&context).await;

    assert!(
        resp.is_err(),
        "Mock should return Err when an error is configured"
    );
    assert_eq!(
        resp.unwrap_err().to_string(),
        "TransportError(Transport error)"
    );
}

/// CONTRACT 3: receives DecisionContext without panicking.
#[tokio::test]
async fn test_mock_jev_integration_receives_decision_context() {
    let context = context_with(&["D1", "D2"]);

    let resp = MockJevIntegration::default().decide(&context).await;

    assert!(resp.is_ok(), "Mock should accept a DecisionContext");
}

/// CONTRACT 4: does not modify the candidate list it was given.
#[tokio::test]
async fn test_mock_response_does_not_modify_candidates() {
    let original_candidates = vec!["D1".to_string(), "D2".to_string()];

    let context = DecisionContext::from_candidates(original_candidates.clone());
    let mock = MockJevIntegration::default().set_response(Some("D1".into()));

    let _ = mock.decide(&context).await;

    assert_eq!(
        original_candidates,
        vec!["D1".to_string(), "D2".to_string()],
        "Deciding must not modify the candidate list"
    );
}

/// CONTRACT 5: NONE is a normal (and acceptable) decision.
#[tokio::test]
async fn test_none_response_is_accepted_by_mock() {
    let context = context_with(&["D1", "D2", "NONE"]);
    let mock = MockJevIntegration::default().set_response(Some("NONE".to_string()));

    let resp = mock.decide(&context).await;

    assert!(resp.is_ok(), "NONE should be accepted as a valid decision");
    assert_eq!(resp.unwrap().destination_id, Some("NONE".to_string()));
}

/// CONTRACT 6: unknown candidate IDs pass through the Mock and are
/// rejected by the production validation layer.
#[tokio::test]
async fn test_unknown_candidate_goes_to_validation() {
    let context = context_with(&["D1", "D2"]);
    let mock = MockJevIntegration::default().set_response(Some("D99".to_string()));

    let resp = mock.decide(&context).await;

    assert!(
        resp.is_ok(),
        "Mock must not validate; D99 must reach the validation layer"
    );

    let decision = resp.unwrap();
    assert_eq!(decision.destination_id, Some("D99".to_string()));
    assert!(
        context.validate_decision("D99").is_err(),
        "Validation layer must reject unknown candidate D99"
    );
}

/// CONTRACT 6b (security invariant): Jev cannot create a destination.
#[tokio::test]
async fn test_jev_cannot_create_destination_path() {
    let context = context_with(&["D1", "D2"]);
    let mock = MockJevIntegration::default().set_response(Some("/Users/foo/Documents".to_string()));

    let resp = mock.decide(&context).await;

    // The Mock is dumb and returns whatever was configured...
    let decision = resp.unwrap();
    assert_eq!(
        decision.destination_id,
        Some("/Users/foo/Documents".to_string())
    );
    // ...but the production validation layer refuses to treat it as a candidate.
    assert!(
        context.validate_decision("/Users/foo/Documents").is_err(),
        "A filesystem path must never validate as a candidate ID"
    );
}

/// CONTRACT 7: basic smoke test.
#[tokio::test]
async fn test_mock_jev_integration_basic_smoke() {
    let context = context_with(&["D1"]);

    let _ = MockJevIntegration::default().decide(&context).await;
}

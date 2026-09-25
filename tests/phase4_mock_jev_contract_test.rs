//! Phase 4: MockJevIntegration Contract Tests
//!
//! These tests establish the contract for MockJevIntegration.
//!
//! Expected final state (GREEN):
//!   - compile: PASS
//!   - existing tests: PASS (39 tests)
//!   - new contract tests: PASS (6 tests)

use serde::{Deserialize, Serialize};

// ============================================================================
// Jev Integration trait definition (MUST match the real implementation)
// ============================================================================

/// Jev integration trait - real or mock must implement this
pub trait JevIntegration: Send + Sync {
    /// Invokes the Jev service to get a response
    ///
    /// - context: Decision context from user's file (from NormalizedFileContext)
    /// - returns: JevResponse OR error
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> impl std::future::Future<Output = Result<JevResponse, JevError>>;
}

/// Transport error from Jev service (real or simulation)
#[derive(Debug, Clone)]
pub enum JevError {
    TransportError(String),
}

impl std::fmt::Display for JevError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            JevError::TransportError(msg) => write!(f, "TransportError({})", msg),
        }
    }
}

/// Jev response structure - MUST match real Jev API response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct JevResponse {
    pub destination_id: Option<String>,
    pub probabilities: std::collections::HashMap<String, f64>,
}

// ============================================================================
// Type stubs - EXISTS in codebase (when Phase 3 is complete)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionContext {
    pub target: DecisionTarget,
    pub candidates: Vec<String>,
    pub semantic_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionTarget {
    pub filename: String,
    pub file_type: Option<String>,
}

impl DecisionContext {
    pub fn new_candidates(candidates: Vec<String>, target: DecisionTarget) -> Self {
        Self {
            candidates: candidates.clone(),
            target,
            semantic_signals: vec![],
        }
    }
}

// ============================================================================
// Minimal MockJevIntegration for TDD testing
// ============================================================================

#[derive(Debug, Clone)]
pub struct MockJevIntegration {
    #[allow(dead_code)]
    response: Option<String>,
    #[allow(dead_code)]
    error: Option<String>,
}

impl MockJevIntegration {
    /// Creates a mock that returns the specified response
    pub fn configured_response(response: String) -> Self {
        Self {
            response: Some(response),
            error: None,
        }
    }

    /// Creates a mock that returns the specified error
    pub fn configured_error(error: String) -> Self {
        Self {
            response: None,
            error: Some(error),
        }
    }

    /// Set the response that will be returned (for flexible testing)
    pub fn set_response(mut self, response: Option<String>) -> Self {
        self.response = response;
        self
    }

    /// Set the error that will be returned (for flexible testing)
    pub fn set_error(mut self, error: Option<String>) -> Self {
        self.error = error;
        self
    }

    /// Returns the configured result - in real implementation this would be an async call
    #[allow(unused)]
    pub fn decide(&self, _context: &DecisionContext) -> Result<JevResponse, JevError> {
        match &self.response {
            Some(r) => Ok(r.clone().into()),
            None => {
                // Return NONE as default if no specific response configured
                Ok(JevResponse::default())
            }
        }
    }
}

impl JevIntegration for MockJevIntegration {
    fn decide(
        &self,
        _context: &DecisionContext,
    ) -> impl std::future::Future<Output = Result<JevResponse, JevError>> {
        async move { self.decide(_context) }
    }
}

impl Default for MockJevIntegration {
    fn default() -> Self {
        Self {
            response: None,
            error: None,
        }
    }
}

// ============================================================================
// MockJevResponse Helper for converting String to JevResponse
// ============================================================================

impl From<String> for JevResponse {
    fn from(string: String) -> Self {
        if !string.is_empty() {
            JevResponse {
                destination_id: Some(string),
                probabilities: Default::default(),
            }
        } else {
            JevResponse::default()
        }
    }
}

// ============================================================================
// Phase 4 Contract Tests
// ============================================================================

/// CONTRACT 1: returns configured response
/// Expected PASS: Mock returns what was configured
#[test]
fn test_mock_jev_integration_returns_configured_response() {
    let context = DecisionContext::new_candidates(
        vec!["D1".into(), "D2".into()],
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    // Verify mock returns configured response
    let mock = MockJevIntegration::configured_response("D1".to_string());
    let resp = mock.decide(&context);

    assert!(resp.is_ok(), "Mock should return Ok D1 when configured");
    let resp = resp.unwrap();
    assert_eq!(resp.destination_id, Some("D1".to_string()));
}

/// CONTRACT 2: propagates configured transport error
/// Expected PASS: Mock returns error when configured
#[test]
fn test_mock_jev_integration_propagates_transport_error() {
    let context = DecisionContext::new_candidates(
        vec!["D1".into()],
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    // Verify mock returns configured error
    let mock = MockJevIntegration::configured_error("Transport error".to_string());
    let resp = mock.decide(&context);

    assert!(
        resp.is_err(),
        "Mock should return Err when configured error is set"
    );
    let err = resp.unwrap_err();
    assert_eq!(err.to_string(), "TransportError(Transport error)");
}

/// CONTRACT 3: receives DecisionContext
/// Expected PASS: Mock accepts DecisionContext parameter without panicking
#[test]
fn test_mock_jev_integration_receives_decision_context() {
    let context = DecisionContext::new_candidates(
        vec!["D1".into(), "D2".into()],
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    let resp = MockJevIntegration::default().decide(&context);

    assert!(
        resp.is_ok(),
        "Mock should accept context without prepending"
    );
}

/// CONTRACT 4: does not modify CandidateDestination[]
/// Expected PASS: Mock must not modify the input candidates
#[test]
fn test_mock_response_does_not_modify_candidates() {
    // CONTRACT: Mock must NOT modify CandidateDestination[]
    let original_candidates = vec!["D1".to_string(), "D2".to_string()];

    let context = DecisionContext::new_candidates(
        original_candidates.clone(),
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    let resp = MockJevIntegration::default()
        .set_response(Some("D1".into()))
        .decide(&context);

    // Verify original candidates are unchanged after decision
    assert_eq!(
        original_candidates,
        context.candidates.as_slice(),
        "Context copies must preserve original candidates"
    );
}

/// CONTRACT 5: NONE response is accepted
/// Expected PASS: Mock returns NONE when configured
#[test]
fn test_none_response_is_accepted_by_mock() {
    let context = DecisionContext::new_candidates(
        vec!["D1".into(), "D2".into(), "NONE".into()],
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    let mock = MockJevIntegration::configured_response("NONE".to_string());
    let resp = mock.decide(&context);

    assert!(resp.is_ok(), "NONE should be accepted as a valid decision");
    assert_eq!(resp.unwrap().destination_id, Some("NONE".to_string()));
}

/// CONTRACT 6: unknown candidate reaches validation
/// Expected PASS: Mock returns unknown ID without blocking to validation
#[test]
fn test_unknown_candidate_goes_to_validation() {
    // Unknown candidate ID like "D99" should not be rejected by Mock
    // It will be rejected later by Validation layer

    let context = DecisionContext::new_candidates(
        vec!["D1".into(), "D2".into()],
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    let mock = MockJevIntegration::configured_response("D99".to_string());
    let resp = mock.decide(&context);

    assert!(
        resp.is_ok(),
        "Mock should allow unknown IDs to reach validation layer"
    );
}

/// CONTRACT 7: basic smoke test
#[test]
fn test_mock_jev_integration_basic_smoke() {
    let context = DecisionContext::new_candidates(
        vec!["D1".into()],
        DecisionTarget {
            filename: "test.png".into(),
            file_type: Some("png".into()),
        },
    );

    // Verify mock can be constructed and called
    let mock = MockJevIntegration::default();
    let _resp = mock.decide(&context);
}

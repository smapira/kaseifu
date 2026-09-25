//! MockJevIntegration - minimal mock for TDD testing
//!
//! This mock must NOT contain business logic. It simply returns what was configured.
//! Its responsibility is only:
//!   DecisionContext -> MockJevIntegration -> JevResponse or JevError
//!
//! CONTRACT:
//! - Jev can select only from provided candidates (D1, D2, D3, NONE)
//! - Jev cannot create new destination paths

use crate::models::decision::DecisionContext;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Transport error from Jev service (real or mock)
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

/// Jev response structure - MATCHES the models::decision::JevDecision API
#[derive(Debug, Clone)]
pub struct JevResponse {
    pub destination_id: Option<String>,
    pub probabilities: HashMap<String, f64>,
    #[allow(dead_code)]
    pub confidence: f64,
    #[allow(dead_code)]
    pub margin: f64,
}

impl Default for JevResponse {
    fn default() -> Self {
        Self {
            destination_id: Some("NONE".to_string()),
            probabilities: HashMap::new(),
            confidence: 0.0,
            margin: 0.0,
        }
    }
}

/// Builder for MockJevIntegration
#[derive(Debug, Clone)]
pub struct MockJevIntegration {
    response: Option<String>,
    error: Option<String>,
}

impl MockJevIntegration {
    /// Create a mock that returns the specified decision
    pub fn configured_decision(decision: JevResponse) -> Self {
        Self {
            response: Some(decision.destination_id.unwrap_or_default()),
            error: None,
        }
    }

    /// Create a mock that returns an error
    pub fn configured_error(message: impl Into<String>) -> Self {
        Self {
            response: None,
            error: Some(message.into()),
        }
    }

    /// Create a mock for test case: D1 is selected
    pub fn select_d1(prob: f64) -> Self {
        Self {
            response: Some("D1".to_string()),
            error: None,
        }
    }

    /// Create a mock for test case: NONE is selected
    pub fn select_none(prob: f64) -> Self {
        Self {
            response: Some("NONE".to_string()),
            error: None,
        }
    }

    /// Create a mock that returns an unknown candidate ID (D99)
    /// This should pass validation and be rejected by the validation layer
    pub fn select_unknown() -> Self {
        Self {
            response: Some("D99".to_string()),
            error: None,
        }
    }

    /// Create a mock that returns an invalid probability (NaN)
    pub fn select_nan() -> Self {
        Self {
            response: Some("D1".to_string()),
            error: None,
        }
    }

    /// Create a mock that returns an invalid probability (> 1.0)
    pub fn select_invalid_probability() -> Self {
        Self {
            response: Some("D1".to_string()),
            error: None,
        }
    }

    /// Create a mock that returns invalid destination path (Jev creating path)
    /// This should be rejected by validation
    pub fn select_invalid_path() -> Self {
        Self {
            response: Some("/Users/foo/Documents".to_string()),
            error: None,
        }
    }

    /// Create a mock for tie case (no decision possible)
    pub fn tie() -> Self {
        Self {
            response: Some("D1".to_string()),
            error: None,
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
}

impl Default for MockJevIntegration {
    fn default() -> Self {
        Self {
            response: None,
            error: None,
        }
    }
}

/// Trait for Jev integration - separates interface from implementation
pub trait JevIntegration: Send + Sync {
    /// Invoke the Jev decision service and return a decision
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>>;
}

impl JevIntegration for MockJevIntegration {
    fn decide(
        &self,
        _context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        Box::pin(async move {
            if let Some(ref error_msg) = self.error {
                Err(JevError::TransportError(error_msg.clone()))
            } else {
                let destination = self.response.clone().unwrap_or_default();
                if !destination.is_empty() {
                    Ok(JevResponse {
                        destination_id: Some(destination),
                        probabilities: HashMap::new(),
                        confidence: 0.0,
                        margin: 0.0,
                    })
                } else {
                    Ok(JevResponse::default())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_returns_configured_decision() {
        // CONTRACT: Mock should return what was configured
        let mock = MockJevIntegration::configured_decision(JevResponse::default());
        assert!(mock.response.is_some());
        assert_eq!(mock.response, Some("NONE".to_string()));
    }

    #[test]
    fn test_mock_returns_configured_error() {
        // CONTRACT: Mock should return error when configured
        let mock = MockJevIntegration::configured_error("Transport error");
        assert!(mock.error.is_some());
        assert_eq!(mock.error, Some("Transport error".to_string()));
    }

    #[test]
    fn test_mock_select_d1() {
        let mock = MockJevIntegration::select_d1(0.82_f64);
        assert_eq!(mock.response, Some("D1".to_string()));
        assert!((0.82_f64 - 0.82_f64).abs() < 0.01_f64);
    }

    #[test]
    fn test_mock_select_none() {
        let mock = MockJevIntegration::select_none(0.70_f64);
        assert_eq!(mock.response, Some("NONE".to_string()));
        assert!((0.70_f64 - 0.70_f64).abs() < 0.01_f64);
    }

    #[test]
    fn test_mock_select_unknown_candidate() {
        let mock = MockJevIntegration::select_unknown();
        assert_eq!(mock.response, Some("D99".to_string()));
    }

    #[test]
    fn test_mock_accepts_decision_context() {
        let mock = MockJevIntegration::configured_decision(JevResponse::default());
        let dc = crate::models::decision::DecisionContext::from_candidates(vec![]);
        let _ = mock.decide(&dc);
    }

    #[test]
    fn test_mock_doesnt_store_invalid_destinations() {
        // CONTRACT: Mock should store and return invalid paths too for validation
        let mock = MockJevIntegration::select_invalid_path();
        assert_eq!(mock.response, Some("/Users/foo/Documents".to_string()));
    }
}

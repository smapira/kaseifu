//! MockJevIntegration - minimal mock for TDD testing
//!
//! This mock must NOT contain business logic. It simply returns what was configured.
//! Its responsibility is only:
//!   DecisionContext -> MockJevIntegration -> JevResponse or JevError
//!
//! CONTRACT:
//! - Jev can select only from provided candidates (D1, D2, D3, NONE)
//! - Jev cannot create new destination paths
//! - The mock never validates, scores, or selects; it returns configured data

use super::jev::{JevError, JevIntegration, JevResponse};
use crate::models::decision::DecisionContext;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// MockJevIntegration - returns the configured response verbatim.
#[derive(Debug, Clone, Default)]
pub struct MockJevIntegration {
    result: Option<JevResponse>,
    error: Option<JevError>,
}

impl MockJevIntegration {
    /// Create a mock that returns the specified decision verbatim
    pub fn configured_decision(decision: JevResponse) -> Self {
        Self {
            result: Some(decision),
            error: None,
        }
    }

    /// Create a mock that returns an error
    pub fn configured_error(message: impl Into<String>) -> Self {
        Self {
            result: None,
            error: Some(JevError::TransportError(message.into())),
        }
    }

    /// Create a mock for test case: D1 is selected
    pub fn select_d1(prob: f64) -> Self {
        Self::select("D1", prob)
    }

    /// Create a mock for test case: NONE is selected
    pub fn select_none(prob: f64) -> Self {
        Self::select("NONE", prob)
    }

    /// Create a mock that returns an unknown candidate ID (D99)
    /// This passes through the mock and is rejected by the validation layer
    pub fn select_unknown() -> Self {
        Self::select("D99", 0.95)
    }

    /// Create a mock that returns an invalid probability (NaN)
    pub fn select_nan() -> Self {
        Self::select("D1", f64::NAN)
    }

    /// Create a mock that returns an invalid probability (> 1.0)
    pub fn select_invalid_probability() -> Self {
        Self::select("D1", 1.5)
    }

    /// Create a mock that returns a destination path (Jev creating a path)
    /// This passes through the mock and is rejected by the validation layer
    pub fn select_invalid_path() -> Self {
        Self::select("/Users/foo/Documents", 0.95)
    }

    /// Create a mock for a tie case (equal top probabilities)
    pub fn tie() -> Self {
        let mut probabilities = HashMap::new();
        probabilities.insert("D1".to_string(), 0.50);
        probabilities.insert("D2".to_string(), 0.50);
        Self::configured_decision(JevResponse {
            destination_id: Some("D1".to_string()),
            probabilities,
            confidence: 0.0,
            margin: 0.0,
        })
    }

    /// Set the response destination that will be returned
    pub fn set_response(mut self, response: Option<String>) -> Self {
        self.result = response.map(|id| JevResponse {
            destination_id: Some(id),
            ..JevResponse::default()
        });
        self
    }

    /// Set the error that will be returned
    pub fn set_error(mut self, error: Option<String>) -> Self {
        self.error = error.map(JevError::TransportError);
        self
    }

    /// Set the probability distribution that will be returned
    pub fn set_probabilities(mut self, probabilities: HashMap<String, f64>) -> Self {
        let mut result = self.result.unwrap_or_default();
        result.probabilities = probabilities;
        self.result = Some(result);
        self
    }

    /// Create a single-candidate selection with one configured probability
    fn select(destination: &str, prob: f64) -> Self {
        let mut probabilities = HashMap::new();
        probabilities.insert(destination.to_string(), prob);
        Self::configured_decision(JevResponse {
            destination_id: Some(destination.to_string()),
            probabilities,
            confidence: 0.0,
            margin: 0.0,
        })
    }
}

impl JevIntegration for MockJevIntegration {
    fn decide(
        &self,
        _context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        Box::pin(async move {
            match (&self.result, &self.error) {
                (_, Some(error)) => Err(error.clone()),
                (Some(result), None) => Ok(result.clone()),
                (None, None) => Ok(JevResponse::default()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_destination(mock: &MockJevIntegration) -> Option<String> {
        mock.result
            .as_ref()
            .and_then(|response| response.destination_id.clone())
    }

    #[test]
    fn test_mock_returns_configured_decision() {
        // CONTRACT: Mock should return what was configured
        let mock = MockJevIntegration::configured_decision(JevResponse::default());
        assert_eq!(configured_destination(&mock), Some("NONE".to_string()));
    }

    #[test]
    fn test_mock_returns_configured_error() {
        // CONTRACT: Mock should return error when configured
        let mock = MockJevIntegration::configured_error("Transport error");
        match mock.error {
            Some(JevError::TransportError(message)) => assert_eq!(message, "Transport error"),
            other => panic!("expected a configured transport error, got {other:?}"),
        }
    }

    #[test]
    fn test_mock_select_d1() {
        let mock = MockJevIntegration::select_d1(0.82_f64);
        assert_eq!(configured_destination(&mock), Some("D1".to_string()));
        let probabilities = &mock.result.unwrap().probabilities;
        assert_eq!(probabilities.get("D1"), Some(&0.82_f64));
    }

    #[test]
    fn test_mock_select_none() {
        let mock = MockJevIntegration::select_none(0.70_f64);
        assert_eq!(configured_destination(&mock), Some("NONE".to_string()));
        let probabilities = &mock.result.unwrap().probabilities;
        assert_eq!(probabilities.get("NONE"), Some(&0.70_f64));
    }

    #[test]
    fn test_mock_select_unknown_candidate() {
        let mock = MockJevIntegration::select_unknown();
        assert_eq!(configured_destination(&mock), Some("D99".to_string()));
    }

    #[test]
    fn test_mock_configured_decision_preserves_probabilities() {
        // CONTRACT: configured probabilities must flow through verbatim
        let mut probabilities = HashMap::new();
        probabilities.insert("D1".to_string(), 0.82);
        probabilities.insert("NONE".to_string(), 0.07);
        let mock = MockJevIntegration::configured_decision(JevResponse {
            destination_id: Some("D1".to_string()),
            probabilities: probabilities.clone(),
            confidence: 0.0,
            margin: 0.0,
        });
        let stored = mock.result.unwrap();
        assert_eq!(stored.destination_id, Some("D1".to_string()));
        assert_eq!(stored.probabilities, probabilities);
    }

    #[test]
    fn test_mock_configured_error_leaves_no_result() {
        let mock = MockJevIntegration::configured_error("Transport error");
        assert!(mock.result.is_none());
    }

    #[test]
    fn test_mock_set_response_overrides_result() {
        let mock = MockJevIntegration::default().set_response(Some("D2".to_string()));
        assert_eq!(configured_destination(&mock), Some("D2".to_string()));
    }

    #[test]
    fn test_mock_set_error_overrides() {
        let mock = MockJevIntegration::default().set_error(Some("boom".to_string()));
        assert!(matches!(mock.error, Some(JevError::TransportError(_))));
    }

    #[test]
    fn test_mock_accepts_decision_context() {
        let mock = MockJevIntegration::configured_decision(JevResponse::default());
        let dc = crate::models::decision::DecisionContext::from_candidates(vec![]);
        let _ = mock.decide(&dc);
    }

    #[test]
    fn test_mock_stores_invalid_destinations_verbatim() {
        // CONTRACT: Mock stores and returns invalid paths too; rejection is
        // the validation layer's job
        let mock = MockJevIntegration::select_invalid_path();
        assert_eq!(
            configured_destination(&mock),
            Some("/Users/foo/Documents".to_string())
        );
    }
}

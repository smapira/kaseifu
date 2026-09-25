//! Phase 4 Decision Context - Minimal mock implementation for TDD

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionContext {
    #[serde(default)]
    target: DecisionTarget,
    #[serde(default, skip_serializing)]
    semantic_signals: Vec<String>,
    #[serde(default, skip_serializing)]
    candidates: Vec<DecisionCandidate>,
}

impl DecisionContext {
    /// Build a decision context from discovered candidate IDs.
    ///
    /// The given IDs are kept verbatim (deduplicated, order preserved);
    /// discovery owns ID generation. `NONE` is always a valid choice and
    /// does not need to be listed.
    pub fn from_candidates(candidates: Vec<String>) -> Self {
        let mut decision_candidates: Vec<DecisionCandidate> = Vec::new();

        for id in candidates {
            if id.is_empty() || id == "NONE" {
                continue;
            }
            if decision_candidates
                .iter()
                .any(|candidate| candidate.id == id)
            {
                continue;
            }
            decision_candidates.push(DecisionCandidate {
                id,
                evidence_count: 0,
                reasons: Vec::new(),
            });
        }

        Self {
            target: DecisionTarget::default(),
            semantic_signals: vec![],
            candidates: decision_candidates,
        }
    }

    /// Validate that a selected ID refers to an existing candidate.
    ///
    /// Jev may select one of the offered candidates (`D1`, `D2`, ...) or
    /// `NONE`. Jev may never select something that was not offered, such as
    /// an unknown ID or a filesystem path.
    pub fn validate_decision(&self, candidate_id: &str) -> Result<String, String> {
        if candidate_id == "NONE" {
            return Ok(candidate_id.to_string());
        }

        if self
            .candidates
            .iter()
            .any(|candidate| candidate.id == candidate_id)
        {
            return Ok(candidate_id.to_string());
        }

        Err(format!(
            "Invalid: {} is not an existing candidate",
            candidate_id
        ))
    }

    /// The target file description offered to Jev.
    pub fn target(&self) -> &DecisionTarget {
        &self.target
    }

    /// Semantic signals extracted for this target.
    pub fn semantic_signals(&self) -> &[String] {
        &self.semantic_signals
    }

    /// The offered candidates; Jev may select one of these or `NONE`.
    pub fn candidates(&self) -> &[DecisionCandidate] {
        &self.candidates
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionTarget {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub file_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionCandidate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub evidence_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction {
    #[default]
    None,
    MoveTo(String),
    AutoMove(String),
    Review(String),
    Leave {
        what: String,
    },
    Recall(String),
    MoveToFolder {
        folder: String,
    },
    Delete {
        what: String,
    },
    Archive {
        what: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JevDecision {
    #[serde(default)]
    pub destination_id: Option<String>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub margin: f64,
    #[serde(default)]
    pub action: DecisionAction,
}

/// Local confidence/margin calculation.
///
/// Considers only valid probabilities (finite and within `0.0..=1.0`);
/// anything else is ignored.
///
/// - `confidence` = highest valid probability (P1), 0.0 when none exist
/// - `margin` = P1 - P2, or 0.0 when fewer than two valid probabilities
///   exist (a lone or tied top is not a decisive margin)
pub fn confidence_and_margin(probabilities: &[(String, f64)]) -> (f64, f64) {
    let mut values: Vec<f64> = probabilities
        .iter()
        .map(|(_, probability)| *probability)
        .filter(|probability| probability.is_finite() && (0.0..=1.0).contains(probability))
        .collect();
    values.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let top = values.first().copied().unwrap_or(0.0);
    let margin = values
        .get(1)
        .map(|second| (top - second).max(0.0))
        .unwrap_or(0.0);

    (top, margin)
}

/// Validate a probability distribution from an untrusted Jev response.
///
/// Every probability must be finite and within `0.0..=1.0`.
pub fn validate_probabilities(probabilities: &[(String, f64)]) -> Result<(), String> {
    for (id, probability) in probabilities {
        if !probability.is_finite() || *probability < 0.0 || *probability > 1.0 {
            return Err(format!("invalid probability {probability} for '{id}'"));
        }
    }

    Ok(())
}

impl JevDecision {
    /// Build a decision from a **validated** Jev selection plus the
    /// probability distribution that accompanied it.
    ///
    /// The caller must have validated `choice` against the offered
    /// candidates first (see [`DecisionContext::validate_decision`]).
    pub fn from_selection(choice: &str, probabilities: &[(String, f64)]) -> Self {
        let (confidence, margin) = confidence_and_margin(probabilities);

        let action = if choice == "NONE" {
            DecisionAction::None
        } else {
            DecisionAction::MoveTo(choice.to_string())
        };

        Self {
            destination_id: Some(choice.to_string()),
            confidence,
            margin,
            action,
        }
    }

    pub fn new_none() -> Self {
        Self {
            destination_id: Some("NONE".into()),
            confidence: 0.0,
            margin: 0.0,
            action: DecisionAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_none_is_accepted() {
        let dc = DecisionContext::from_candidates(vec![]);
        let result = dc.validate_decision("NONE");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_known_is_accepted() {
        let dc = DecisionContext::from_candidates(vec!["D1".into()]);
        let result = dc.validate_decision("D1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_unknown_is_rejected() {
        let dc = DecisionContext::from_candidates(vec!["D1".into(), "D2".into()]);
        let result = dc.validate_decision("D99");
        assert!(result.is_err(), "Unknown candidates should be rejected");
    }
}

//! Phase 4 Decision Context - the semantic question offered to Jev.

use crate::models::candidate::CandidateDestination;
use crate::models::file_context::{ImageMetadata, NormalizedFileContext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionContext {
    #[serde(default)]
    target: DecisionTarget,
    #[serde(default, skip_serializing)]
    semantic_signals: Vec<String>,
    #[serde(default, skip_serializing)]
    candidates: Vec<DecisionCandidate>,
    /// User-provided keywords from `--keywords` (v112): decision material
    /// only. Skipped in this struct's own serialization like every other
    /// content field; the adapter maps them onto `state.semantic_signals`
    /// with the `user: ` prefix. They are never merged into
    /// `semantic_signals`, so the domain can still tell user input from
    /// derived signals and Discovery never sees them.
    #[serde(default, skip_serializing)]
    user_keywords: Vec<String>,
    /// Normalized image metadata for Jev (v3.2; `None` for non-images).
    ///
    /// Skipped in this struct's own serialization (the adapter maps it onto
    /// the real `/v1/systemone` `state` fields with privacy gating).
    #[serde(default, skip_serializing)]
    image_metadata: Option<ImageMetadata>,
}

impl DecisionContext {
    /// Build the decision context from the production pipeline output.
    ///
    /// Pure information mapping; no scoring, no selection, no policy:
    ///
    /// - normalized target       -> decision target (filename/extension/type)
    /// - normalized signals      -> `semantic_signals` (verbatim; never re-derived)
    /// - discovered candidates   -> decision candidates (id/evidence/reasons)
    pub fn from_normalized(
        normalized: &NormalizedFileContext,
        candidates: &[CandidateDestination],
    ) -> Self {
        let target = DecisionTarget {
            filename: normalized.target.filename.clone(),
            extension: normalized.target.extension.clone(),
            file_type: normalized.file_type.clone(),
        };

        let candidates = candidates
            .iter()
            .map(|candidate| DecisionCandidate {
                id: candidate.id.clone(),
                path: candidate.path.display().to_string(),
                evidence_count: candidate.evidence_count,
                reasons: candidate.reasons.clone(),
            })
            .collect();

        let mut context = Self::from_parts(target, normalized.semantic_signals.clone(), candidates);
        context.image_metadata = normalized.image_metadata.clone();
        context
    }

    /// Build a decision context from discovered candidate IDs alone.
    ///
    /// Used where only membership matters (e.g. ID-level tests); production
    /// wiring uses [`DecisionContext::from_normalized`].
    pub fn from_candidates(candidates: Vec<String>) -> Self {
        let candidates = candidates
            .into_iter()
            .map(|id| DecisionCandidate {
                id,
                path: String::new(),
                evidence_count: 0,
                reasons: Vec::new(),
            })
            .collect();

        Self::from_parts(DecisionTarget::default(), Vec::new(), candidates)
    }

    /// Single construction path: ids are kept verbatim (deduplicated, order
    /// preserved). `NONE` is always a valid choice and is never a candidate.
    fn from_parts(
        target: DecisionTarget,
        semantic_signals: Vec<String>,
        candidates: Vec<DecisionCandidate>,
    ) -> Self {
        let mut decision_candidates: Vec<DecisionCandidate> = Vec::new();

        for candidate in candidates {
            if candidate.id.is_empty() || candidate.id == "NONE" {
                continue;
            }
            if decision_candidates
                .iter()
                .any(|existing| existing.id == candidate.id)
            {
                continue;
            }
            decision_candidates.push(candidate);
        }

        Self {
            target,
            semantic_signals,
            candidates: decision_candidates,
            user_keywords: Vec::new(),
            image_metadata: None,
        }
    }

    /// Attach image metadata (v3.2; builder for tests and redaction checks).
    pub fn with_image_metadata(mut self, meta: Option<ImageMetadata>) -> Self {
        self.image_metadata = meta;
        self
    }

    /// Attach the user's `--keywords` (v112; replaces any previous list).
    ///
    /// Content signals stay untouched: only the adapter turns these into
    /// `user: <KEY>` entries on the wire, and only when it builds the request.
    pub fn with_user_keywords(mut self, keywords: &[String]) -> Self {
        self.user_keywords = keywords.to_vec();
        self
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

    /// The user's `--keywords` for this decision (verbatim, no prefix).
    pub fn user_keywords(&self) -> &[String] {
        &self.user_keywords
    }

    /// The offered candidates; Jev may select one of these or `NONE`.
    pub fn candidates(&self) -> &[DecisionCandidate] {
        &self.candidates
    }

    /// Normalized image metadata, if the target is an image with any.
    pub fn image_metadata(&self) -> Option<&ImageMetadata> {
        self.image_metadata.as_ref()
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
    /// The candidate destination directory the pipeline discovered.
    #[serde(default)]
    pub path: String,
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

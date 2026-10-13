//! Phase 5 - local Policy Engine.
//!
//! The engine converts a validated `JevDecision` into the *execution*
//! action. It never touches the filesystem and never invents destinations:
//! the payload of `AutoMove`/`Review` is a candidate ID, never a path.
//!
//! Thresholds come from `docs/design/05-execute.md` (matching
//! `config/default.toml`); they are conservative initial values and are
//! expected to be tuned from real data later.

use crate::models::decision::{DecisionAction, JevDecision};

/// AUTO requires both a high confidence and a clear margin.
pub const AUTO_MOVE_CONFIDENCE: f64 = 0.85;
pub const AUTO_MOVE_MARGIN: f64 = 0.40;

/// REVIEW requires at least this confidence; below it the decision is LEFT.
pub const REVIEW_CONFIDENCE: f64 = 0.60;

/// Tunable policy thresholds (defaults match the engine constants).
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// Minimum confidence for automatic moves.
    pub auto_move_confidence: f64,
    /// Minimum margin over the second-best candidate.
    pub auto_move_margin: f64,
    /// Below this confidence the file is left in place.
    pub review_confidence: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            auto_move_confidence: AUTO_MOVE_CONFIDENCE,
            auto_move_margin: AUTO_MOVE_MARGIN,
            review_confidence: REVIEW_CONFIDENCE,
        }
    }
}

/// Evaluate the execution policy for a validated decision.
///
/// - `AutoMove(candidate_id)` - execution may build a plan for this candidate
/// - `Review(candidate_id)`   - uncertain; no execution
/// - `Leave { what }`         - no action (including `NONE` decisions)
pub fn evaluate(decision: &JevDecision) -> DecisionAction {
    evaluate_with(decision, Thresholds::default())
}

/// Evaluate the policy with explicit thresholds (user TOML overrides).
pub fn evaluate_with(decision: &JevDecision, thresholds: Thresholds) -> DecisionAction {
    let Some(destination) = leave_or_destination(decision) else {
        return DecisionAction::Leave {
            what: "no candidate selected".to_string(),
        };
    };

    if is_auto_move(decision, thresholds) {
        DecisionAction::AutoMove(destination)
    } else if decision.confidence >= thresholds.review_confidence {
        DecisionAction::Review(destination)
    } else {
        DecisionAction::Leave {
            what: "confidence below review threshold".to_string(),
        }
    }
}

fn leave_or_destination(decision: &JevDecision) -> Option<String> {
    match decision.destination_id.as_deref() {
        Some(id) if id != "NONE" => Some(id.to_string()),
        _ => None,
    }
}

fn is_auto_move(decision: &JevDecision, thresholds: Thresholds) -> bool {
    decision.confidence >= thresholds.auto_move_confidence
        && decision.margin >= thresholds.auto_move_margin
}

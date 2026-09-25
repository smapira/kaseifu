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

/// Evaluate the execution policy for a validated decision.
///
/// - `AutoMove(candidate_id)` - execution may build a plan for this candidate
/// - `Review(candidate_id)`   - uncertain; no execution
/// - `Leave { what }`         - no action (including `NONE` decisions)
pub fn evaluate(decision: &JevDecision) -> DecisionAction {
    let destination = match decision.destination_id.as_deref() {
        Some(id) if id != "NONE" => id,
        _ => {
            return DecisionAction::Leave {
                what: "no candidate selected".to_string(),
            };
        }
    };

    if decision.confidence >= AUTO_MOVE_CONFIDENCE && decision.margin >= AUTO_MOVE_MARGIN {
        DecisionAction::AutoMove(destination.to_string())
    } else if decision.confidence >= REVIEW_CONFIDENCE {
        DecisionAction::Review(destination.to_string())
    } else {
        DecisionAction::Leave {
            what: "confidence below review threshold".to_string(),
        }
    }
}

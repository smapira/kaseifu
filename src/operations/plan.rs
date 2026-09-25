//! Phase 5 - `MovePlan`: the only sanctioned translation from a decision to
//! a filesystem destination.
//!
//! Invariant: the destination directory always comes from a discovered
//! [`CandidateDestination`]. A `JevDecision` only ever selects an *ID*; it
//! can never supply a path.

use crate::models::candidate::CandidateDestination;
use crate::models::decision::{DecisionAction, JevDecision};
use std::path::{Path, PathBuf};

/// A validated, not-yet-executed move.
#[derive(Debug, Clone, PartialEq)]
pub struct MovePlan {
    pub source: PathBuf,
    pub destination_directory: PathBuf,
    pub destination_path: PathBuf,
    pub candidate_id: String,
    pub confidence: f64,
    pub margin: f64,
}

/// Why a plan could not be built. In every case the filesystem is untouched.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// The policy outcome does not permit execution (Review, Leave, NONE).
    NotAutoMove,
    /// The decision selected something that was not an offered candidate.
    UnknownCandidate(String),
    SourceMissing(PathBuf),
    SourceNotAFile(PathBuf),
    DestinationNotADirectory(PathBuf),
    /// The destination already exists; Phase 5 never overwrites.
    Collision(PathBuf),
}

/// Build a move plan for `source` from a validated decision.
///
/// The plan is only produced when the policy allows an auto-move *and* every
/// path-safety precondition holds.
pub fn build_move_plan(
    source: &Path,
    decision: &JevDecision,
    candidates: &[CandidateDestination],
) -> Result<MovePlan, PlanError> {
    // 1. Policy gate: only an auto-move may be planned.
    let candidate_id = match crate::policy::decision::evaluate(decision) {
        DecisionAction::AutoMove(id) => id,
        _ => return Err(PlanError::NotAutoMove),
    };

    // 2. Destination lineage: the ID must match a discovered candidate.
    //    A selection that is not an offered candidate (unknown ID, path,
    //    free text) is rejected here.
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.id == candidate_id)
        .ok_or_else(|| PlanError::UnknownCandidate(candidate_id.clone()))?;

    // 3. Path safety for the source.
    if !source.exists() {
        return Err(PlanError::SourceMissing(source.to_path_buf()));
    }
    if !source.is_file() {
        return Err(PlanError::SourceNotAFile(source.to_path_buf()));
    }

    // 4. Path safety for the destination directory (from discovery only).
    let destination_directory = candidate.path.clone();
    if !destination_directory.is_dir() {
        return Err(PlanError::DestinationNotADirectory(destination_directory));
    }

    let file_name = source
        .file_name()
        .ok_or_else(|| PlanError::SourceMissing(source.to_path_buf()))?;
    let destination_path = destination_directory.join(file_name);

    // 5. Collision: Phase 5 never overwrites.
    if destination_path.exists() {
        return Err(PlanError::Collision(destination_path));
    }

    Ok(MovePlan {
        source: source.to_path_buf(),
        destination_directory,
        destination_path,
        candidate_id,
        confidence: decision.confidence,
        margin: decision.margin,
    })
}

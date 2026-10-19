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
    /// The destination directory is on the never-a-destination list
    /// (`~/Library/**`, caches, system trees, machine-generated names).
    DestinationExcluded(PathBuf),
    /// A repository policy rule (`.kaseifu.toml`) pointed outside its own
    /// workspace (the directory that holds the policy file), either directly
    /// or through a symbolic link.
    DestinationOutsideWorkspace(PathBuf),
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
    build_move_plan_with(
        source,
        decision,
        candidates,
        crate::policy::decision::Thresholds::default(),
    )
}

/// Build a move plan with explicit policy thresholds (user TOML overrides).
///
/// The gate agrees with `organize_file_with`: both use the same thresholds,
/// so a plan is never refused that the policy allowed (and vice versa).
pub fn build_move_plan_with(
    source: &Path,
    decision: &JevDecision,
    candidates: &[CandidateDestination],
    thresholds: crate::policy::decision::Thresholds,
) -> Result<MovePlan, PlanError> {
    // 1. Policy gate: only an auto-move may be planned.
    let candidate_id = match crate::policy::decision::evaluate_with(decision, thresholds) {
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

    let destination_path = checked_destination_path(source, &candidate.path)?;

    Ok(MovePlan {
        source: source.to_path_buf(),
        destination_directory: candidate.path.clone(),
        destination_path,
        candidate_id,
        confidence: decision.confidence,
        margin: decision.margin,
    })
}

/// Build a plan for a destination the user picked by hand (ZC-13).
///
/// The policy gate is intentionally absent: a human decision replaces the
/// automatic verdict. Everything else is identical to
/// [`build_move_plan_with`]: the destination must be a usable directory
/// (the never-a-destination rules are re-checked here, so even a candidate
/// from an older list is refused) and a collision is never overwritten.
///
/// The resulting plan carries no Jev numbers (confidence/margin are 0.0);
/// [`crate::operations::mover::execute_manual_move`] records the move with
/// NULL journal columns so a human move stays distinguishable from a Jev
/// move.
pub fn build_manual_move_plan(
    source: &Path,
    candidate: &CandidateDestination,
) -> Result<MovePlan, PlanError> {
    if crate::discovery::is_excluded_destination(&candidate.path.to_string_lossy()) {
        return Err(PlanError::DestinationExcluded(candidate.path.clone()));
    }
    let destination_path = checked_destination_path(source, &candidate.path)?;
    Ok(MovePlan {
        source: source.to_path_buf(),
        destination_directory: candidate.path.clone(),
        destination_path,
        candidate_id: candidate.id.clone(),
        confidence: 0.0,
        margin: 0.0,
    })
}

/// Build a plan for a destination a repository policy rule chose.
///
/// `.kaseifu.toml` supplies both the pattern and the destination, so no
/// discovered candidate exists. The rule is the decision, exactly as `--to` is
/// for a human move, and the checks are the same as
/// [`build_manual_move_plan`] plus one more: the destination must stay inside
/// the workspace root that holds the policy file. Rule destinations and the
/// directories they resolve through are therefore never able to place a file
/// outside its repository, even when a symbolic link points elsewhere.
///
/// The plan carries `rule_id` as its selection (the journal records it in the
/// `candidate_id` column, the way `rename` records its naming rule) and no
/// classifier numbers.
pub fn build_rule_move_plan(
    source: &Path,
    destination_directory: &Path,
    workspace_root: &Path,
    rule_id: &str,
) -> Result<MovePlan, PlanError> {
    if !crate::benchmark::is_inside(workspace_root, destination_directory) {
        return Err(PlanError::DestinationOutsideWorkspace(
            destination_directory.to_path_buf(),
        ));
    }
    if crate::discovery::is_excluded_destination(&destination_directory.to_string_lossy()) {
        return Err(PlanError::DestinationExcluded(
            destination_directory.to_path_buf(),
        ));
    }
    let destination_path = checked_destination_path(source, destination_directory)?;
    Ok(MovePlan {
        source: source.to_path_buf(),
        destination_directory: destination_directory.to_path_buf(),
        destination_path,
        candidate_id: rule_id.to_string(),
        confidence: 0.0,
        margin: 0.0,
    })
}

/// Source, destination-directory and collision checks shared by the plan
/// builders (same order as the original `build_move_plan_with`).
fn checked_destination_path(
    source: &Path,
    destination_directory: &Path,
) -> Result<PathBuf, PlanError> {
    if !source.exists() {
        return Err(PlanError::SourceMissing(source.to_path_buf()));
    }
    if !source.is_file() {
        return Err(PlanError::SourceNotAFile(source.to_path_buf()));
    }
    if !destination_directory.is_dir() {
        return Err(PlanError::DestinationNotADirectory(
            destination_directory.to_path_buf(),
        ));
    }

    let file_name = source
        .file_name()
        .ok_or_else(|| PlanError::SourceMissing(source.to_path_buf()))?;
    let destination_path = destination_directory.join(file_name);

    if destination_path.exists() {
        return Err(PlanError::Collision(destination_path));
    }
    Ok(destination_path)
}

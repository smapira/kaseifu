//! Phase 5 - the single executor for filesystem moves.
//!
//! Responsibilities (and nothing else):
//!
//! ```text
//! verify preconditions
//!     -> record the operation in the journal (before the mutation)
//!     -> rename
//!     -> record the result
//! ```
//!
//! The executor never normalizes, discovers, or calls Jev, and it never
//! invents a destination: the plan was built from a discovered candidate.

use crate::operations::plan::MovePlan;
use crate::storage::history::OperationHistory;
use std::path::PathBuf;

/// Why an execution failed. In every case the source is preserved.
#[derive(Debug, Clone, PartialEq)]
pub enum MoveError {
    SourceMissing(PathBuf),
    SourceNotAFile(PathBuf),
    DestinationNotADirectory(PathBuf),
    Collision(PathBuf),
    /// The filesystem refused the move (permissions, cross-device, ...).
    Io(String),
    /// The journal could not be written.
    History(String),
}

/// Result of a successful (or simulated) execution.
#[derive(Debug, Clone, PartialEq)]
pub enum MoveOutcome {
    Moved { operation_id: String },
    WouldMove,
}

/// Validate that the plan can still be executed.
///
/// Touches nothing; used both by the dry run and immediately before a real
/// move (the filesystem may have changed since the plan was built).
fn verify_preconditions(plan: &MovePlan) -> Result<(), MoveError> {
    if !plan.source.exists() {
        return Err(MoveError::SourceMissing(plan.source.clone()));
    }
    if !plan.source.is_file() {
        return Err(MoveError::SourceNotAFile(plan.source.clone()));
    }
    if !plan.destination_directory.is_dir() {
        return Err(MoveError::DestinationNotADirectory(
            plan.destination_directory.clone(),
        ));
    }
    if plan.destination_path.exists() {
        return Err(MoveError::Collision(plan.destination_path.clone()));
    }
    Ok(())
}

/// Dry run: report what would happen without any mutation or journal write.
pub fn dry_run(plan: &MovePlan) -> Result<MoveOutcome, MoveError> {
    verify_preconditions(plan)?;
    Ok(MoveOutcome::WouldMove)
}

/// Execute the move, journaling it before the mutation.
///
/// Ordering follows docs/design/05-execute.md ("record, then move"):
///
/// 1. verify preconditions (no side effects)
/// 2. record the operation as pending
/// 3. rename
/// 4. mark success, or mark failure with the error detail
///
/// If the rename succeeds but the journal cannot be finalized, the operation
/// stays `pending` in the journal: the mutation is always *recorded*, even
/// when its outcome could not be written.
pub fn execute_move(plan: &MovePlan, history: &OperationHistory) -> Result<MoveOutcome, MoveError> {
    execute_move_inner(plan, history, MoveJournal::Jev)
}

/// Execute a move the user picked by hand (ZC-13).
///
/// Identical to [`execute_move`] except for the journal row: a manual move
/// has no Jev selection, confidence or margin, so those columns stay NULL.
/// That is how `doctor`, `undo` and any journal reader can tell a human
/// decision apart from a Jev decision.
pub fn execute_manual_move(
    plan: &MovePlan,
    history: &OperationHistory,
) -> Result<MoveOutcome, MoveError> {
    execute_move_inner(plan, history, MoveJournal::Manual)
}

/// Which selection the journal row should carry.
enum MoveJournal {
    /// Jev selected the candidate: record its id and numbers.
    Jev,
    /// The user selected the candidate: no Jev numbers exist.
    Manual,
}

fn execute_move_inner(
    plan: &MovePlan,
    history: &OperationHistory,
    journal: MoveJournal,
) -> Result<MoveOutcome, MoveError> {
    verify_preconditions(plan)?;

    let (candidate_id, confidence, margin) = match journal {
        MoveJournal::Jev => (
            Some(plan.candidate_id.as_str()),
            Some(plan.confidence),
            Some(plan.margin),
        ),
        MoveJournal::Manual => (None, None, None),
    };

    let operation_id = history
        .record_pending(
            "move",
            &plan.source,
            &plan.destination_path,
            candidate_id,
            confidence,
            margin,
        )
        .map_err(|error| MoveError::History(error.to_string()))?;

    match std::fs::rename(&plan.source, &plan.destination_path) {
        Ok(()) => {
            if let Err(error) = history.mark_success(&operation_id) {
                return Err(MoveError::History(error.to_string()));
            }
            Ok(MoveOutcome::Moved { operation_id })
        }
        Err(error) => {
            // The filesystem was not mutated (rename is atomic); record the
            // failure and never a success.
            let _ = history.mark_failure(&operation_id, &error.to_string());
            Err(MoveError::Io(error.to_string()))
        }
    }
}

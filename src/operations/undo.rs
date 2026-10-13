//! Phase 5 - Undo.
//!
//! Reverses the most recent successful, not-yet-undone move using the
//! journal only. The reverse path comes from the recorded paths; collision
//! rules are identical to a move (never overwrite).

use crate::storage::history::OperationHistory;
use std::path::{Path, PathBuf};

/// Why an undo failed. In every case the filesystem is preserved.
#[derive(Debug, Clone, PartialEq)]
pub enum UndoError {
    NothingToUndo,
    SourceMissing(PathBuf),
    SourceNotAFile(PathBuf),
    DestinationNotADirectory(PathBuf),
    Collision(PathBuf),
    Io(String),
    History(String),
}

/// Result of a successful undo.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoOutcome {
    /// The journal id of the undo operation itself.
    pub operation_id: String,
    /// The journal id of the move that was reversed.
    pub undone_move_id: String,
    /// Where the file was restored to.
    pub restored_to: PathBuf,
}

/// Undo the most recent successful move.
///
/// Ordering mirrors a move: verify preconditions, record the undo as
/// pending, rename, then mark the result and the original move. Any refusal
/// leaves the filesystem and the journal untouched.
pub fn undo_last_move(history: &OperationHistory) -> Result<UndoOutcome, UndoError> {
    let entry = history
        .last_successful_move()
        .map_err(|error| UndoError::History(error.to_string()))?
        .ok_or(UndoError::NothingToUndo)?;

    let paths = validate_undo_preconditions(&entry)?;
    commit_undo(history, &entry, &paths)
}

/// Journaled locations for the reverse move (validated before any mutation).
struct UndoPaths {
    moved_file: PathBuf,
    original_path: PathBuf,
}

/// Verify preconditions without touching the filesystem or the journal:
/// the moved file must exist exactly as journaled, and the original
/// location must be free (undo never overwrites).
fn validate_undo_preconditions(
    entry: &crate::storage::history::HistoryEntry,
) -> Result<UndoPaths, UndoError> {
    let moved_file = PathBuf::from(&entry.destination_path);
    let original_path = PathBuf::from(&entry.source_path);

    if !moved_file.exists() {
        return Err(UndoError::SourceMissing(moved_file));
    }
    if !moved_file.is_file() {
        return Err(UndoError::SourceNotAFile(moved_file));
    }
    let original_directory = original_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| UndoError::DestinationNotADirectory(original_path.clone()))?;
    if !original_directory.is_dir() {
        return Err(UndoError::DestinationNotADirectory(original_directory));
    }
    if original_path.exists() {
        return Err(UndoError::Collision(original_path));
    }
    Ok(UndoPaths {
        moved_file,
        original_path,
    })
}

/// Record the undo as pending, rename, then mark the result and the original.
/// Any refusal leaves the filesystem and the journal untouched.
fn commit_undo(
    history: &OperationHistory,
    entry: &crate::storage::history::HistoryEntry,
    paths: &UndoPaths,
) -> Result<UndoOutcome, UndoError> {
    let undo_id = history
        .record_pending(
            "undo",
            &paths.moved_file,
            &paths.original_path,
            entry.candidate_id.as_deref(),
            entry.confidence,
            entry.margin,
        )
        .map_err(|error| UndoError::History(error.to_string()))?;

    match std::fs::rename(&paths.moved_file, &paths.original_path) {
        Ok(()) => finalize_undo_success(history, &undo_id, entry, &paths.original_path),
        Err(error) => {
            let _ = history.mark_failure(&undo_id, &error.to_string());
            Err(UndoError::Io(error.to_string()))
        }
    }
}

/// Mark both the undo and the reversed move; failures preserve the filesystem.
fn finalize_undo_success(
    history: &OperationHistory,
    undo_id: &str,
    entry: &crate::storage::history::HistoryEntry,
    original_path: &Path,
) -> Result<UndoOutcome, UndoError> {
    if let Err(error) = history.mark_success(undo_id) {
        return Err(UndoError::History(error.to_string()));
    }
    if let Err(error) = history.mark_undone(&entry.id) {
        return Err(UndoError::History(error.to_string()));
    }
    Ok(UndoOutcome {
        operation_id: undo_id.to_string(),
        undone_move_id: entry.id.clone(),
        restored_to: original_path.to_path_buf(),
    })
}

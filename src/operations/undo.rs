//! Phase 5 - Undo.
//!
//! Reverses the most recent successful, not-yet-undone move using the
//! journal only. The reverse path comes from the recorded paths; collision
//! rules are identical to a move (never overwrite).
//!
//! Phase 7 moved folders through the same journal: a folder rename is a `move`
//! row like any other, so undo has to accept a folder at the recorded path.
//! The selection order (newest successful move first) is what keeps a child's
//! history resolvable after an ancestor folder was renamed: the ancestor row is
//! always reversed before any row recorded under its old name, so by the time
//! that row is reached its recorded paths exist again. Nothing rewrites journal
//! rows. When a recorded path cannot be found and no row explains where it
//! went (an ancestor renamed outside kaseifu, or an interrupted operation),
//! the undo refuses with that path and mutates nothing.

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

impl std::fmt::Display for UndoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToUndo => write!(f, "nothing left to undo"),
            Self::SourceMissing(path) => write!(
                f,
                "the recorded path {} no longer exists (a parent folder may have been renamed \
                 outside kaseifu); nothing was changed",
                path.display()
            ),
            Self::SourceNotAFile(path) => write!(
                f,
                "the recorded path {} is neither a file nor a folder; nothing was changed",
                path.display()
            ),
            Self::DestinationNotADirectory(path) => write!(
                f,
                "{} is not a usable directory to restore into; nothing was changed",
                path.display()
            ),
            Self::Collision(path) => write!(
                f,
                "refusing to overwrite {}; nothing was changed",
                path.display()
            ),
            Self::Io(detail) => write!(f, "{detail}"),
            Self::History(detail) => write!(f, "journal: {detail}"),
        }
    }
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
    undo_entry(history, &entry)
}

/// Undo one journaled move (batch undo reverses each batch row through here,
/// newest first; the per-row preconditions are identical to the last-move
/// path, so a batch stops at the same refusals instead of skipping them).
pub fn undo_entry(
    history: &OperationHistory,
    entry: &crate::storage::history::HistoryEntry,
) -> Result<UndoOutcome, UndoError> {
    let paths = validate_undo_preconditions(entry)?;
    commit_undo(history, entry, &paths)
}

/// Journaled locations for the reverse move (validated before any mutation).
struct UndoPaths {
    moved_file: PathBuf,
    original_path: PathBuf,
}

/// Verify preconditions without touching the filesystem or the journal:
/// the moved target must exist exactly as journaled (a file or a folder), and
/// the original location must be free (undo never overwrites).
fn validate_undo_preconditions(
    entry: &crate::storage::history::HistoryEntry,
) -> Result<UndoPaths, UndoError> {
    let moved_file = PathBuf::from(&entry.destination_path);
    let original_path = PathBuf::from(&entry.source_path);

    if !moved_file.exists() {
        return Err(UndoError::SourceMissing(moved_file));
    }
    // A rename moves files and folders alike (Phase 7). Anything else (a
    // socket, a device node, a dangling symlink) is refused; `exists` already
    // follows symlinks, so a symlink to a real target gets here and is undone
    // as the move it was.
    if !moved_file.is_file() && !moved_file.is_dir() {
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

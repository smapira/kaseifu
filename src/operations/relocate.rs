//! A move with no judgement in it: one path into one directory, file or
//! folder (issue #22).
//!
//! `organize` refuses a folder by design (it decides *where* a file belongs,
//! and #14 settled that a folder move needs its own semantics), so the user had
//! no way to file a directory at all. This module is the low-level path: no
//! discovery, no classifier, no key, no network. It applies the same
//! never-a-destination rules as everything else, journals the move (with the
//! entry kind) before mutating, and reuses the journal for `undo`.
//!
//! A same-volume move is one `rename`. Crossing volumes copies the tree,
//! verifies the copy (file count and byte total) and only then removes the
//! source; a failed copy removes the partial copy and leaves the source alone,
//! and a source that cannot be removed is reported with both sides intact
//! (a verified copy is never deleted because the other side failed).

use crate::context::directory::{
    is_bundle_container, is_cache_or_temp, is_library_tree, is_system_area, is_system_tree_name,
};
use crate::discovery::is_excluded_destination;
use crate::storage::history::{OperationHistory, PendingRecord};
use std::io;
use std::path::{Path, PathBuf};

/// What is being moved. Recorded in the journal so a reader never has to infer
/// it from the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
}

impl EntryKind {
    /// The journal word (`file` / `dir`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
        }
    }
}

/// Why a move was refused. In every case the filesystem is untouched.
#[derive(Debug, Clone, PartialEq)]
pub enum RelocateError {
    SourceMissing(PathBuf),
    /// The source is neither a regular file nor a folder (a socket, a device,
    /// a symbolic link).
    SourceNotMovable(PathBuf),
    /// The source is in an area this command does not move things out of
    /// (a Library tree, a cache, a system area, an app bundle).
    SourceExcluded(PathBuf),
    DestinationNotADirectory(PathBuf),
    DestinationExcluded(PathBuf),
    /// The source already sits in the destination directory.
    SameDirectory(PathBuf),
    /// The destination is inside the source (a folder cannot move into itself).
    Nested(PathBuf, PathBuf),
    Collision(PathBuf),
    Io(String),
    History(String),
}

impl std::fmt::Display for RelocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceMissing(path) => write!(f, "source does not exist: {}", path.display()),
            Self::SourceNotMovable(path) => write!(
                f,
                "only a regular file or a folder can be moved: {}",
                path.display()
            ),
            Self::SourceExcluded(path) => write!(
                f,
                "source is in an area kaseifu does not move things out of: {}",
                path.display()
            ),
            Self::DestinationNotADirectory(path) => {
                write!(f, "destination is not a directory: {}", path.display())
            }
            Self::DestinationExcluded(path) => {
                write!(f, "destination is not a usable folder: {}", path.display())
            }
            Self::SameDirectory(path) => {
                write!(f, "already in {}; nothing to move", path.display())
            }
            Self::Nested(source, destination) => write!(
                f,
                "cannot move {} into its own subtree ({})",
                source.display(),
                destination.display()
            ),
            Self::Collision(path) => {
                write!(
                    f,
                    "refusing to overwrite {}; nothing was changed",
                    path.display()
                )
            }
            Self::Io(detail) => write!(f, "{detail}"),
            Self::History(detail) => write!(f, "journal: {detail}"),
        }
    }
}

/// A validated, not-yet-executed move.
#[derive(Debug, Clone, PartialEq)]
pub struct RelocatePlan {
    pub source: PathBuf,
    pub destination_directory: PathBuf,
    pub destination_path: PathBuf,
    pub kind: EntryKind,
}

/// Result of a move (or of the dry run).
#[derive(Debug, Clone, PartialEq)]
pub enum RelocateOutcome {
    Moved { operation_id: String },
    WouldMove,
}

/// A move of one or more paths into one destination directory (issue #25).
#[derive(Debug, Clone, PartialEq)]
pub struct MoveBatch {
    /// Where every path goes.
    pub destination_directory: PathBuf,
    /// The levels that must be created first, outermost first. Empty when the
    /// directory already exists, or when creation was not requested.
    pub missing_directories: Vec<PathBuf>,
    /// One entry per requested path, in the order the user gave them.
    pub items: Vec<MoveItem>,
}

impl MoveBatch {
    /// The plans that are ready to run (a failed item is reported as it is).
    pub fn planned(&self) -> impl Iterator<Item = &RelocatePlan> {
        self.items.iter().filter_map(|item| item.plan.as_ref().ok())
    }
}

/// One requested path and what planning made of it.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveItem {
    /// The path the user named (kept so a refusal still names it).
    pub source: PathBuf,
    /// The validated move, or the reason there is none.
    pub plan: Result<RelocatePlan, RelocateError>,
}

/// What one path did during an executed batch.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemOutcome {
    /// The journal id of the move, when it happened.
    pub operation_id: Option<String>,
    /// Why it did not happen; `None` when it did.
    pub error: Option<RelocateError>,
}

impl ItemOutcome {
    /// True when this path was moved.
    pub fn moved(&self) -> bool {
        self.operation_id.is_some()
    }
}

/// The result of executing a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchOutcome {
    /// The id that groups this run's journal rows for `undo --batch`.
    pub batch_id: String,
    /// One result per requested path, in the same order as [`MoveBatch::items`].
    pub items: Vec<ItemOutcome>,
    /// The directories this run created and kept.
    pub created_directories: Vec<PathBuf>,
}

/// Validate one move without touching anything.
pub fn plan_relocate(source: &Path, destination: &Path) -> Result<RelocatePlan, RelocateError> {
    // The one-path form keeps its historical contract: the destination has to
    // exist, and a plan never creates anything.
    destination_levels(destination, false)?;
    plan_item(source, destination)
}

/// Plan a move of every path into `destination`, creating it when `create`.
///
/// The destination is validated once (it is shared), then each path is
/// validated on its own: one unreadable source must not stop the others, which
/// is what lets the command report per item.
pub fn plan_batch(
    paths: &[PathBuf],
    destination: &Path,
    create: bool,
) -> Result<MoveBatch, RelocateError> {
    let missing_directories = destination_levels(destination, create)?;
    let mut items = Vec::with_capacity(paths.len());
    let mut destinations: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for path in paths {
        let plan = plan_item(path, destination).and_then(|plan| {
            // Two paths with the same name would both target one file: the plan
            // names that as a collision instead of letting the second move fail
            // halfway through the batch.
            if destinations.contains(&plan.destination_path) {
                return Err(RelocateError::Collision(plan.destination_path));
            }
            destinations.push(plan.destination_path.clone());
            Ok(plan)
        });
        items.push(MoveItem {
            source: path.clone(),
            plan,
        });
    }
    Ok(MoveBatch {
        destination_directory: destination.to_path_buf(),
        missing_directories,
        items,
    })
}

/// The destination levels a move has to create, outermost first (issue #25).
///
/// Empty when the directory already exists. `create = false` keeps the
/// historical refusal, and an existing non-directory is refused either way: a
/// file where the folder should go is never replaced.
fn destination_levels(destination: &Path, create: bool) -> Result<Vec<PathBuf>, RelocateError> {
    // The never-a-destination rules come first: a place a person never picks
    // (`~/Library/**`, caches, system trees) is refused as *that*, not as "not a
    // directory".
    if is_excluded_destination(&destination.to_string_lossy()) {
        return Err(RelocateError::DestinationExcluded(
            destination.to_path_buf(),
        ));
    }
    if destination.is_dir() {
        return Ok(Vec::new());
    }
    if destination.exists() || !create {
        return Err(RelocateError::DestinationNotADirectory(
            destination.to_path_buf(),
        ));
    }
    let mut levels = Vec::new();
    let mut current = Some(destination);
    while let Some(path) = current {
        if path.as_os_str().is_empty() || path.is_dir() {
            break;
        }
        // A file in the way is refused, never replaced.
        if path.exists() {
            return Err(RelocateError::DestinationNotADirectory(path.to_path_buf()));
        }
        // The rule applies to every level this run would create, not just the
        // leaf (issue #25: a created path is a destination too).
        if is_excluded_destination(&path.to_string_lossy()) {
            return Err(RelocateError::DestinationExcluded(path.to_path_buf()));
        }
        levels.push(path.to_path_buf());
        current = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
    }
    levels.reverse();
    Ok(levels)
}

/// Validate one path against a destination already known to be usable (it
/// exists, or the batch will create it).
fn plan_item(source: &Path, destination: &Path) -> Result<RelocatePlan, RelocateError> {
    let kind = source_kind(source)?;
    // The source side is the tree rules only: a hidden or timestamped name the
    // user named explicitly is a legitimate source (issue #22).
    if is_out_of_bounds_source(&canonical_text(source)) {
        return Err(RelocateError::SourceExcluded(source.to_path_buf()));
    }
    check_not_inside_itself(source, destination, kind)?;
    let file_name = source
        .file_name()
        .ok_or_else(|| RelocateError::SourceMissing(source.to_path_buf()))?;
    let destination_path = destination.join(file_name);
    if destination_path.exists() {
        return Err(RelocateError::Collision(destination_path));
    }
    Ok(RelocatePlan {
        source: source.to_path_buf(),
        destination_directory: destination.to_path_buf(),
        destination_path,
        kind,
    })
}

/// The source must be a regular file or a folder, never a link or a device.
fn source_kind(source: &Path) -> Result<EntryKind, RelocateError> {
    let Ok(metadata) = std::fs::symlink_metadata(source) else {
        return Err(RelocateError::SourceMissing(source.to_path_buf()));
    };
    if metadata.file_type().is_symlink() {
        return Err(RelocateError::SourceNotMovable(source.to_path_buf()));
    }
    if metadata.is_file() {
        return Ok(EntryKind::File);
    }
    if metadata.is_dir() {
        return Ok(EntryKind::Dir);
    }
    Err(RelocateError::SourceNotMovable(source.to_path_buf()))
}

/// Refuse a move into the source's own directory or into its own subtree.
fn check_not_inside_itself(
    source: &Path,
    destination: &Path,
    kind: EntryKind,
) -> Result<(), RelocateError> {
    let parent = source.parent().map(Path::to_path_buf).unwrap_or_default();
    if same_directory(&parent, destination) {
        return Err(RelocateError::SameDirectory(destination.to_path_buf()));
    }
    if kind == EntryKind::Dir {
        let source_canonical =
            std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
        let destination_canonical =
            std::fs::canonicalize(destination).unwrap_or_else(|_| destination.to_path_buf());
        if crate::benchmark::is_inside(&source_canonical, &destination_canonical) {
            return Err(RelocateError::Nested(
                source.to_path_buf(),
                destination.to_path_buf(),
            ));
        }
    }
    Ok(())
}

/// The canonical path as text, falling back to the given spelling.
fn canonical_text(path: &Path) -> String {
    match std::fs::canonicalize(path) {
        Ok(canonical) => canonical.to_string_lossy().to_string(),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

/// True when a *source* tree is out of bounds.
///
/// The source side is the tree rules only: a Library tree, a system area, a
/// cache or temporary area, an app bundle. The destination predicate also
/// refuses hidden and machine-generated names, which a source the user named
/// explicitly may legitimately have - `kaseifu move ~/Desktop/.env --to
/// ~/Documents/backup` is a move the user asked for, and so is one out of a
/// timestamp-named folder (the folder population issue #22 measured).
fn is_out_of_bounds_source(path: &str) -> bool {
    let components: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    components.iter().any(|part| is_bundle_container(part))
        || is_library_tree(&components)
        || is_cache_or_temp(&components)
        || is_system_area(&components)
        || (path.starts_with('/')
            && components
                .first()
                .is_some_and(|name| is_system_tree_name(name)))
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Dry run: verify the plan still holds and report it.
pub fn dry_run(plan: &RelocatePlan) -> Result<RelocateOutcome, RelocateError> {
    verify_preconditions(plan, false)?;
    Ok(RelocateOutcome::WouldMove)
}

/// Dry run for a batch: verify every planned item, create nothing (issue #25).
pub fn dry_run_batch(batch: &MoveBatch) -> Result<(), RelocateError> {
    let may_create = !batch.missing_directories.is_empty();
    for plan in batch.planned() {
        verify_preconditions(plan, may_create)?;
    }
    Ok(())
}

/// Execute one move, journaling it before the mutation.
pub fn execute_relocate(
    plan: &RelocatePlan,
    history: &OperationHistory,
) -> Result<RelocateOutcome, RelocateError> {
    let operation_id = execute_one(plan, history, None)?;
    Ok(RelocateOutcome::Moved { operation_id })
}

/// Execute a planned batch (issue #25): create what is missing, move every
/// planned path, and leave no empty directory behind.
///
/// Order matches every other mutation in this tool: journal, mutate, mark. The
/// creation is journaled per level so `undo` can remove exactly what this run
/// made; a run where nothing could be moved removes the directories it created
/// instead of leaving an empty folder.
pub fn execute_batch(
    batch: &MoveBatch,
    history: &OperationHistory,
) -> Result<BatchOutcome, RelocateError> {
    let batch_id = crate::storage::history::new_batch_id();
    let creations = create_missing_directories(batch, history, &batch_id)?;

    let mut items = Vec::with_capacity(batch.items.len());
    for item in &batch.items {
        let outcome = match &item.plan {
            Err(error) => ItemOutcome {
                operation_id: None,
                error: Some(error.clone()),
            },
            Ok(plan) => match execute_one(plan, history, Some(&batch_id)) {
                Ok(operation_id) => ItemOutcome {
                    operation_id: Some(operation_id),
                    error: None,
                },
                Err(error) => ItemOutcome {
                    operation_id: None,
                    error: Some(error),
                },
            },
        };
        items.push(outcome);
    }

    // A directory this run created must not survive empty (issue #25): when no
    // path made it in, the journal rows are retired with it.
    if items.iter().all(|item| !item.moved()) {
        retire_created_directories(history, &creations, &batch.missing_directories);
    }

    let created_directories = batch
        .missing_directories
        .iter()
        .filter(|directory| directory.is_dir())
        .cloned()
        .collect();
    Ok(BatchOutcome {
        batch_id,
        items,
        created_directories,
    })
}

/// Journal and create every missing level, returning the rows to reconcile.
///
/// The rows are written before the mutation (like any other operation) so an
/// interrupted run leaves a record instead of an unexplained directory.
fn create_missing_directories(
    batch: &MoveBatch,
    history: &OperationHistory,
    batch_id: &str,
) -> Result<Vec<(String, PathBuf)>, RelocateError> {
    let mut creations = Vec::with_capacity(batch.missing_directories.len());
    for directory in &batch.missing_directories {
        let row = history
            .record_directory_creation(directory, Some(batch_id))
            .map_err(|error| RelocateError::History(error.to_string()))?;
        creations.push((row, directory.clone()));
    }
    let Some(leaf) = batch.missing_directories.last() else {
        return Ok(creations);
    };
    if let Err(error) = std::fs::create_dir_all(leaf) {
        for (row, _) in &creations {
            let _ = history.mark_failure(row, &error.to_string());
        }
        remove_empty_directories(&batch.missing_directories);
        return Err(RelocateError::Io(error.to_string()));
    }
    for (row, _) in &creations {
        let _ = history.mark_success(row);
    }
    Ok(creations)
}

/// Remove the directories this run created and retire their journal rows.
fn retire_created_directories(
    history: &OperationHistory,
    creations: &[(String, PathBuf)],
    levels: &[PathBuf],
) {
    let removed = remove_empty_directories(levels);
    for (row, directory) in creations {
        if removed.iter().any(|path| path == directory) {
            let _ = history.mark_failure(row, "removed: nothing was moved into it");
        }
    }
}

/// One journaled move (the shared body of the single and batch paths).
fn execute_one(
    plan: &RelocatePlan,
    history: &OperationHistory,
    batch_id: Option<&str>,
) -> Result<String, RelocateError> {
    verify_preconditions(plan, false)?;
    let operation_id = history
        .record_pending_in_batch(PendingRecord {
            operation: "move",
            source: &plan.source,
            destination: &plan.destination_path,
            candidate_id: None,
            confidence: None,
            margin: None,
            batch_id,
            kind: Some(plan.kind.as_str()),
        })
        .map_err(|error| RelocateError::History(error.to_string()))?;
    match transfer(&plan.source, &plan.destination_path) {
        Ok(()) => {
            history
                .mark_success(&operation_id)
                .map_err(|error| RelocateError::History(error.to_string()))?;
            Ok(operation_id)
        }
        Err(error) => {
            let _ = history.mark_failure(&operation_id, &error.to_string());
            Err(RelocateError::Io(error.to_string()))
        }
    }
}

/// Remove the directories kaseifu created for one batch, deepest first, and
/// only while they are empty.
pub fn remove_created_directories(
    history: &OperationHistory,
    batch_id: Option<&str>,
) -> Vec<PathBuf> {
    let Some(batch_id) = batch_id else {
        return Vec::new();
    };
    let Ok(rows) = history.created_directories_in_batch(batch_id) else {
        return Vec::new();
    };
    let levels: Vec<PathBuf> = rows
        .iter()
        .map(|row| PathBuf::from(&row.destination_path))
        .collect();
    let removed = remove_empty_directories(&levels);
    for row in &rows {
        if removed
            .iter()
            .any(|path| path.to_string_lossy() == row.destination_path)
        {
            let _ = history.mark_undone(&row.id);
        }
    }
    removed
}

/// Remove the given directories, deepest first, only while they are empty.
///
/// `remove_dir` refuses a non-empty directory, which is the whole safety rule:
/// a directory someone started using is left alone.
fn remove_empty_directories(levels: &[PathBuf]) -> Vec<PathBuf> {
    let mut ordered: Vec<&PathBuf> = levels.iter().collect();
    ordered.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    let mut removed = Vec::new();
    for level in ordered {
        if !level.is_dir() {
            continue;
        }
        let empty = std::fs::read_dir(level)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if empty && std::fs::remove_dir(level).is_ok() {
            removed.push(level.clone());
        }
    }
    removed
}

/// Re-verify the preconditions immediately before the mutation (TOCTOU).
///
/// `may_create` allows the destination to be missing: a batch creates it
/// before the first move, so the check runs again with the directory in place.
fn verify_preconditions(plan: &RelocatePlan, may_create: bool) -> Result<(), RelocateError> {
    if !plan.source.exists() {
        return Err(RelocateError::SourceMissing(plan.source.clone()));
    }
    if !plan.destination_directory.is_dir() && !(may_create && !plan.destination_directory.exists())
    {
        return Err(RelocateError::DestinationNotADirectory(
            plan.destination_directory.clone(),
        ));
    }
    if plan.destination_path.exists() {
        return Err(RelocateError::Collision(plan.destination_path.clone()));
    }
    Ok(())
}

/// Move one path: `rename`, or copy-verify-delete across volumes.
pub fn transfer(source: &Path, destination: &Path) -> io::Result<()> {
    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if crosses_devices(&error) => copy_then_remove(source, destination),
        Err(error) => Err(error),
    }
}

/// True for `EXDEV` (the one `rename` failure that a copy can answer).
fn crosses_devices(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::EXDEV)
}

/// Copy the tree, verify it, then remove the source.
///
/// Public because it is the whole cross-volume path, and a test that has no
/// second volume still has to exercise it.
///
/// A failed copy or a failed verification removes the partial copy: the source
/// is then untouched. A source that cannot be removed is a different failure
/// and is reported with **both** sides intact - a verified copy is never
/// deleted because the other side failed, or the data would exist nowhere.
pub fn copy_then_remove(source: &Path, destination: &Path) -> io::Result<()> {
    let tally = match copy_tree(source, destination) {
        Ok(tally) => tally,
        Err(error) => {
            let _ = remove_path(destination);
            return Err(error);
        }
    };
    if let Err(error) = verify_copy(destination, &tally) {
        let _ = remove_path(destination);
        return Err(error);
    }
    if let Err(error) = remove_path(source) {
        return Err(io::Error::other(format!(
            "copied to {} but the source could not be removed ({error}); both were left in place",
            destination.display()
        )));
    }
    Ok(())
}

/// What a copy produced: how many files, how many bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CopyTally {
    files: u64,
    bytes: u64,
}

/// Copy one file or one tree, returning what was written.
///
/// Symbolic links are refused rather than dropped: a copy that silently loses a
/// link is not the move the user asked for.
fn copy_tree(source: &Path, destination: &Path) -> io::Result<CopyTally> {
    if source.is_file() {
        let bytes = std::fs::copy(source, destination)?;
        return Ok(CopyTally { files: 1, bytes });
    }
    std::fs::create_dir_all(destination)?;
    let mut tally = CopyTally::default();
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry.map_err(walk_error)?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            let bytes = std::fs::copy(entry.path(), &target)?;
            tally.files += 1;
            tally.bytes += bytes;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "only regular files and folders are copied: {}",
                    entry.path().display()
                ),
            ));
        }
    }
    Ok(tally)
}

/// One walkdir failure as an IO error.
fn walk_error(error: walkdir::Error) -> io::Error {
    io::Error::other(error.to_string())
}

/// Re-count the copy and compare it with what the copy reported.
fn verify_copy(destination: &Path, expected: &CopyTally) -> io::Result<()> {
    let actual = measure_copy(destination)?;
    if actual != *expected {
        return Err(io::Error::other(format!(
            "the copy does not match: {} files / {} bytes written, {} files / {} bytes found",
            expected.files, expected.bytes, actual.files, actual.bytes
        )));
    }
    Ok(())
}

/// Count the files and bytes under `path`.
fn measure_copy(path: &Path) -> io::Result<CopyTally> {
    if path.is_file() {
        return Ok(CopyTally {
            files: 1,
            bytes: std::fs::metadata(path)?.len(),
        });
    }
    let mut tally = CopyTally::default();
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry.map_err(walk_error)?;
        if entry.file_type().is_file() {
            tally.files += 1;
            tally.bytes += entry.metadata().map_err(walk_error)?.len();
        }
    }
    Ok(tally)
}

/// Remove a file or a tree (used for the copy and by `undo`).
pub fn remove_path(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-sandboxes")
            .join(format!("relocate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("sandbox");
        root
    }

    #[test]
    fn the_copy_path_moves_a_tree_and_leaves_nothing_behind() {
        let root = sandbox("copy");
        let source = root.join("source");
        std::fs::create_dir_all(source.join("nested")).expect("nested");
        std::fs::write(source.join("a.txt"), b"a").expect("a");
        std::fs::write(source.join("nested/b.txt"), b"bb").expect("b");
        let destination = root.join("destination");

        copy_then_remove(&source, &destination).expect("copy");

        assert!(!source.exists(), "the source is gone");
        assert_eq!(std::fs::read(destination.join("a.txt")).expect("a"), b"a");
        assert_eq!(
            std::fs::read(destination.join("nested/b.txt")).expect("b"),
            b"bb"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_copy_leaves_the_source_and_no_residue() {
        let root = sandbox("failed-copy");
        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("source");
        std::fs::write(source.join("a.txt"), b"a").expect("a");
        // A dangling symlink is refused by the copy, after the first file was
        // already written.
        std::os::unix::fs::symlink(root.join("missing"), source.join("link")).expect("symlink");
        let destination = root.join("destination");

        let error = copy_then_remove(&source, &destination).expect_err("must fail");

        assert!(
            error.to_string().contains("only regular files and folders"),
            "{error}"
        );
        assert!(source.join("a.txt").is_file(), "the source is untouched");
        assert!(!destination.exists(), "no partial copy is left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_source_that_cannot_be_removed_keeps_both_sides() {
        use std::os::unix::fs::PermissionsExt;

        let root = sandbox("source-locked");
        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("source");
        std::fs::write(source.join("a.txt"), b"a").expect("a");
        let destination = root.join("destination");

        // Readable but not writable: the copy succeeds, removing the children
        // does not.
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o555))
            .expect("read-only source");
        let result = copy_then_remove(&source, &destination);
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).expect("restore");

        let error = result.expect_err("the source cannot be removed");
        assert!(
            error.to_string().contains("both were left in place"),
            "{error}"
        );
        // The verified copy is kept: deleting it would lose the only copy of
        // anything the partial removal already took.
        assert_eq!(
            std::fs::read(destination.join("a.txt")).expect("copy"),
            b"a"
        );
        assert!(source.join("a.txt").is_file(), "the source is still there");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_kind_words_are_the_journal_words() {
        assert_eq!(EntryKind::File.as_str(), "file");
        assert_eq!(EntryKind::Dir.as_str(), "dir");
    }
}

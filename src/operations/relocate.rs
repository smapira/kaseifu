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
use crate::storage::history::OperationHistory;
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

/// Validate one move without touching anything.
pub fn plan_relocate(source: &Path, destination: &Path) -> Result<RelocatePlan, RelocateError> {
    let kind = source_kind(source)?;
    // The never-a-destination rules come first: a place a person never picks
    // (`~/Library/**`, caches, system trees) is refused as *that*, not as "not a
    // directory", and the answer needs no filesystem read of the destination.
    if is_out_of_bounds_source(&canonical_text(source)) {
        return Err(RelocateError::SourceExcluded(source.to_path_buf()));
    }
    if is_excluded_destination(&destination.to_string_lossy()) {
        return Err(RelocateError::DestinationExcluded(
            destination.to_path_buf(),
        ));
    }
    if !destination.is_dir() {
        return Err(RelocateError::DestinationNotADirectory(
            destination.to_path_buf(),
        ));
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
    verify_preconditions(plan)?;
    Ok(RelocateOutcome::WouldMove)
}

/// Execute the move, journaling it before the mutation.
///
/// Order matches every other mutation in this tool (`operations::mover`):
/// verify, record, transfer, mark. The journal row carries `kind`, and the
/// selection columns stay NULL exactly like a `--to` move, so `undo` and
/// `doctor` read it the same way.
pub fn execute_relocate(
    plan: &RelocatePlan,
    history: &OperationHistory,
) -> Result<RelocateOutcome, RelocateError> {
    verify_preconditions(plan)?;
    let operation_id = history
        .record_pending_kind(
            "move",
            &plan.source,
            &plan.destination_path,
            None,
            None,
            None,
            plan.kind.as_str(),
        )
        .map_err(|error| RelocateError::History(error.to_string()))?;

    match transfer(&plan.source, &plan.destination_path) {
        Ok(()) => {
            if let Err(error) = history.mark_success(&operation_id) {
                return Err(RelocateError::History(error.to_string()));
            }
            Ok(RelocateOutcome::Moved { operation_id })
        }
        Err(error) => {
            let _ = history.mark_failure(&operation_id, &error.to_string());
            Err(RelocateError::Io(error.to_string()))
        }
    }
}

/// Re-verify the preconditions immediately before the mutation (TOCTOU).
fn verify_preconditions(plan: &RelocatePlan) -> Result<(), RelocateError> {
    if !plan.source.exists() {
        return Err(RelocateError::SourceMissing(plan.source.clone()));
    }
    if !plan.destination_directory.is_dir() {
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

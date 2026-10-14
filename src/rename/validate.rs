//! File name validation (original spec section 9.1).
//!
//! Every check refuses with a reason; dangerous characters from AI output
//! are an error, never silently replaced. Small one-purpose functions keep
//! the complexity gate quiet.

use std::path::Path;

/// Maximum file name length in bytes (filesystem limit guard).
pub const MAX_NAME_BYTES: usize = 255;

/// Why a proposed name was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    DotEntry,
    Separator,
    TooLong,
    ExtensionChanged,
    Symlink,
    Collision(String),
    ParentMissing,
    ParentNotADirectory,
    ReadOnly,
    SourceMissing,
    SourceNotAFile,
    SourceChanged,
}

impl std::fmt::Display for NameProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "name must not be empty"),
            Self::DotEntry => write!(f, "name must not be . or .."),
            Self::Separator => write!(f, "name must not contain a path separator"),
            Self::TooLong => write!(f, "name exceeds {MAX_NAME_BYTES} bytes"),
            Self::ExtensionChanged => write!(f, "extension change is not allowed"),
            Self::Symlink => write!(f, "symbolic links are out of scope"),
            Self::Collision(path) => write!(f, "refusing to overwrite {path}"),
            Self::ParentMissing => write!(f, "parent directory is missing"),
            Self::ParentNotADirectory => write!(f, "parent is not a directory"),
            Self::ReadOnly => write!(f, "target is not writable"),
            Self::SourceMissing => write!(f, "source no longer exists"),
            Self::SourceNotAFile => write!(f, "source is no longer a file"),
            Self::SourceChanged => write!(f, "source changed since the proposal"),
        }
    }
}

/// Check the shape of a bare file name against the current extension.
///
/// The extension must match exactly: Phase 4 never changes it.
pub fn check_filename(name: &str, current_extension: &str) -> Result<(), NameProblem> {
    check_nonempty(name)?;
    check_dot_entry(name)?;
    check_separators(name)?;
    check_length(name)?;
    check_extension_kept(name, current_extension)?;
    Ok(())
}

/// Refuse empty names.
fn check_nonempty(name: &str) -> Result<(), NameProblem> {
    if name.is_empty() {
        return Err(NameProblem::Empty);
    }
    Ok(())
}

/// Refuse `.` and `..`.
fn check_dot_entry(name: &str) -> Result<(), NameProblem> {
    if name == "." || name == ".." {
        return Err(NameProblem::DotEntry);
    }
    Ok(())
}

/// Refuse `/`, NUL, backslash and `:` (Finder separator compatibility).
fn check_separators(name: &str) -> Result<(), NameProblem> {
    if name.contains('/') || name.contains('\0') || name.contains('\\') || name.contains(':') {
        return Err(NameProblem::Separator);
    }
    Ok(())
}

/// Refuse over-long names.
fn check_length(name: &str) -> Result<(), NameProblem> {
    if name.len() > MAX_NAME_BYTES {
        return Err(NameProblem::TooLong);
    }
    Ok(())
}

/// Refuse extension changes (compared case-sensitively on the raw text).
fn check_extension_kept(name: &str, current_extension: &str) -> Result<(), NameProblem> {
    let kept = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if kept != current_extension {
        return Err(NameProblem::ExtensionChanged);
    }
    Ok(())
}

/// True when the path itself is a symbolic link (checked before resolve).
pub fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// A same-directory rename target for `new_name`.
pub fn sibling_path(source: &Path, new_name: &str) -> Option<std::path::PathBuf> {
    source.parent().map(|parent| parent.join(new_name))
}

/// A lightweight snapshot of the proposal-time source (TOCTOU recheck).
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
}

/// Snapshot the source file (size and mtime).
pub fn snapshot(source: &Path) -> Result<FileSnapshot, NameProblem> {
    let metadata = std::fs::metadata(source).map_err(|_| NameProblem::SourceMissing)?;
    if !metadata.is_file() {
        return Err(NameProblem::SourceNotAFile);
    }
    Ok(FileSnapshot {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// Re-verify the execution preconditions immediately before rename.
///
/// Refuses when the source vanished, changed, or became unwritable, when the
/// destination appeared, or when the parent directory is unusable.
pub fn recheck_before_execute(
    source: &Path,
    destination: &Path,
    before: &FileSnapshot,
) -> Result<(), NameProblem> {
    check_source_stable(source, before)?;
    check_destination_free(destination)?;
    check_parent_usable(destination)?;
    Ok(())
}

/// The source must still be the file the proposal was built for.
fn check_source_stable(source: &Path, before: &FileSnapshot) -> Result<(), NameProblem> {
    if is_symlink(source) {
        return Err(NameProblem::Symlink);
    }
    let metadata = std::fs::metadata(source).map_err(|_| NameProblem::SourceMissing)?;
    if !metadata.is_file() {
        return Err(NameProblem::SourceNotAFile);
    }
    if metadata.permissions().readonly() {
        return Err(NameProblem::ReadOnly);
    }
    let now = FileSnapshot {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    };
    if now.size != before.size || now.modified != before.modified {
        return Err(NameProblem::SourceChanged);
    }
    Ok(())
}

/// The destination must not exist: renames never overwrite.
fn check_destination_free(destination: &Path) -> Result<(), NameProblem> {
    if destination.exists() {
        return Err(NameProblem::Collision(destination.display().to_string()));
    }
    Ok(())
}

/// The destination parent must exist and be a directory.
fn check_parent_usable(destination: &Path) -> Result<(), NameProblem> {
    let Some(parent) = destination.parent() else {
        return Err(NameProblem::ParentMissing);
    };
    if !parent.exists() {
        return Err(NameProblem::ParentMissing);
    }
    if !parent.is_dir() {
        return Err(NameProblem::ParentNotADirectory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_shapes_are_checked() {
        assert_eq!(check_filename("", "txt"), Err(NameProblem::Empty));
        assert_eq!(check_filename(".", "txt"), Err(NameProblem::DotEntry));
        assert_eq!(check_filename("..", "txt"), Err(NameProblem::DotEntry));
        assert_eq!(
            check_filename("a/b.txt", "txt"),
            Err(NameProblem::Separator)
        );
        assert_eq!(
            check_filename("a\u{0}b.txt", "txt"),
            Err(NameProblem::Separator)
        );
        assert_eq!(
            check_filename("a:b.txt", "txt"),
            Err(NameProblem::Separator)
        );
        assert_eq!(
            check_filename(&"a".repeat(300), "txt"),
            Err(NameProblem::TooLong)
        );
        assert_eq!(
            check_filename("a.md", "txt"),
            Err(NameProblem::ExtensionChanged)
        );
        assert_eq!(check_filename("2026-10-14_title.txt", "txt"), Ok(()));
    }
}

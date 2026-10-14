//! File name validation (original spec section 9.1).
//!
//! Every check refuses with a reason; dangerous characters from AI output
//! are an error, never silently replaced. Small one-purpose functions keep
//! the complexity gate quiet.

use std::os::unix::fs::MetadataExt;
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
    SourceNotADirectory,
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
            Self::SourceNotADirectory => write!(f, "source is no longer a folder"),
            Self::SourceChanged => write!(f, "source changed since the proposal"),
        }
    }
}

/// Check the shape of a bare file name against the current extension.
///
/// The extension must match exactly: Phase 4 never changes it.
pub fn check_filename(name: &str, current_extension: &str) -> Result<(), NameProblem> {
    check_name_shape(name)?;
    check_extension_kept(name, current_extension)
}

/// Check the shape of a bare folder name.
///
/// A folder has no extension, so the extension rule does not apply to it: a
/// dot inside the name is part of the name (`v1.2 notes`), not a second
/// extension. Every other rule is the same one files answer to, checked by
/// the same functions (original spec section 9.1).
pub fn check_directory_name(name: &str) -> Result<(), NameProblem> {
    check_name_shape(name)
}

/// The shape rules every rename target answers to.
fn check_name_shape(name: &str) -> Result<(), NameProblem> {
    check_nonempty(name)?;
    check_dot_entry(name)?;
    check_separators(name)?;
    check_length(name)?;
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

/// A proposal-time identity for a folder (TOCTOU recheck).
///
/// A folder's size and modification time move every time a child is added or
/// removed, so comparing them would refuse legitimate renames while still not
/// answering the real question ("is this the same folder?"). The device and
/// inode pair answers exactly that question: the folder may have gained and
/// lost children, but it is still the same object on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySnapshot {
    pub device: u64,
    pub inode: u64,
}

/// Snapshot a folder's identity (device and inode).
pub fn snapshot_directory(source: &Path) -> Result<DirectorySnapshot, NameProblem> {
    let metadata = std::fs::metadata(source).map_err(|_| NameProblem::SourceMissing)?;
    if !metadata.is_dir() {
        return Err(NameProblem::SourceNotADirectory);
    }
    Ok(DirectorySnapshot {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Re-verify the execution preconditions for a folder immediately before
/// rename.
///
/// Refuses when the folder vanished, became something else, is no longer the
/// same object, or became unwritable, when the destination appeared, or when
/// the parent directory is unusable. Same shape as
/// [`recheck_before_execute`], with the folder identity in place of the file
/// size and modification time.
pub fn recheck_directory_before_execute(
    source: &Path,
    destination: &Path,
    before: &DirectorySnapshot,
) -> Result<(), NameProblem> {
    check_directory_stable(source, before)?;
    check_destination_free(destination)?;
    check_parent_usable(destination)?;
    Ok(())
}

/// The folder must still be the folder the proposal was built for.
fn check_directory_stable(source: &Path, before: &DirectorySnapshot) -> Result<(), NameProblem> {
    if is_symlink(source) {
        return Err(NameProblem::Symlink);
    }
    let metadata = std::fs::metadata(source).map_err(|_| NameProblem::SourceMissing)?;
    if !metadata.is_dir() {
        return Err(NameProblem::SourceNotADirectory);
    }
    if metadata.permissions().readonly() {
        return Err(NameProblem::ReadOnly);
    }
    if metadata.dev() != before.device || metadata.ino() != before.inode {
        return Err(NameProblem::SourceChanged);
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

    #[test]
    fn directory_names_keep_their_dots_and_answer_the_other_rules() {
        assert_eq!(check_directory_name(""), Err(NameProblem::Empty));
        assert_eq!(check_directory_name("."), Err(NameProblem::DotEntry));
        assert_eq!(check_directory_name(".."), Err(NameProblem::DotEntry));
        assert_eq!(check_directory_name("a/b"), Err(NameProblem::Separator));
        assert_eq!(check_directory_name("a\u{0}b"), Err(NameProblem::Separator));
        assert_eq!(check_directory_name("a:b"), Err(NameProblem::Separator));
        assert_eq!(
            check_directory_name(&"a".repeat(300)),
            Err(NameProblem::TooLong)
        );
        // A folder has no extension: a dot is part of the name.
        for name in ["v1.2 notes", "2026. invoices", "project.x"] {
            assert_eq!(check_directory_name(name), Ok(()), "must pass: {name}");
            assert_eq!(
                check_filename(name, ""),
                Err(NameProblem::ExtensionChanged),
                "the file rule stays strict about dots: {name}"
            );
        }
        // Without a dot the two rules agree (the file rule only objects when
        // the last dot would read as a changed extension).
        let japanese = "\u{8acb}\u{6c42}\u{30fb}\u{5951}\u{7d04}\u{95a2}\u{9023}\u{66f8}\u{985e}";
        assert_eq!(check_directory_name(japanese), Ok(()));
        assert_eq!(check_filename(japanese, ""), Ok(()));
    }

    #[test]
    fn directory_snapshots_track_the_object_not_its_contents() {
        let dir = std::env::temp_dir().join(format!("kaseifu-rename-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let folder = dir.join("folder");
        std::fs::create_dir_all(&folder).expect("folder");
        let before = snapshot_directory(&folder).expect("snapshot");
        // Writing a child moves the folder's size and mtime; the identity stays
        // and the recheck must accept it (a legitimate rename is not refused).
        std::fs::write(folder.join("child.txt"), b"x").expect("child");
        assert_eq!(
            recheck_directory_before_execute(&folder, &dir.join("moved"), &before),
            Ok(())
        );
        // A different folder at the same path is a different object.
        let replaced = dir.join("folder");
        std::fs::remove_dir_all(&replaced).expect("remove");
        std::fs::create_dir_all(&replaced).expect("recreate");
        assert_eq!(
            recheck_directory_before_execute(&replaced, &dir.join("moved"), &before),
            Err(NameProblem::SourceChanged)
        );
        // A file is not a folder.
        let file = dir.join("file.txt");
        std::fs::write(&file, b"x").expect("file");
        assert_eq!(
            snapshot_directory(&file),
            Err(NameProblem::SourceNotADirectory)
        );
        // The destination must stay free.
        let taken = dir.join("taken");
        std::fs::create_dir_all(&taken).expect("taken");
        let fresh = snapshot_directory(&folder).expect("snapshot again");
        assert!(matches!(
            recheck_directory_before_execute(&folder, &taken, &fresh),
            Err(NameProblem::Collision(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

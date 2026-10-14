//! Folder classification for the rename service (original spec section 5.4).
//!
//! Mirrors `rename::image` and `rename::video`: the rename layer decides what
//! a target is and refuses everything else with the reason, while the path
//! rules themselves stay in `crate::context::directory` (shared with the
//! destination policy, so a rule has one implementation).

use std::path::Path;

use crate::context::directory::{DirectoryRefusal, RefusalEnv, is_directory_target, refuse_reason};

/// Why a path is not a folder kaseifu may rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryError {
    /// The path is not a folder (a file, or nothing at all).
    NotADirectory(String),
    /// The folder is one kaseifu refuses to touch (original spec section 9.1
    /// plus the platform rules that keep macOS and app storage out of reach).
    Refused(DirectoryRefusal),
}

impl std::fmt::Display for DirectoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotADirectory(detail) => write!(f, "{detail}"),
            Self::Refused(refusal) => write!(f, "{refusal}"),
        }
    }
}

/// Accept a path only when it is a folder.
///
/// The filesystem type decides, not the name: a folder called `IMG_0001.png`
/// is a folder (and is named as one).
pub fn classify_directory(path: &Path) -> Result<(), DirectoryError> {
    if !is_directory_target(path) {
        return Err(DirectoryError::NotADirectory(format!(
            "{} is not a folder",
            path.display()
        )));
    }
    Ok(())
}

/// Refuse the folder shapes kaseifu must not rename.
///
/// `path` is the canonical folder path (the caller canonicalizes so a relative
/// target cannot dodge the home and working directory comparisons).
pub fn refuse_rename(path: &Path, env: &RefusalEnv) -> Result<(), DirectoryError> {
    match refuse_reason(path, env) {
        Some(refusal) => Err(DirectoryError::Refused(refusal)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn work_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kaseifu-ren-dir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn folders_are_accepted_and_other_paths_are_refused() {
        let dir = work_dir("classify");
        let folder = dir.join("folder");
        std::fs::create_dir_all(&folder).expect("folder");
        let file = dir.join("note.txt");
        std::fs::write(&file, b"x").expect("file");
        assert_eq!(classify_directory(&folder), Ok(()));
        assert!(matches!(
            classify_directory(&file),
            Err(DirectoryError::NotADirectory(_))
        ));
        assert!(matches!(
            classify_directory(&dir.join("missing")),
            Err(DirectoryError::NotADirectory(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_refusals_are_reported_with_their_reason() {
        let without_env = RefusalEnv::default();
        assert_eq!(
            refuse_rename(Path::new("/Users/test/Documents/notes"), &without_env),
            Ok(())
        );
        let refusal = refuse_rename(Path::new("/Users/test/Downloads/App.app"), &without_env)
            .expect_err("a bundle is refused");
        assert_eq!(refusal, DirectoryError::Refused(DirectoryRefusal::Bundle));
        assert!(refusal.to_string().contains("app-managed"), "{refusal}");
    }
}

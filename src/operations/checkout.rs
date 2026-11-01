//! Which git working copy a path belongs to (v114, issue #17).
//!
//! Discovery is machine-wide, so a candidate destination can be another
//! working copy of the repository the source lives in. The measured case
//! (2026-10-19, `seo-operations`): organizing a file from
//! `.delta/worktrees/<id>/...` selected the *primary* checkout as the
//! destination, because Spotlight never returns the hidden worktree path
//! itself, so only the visible sibling checkout can be a candidate at all.
//! `--execute` then puts the file into that other checkout as an untracked
//! file, and the dry-run report said nothing about it.
//!
//! This module answers one question without spawning `git`: does the
//! destination belong to a *different* working copy of the same repository?
//! The walk is the one `repo_policy::locate_root` already does (a few `stat`
//! calls per run), and the identity is the git common directory, which is what
//! `git rev-parse --git-common-dir` reports:
//!
//! - `.git` is a directory: that directory is the common directory (a plain
//!   checkout),
//! - `.git` is a file (`gitdir: <path>`): the working copy of a worktree or a
//!   submodule. `<gitdir>/commondir`, when it exists, holds the common
//!   directory relative to `<gitdir>` (a linked worktree shares the
//!   repository's common directory); without it the `<gitdir>` itself is the
//!   common directory (a submodule is its own repository) - so submodules and
//!   separate clones read as *different* repositories and are allowed, exactly
//!   as intended,
//! - the answer is "another working copy" only when the common directory is the
//!   same *and* the root (the directory holding `.git`) is different.
//!
//! Two clones of one origin are deliberately not compared by their `remote.origin`
//! URL: the same URL legitimately appears for an intentional second clone, and
//! reading `.git/config` would refuse those moves.

use std::path::{Path, PathBuf};

/// Prefix of the `.git` *file* form (`gitdir: <path>`).
const GITDIR_PREFIX: &str = "gitdir:";

/// File inside a linked worktree's git directory that names the shared common
/// directory (relative to that directory).
const COMMONDIR_FILE: &str = "commondir";

/// One git working copy: the directory that holds the `.git` entry, plus the
/// common directory shared by every working copy of the same repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRoots {
    /// The working-copy root: the directory that holds the `.git` entry.
    pub root: PathBuf,
    /// The git common directory (`git rev-parse --git-common-dir`).
    pub common_dir: PathBuf,
}

/// The working copy `path` belongs to, or `None` when no `.git` entry sits at
/// or above it.
///
/// `path` may be a file or a directory; both spellings are resolved to the
/// nearest `.git` entry at or above them. Both returned paths are normalized
/// the way the rest of the crate normalizes paths (symlinks resolved), so two
/// spellings of one directory compare equal.
pub fn git_roots(path: &Path) -> Option<GitRoots> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let (root, entry) = search_up(start)?;
    Some(GitRoots {
        root: crate::benchmark::normalize_path(&root),
        common_dir: crate::benchmark::normalize_path(&common_dir_of(&entry)?),
    })
}

/// The git common directory `path` belongs to, or `None` outside git.
pub fn git_common_dir(path: &Path) -> Option<PathBuf> {
    git_roots(path).map(|roots| roots.common_dir)
}

/// The destination's working copy, when it is a *different* working copy of the
/// same repository as `source`.
///
/// `None` means the two paths are in the same working copy, in different
/// repositories (another clone, a submodule), or outside git on either side:
/// nothing about the move is surprising, so nothing has to be said about it.
pub fn other_working_copy(source: &Path, destination: &Path) -> Option<GitRoots> {
    let source = git_roots(source)?;
    let destination = git_roots(destination)?;
    (source.common_dir == destination.common_dir && source.root != destination.root)
        .then_some(destination)
}

/// The nearest directory at or above `start` that holds a `.git` entry (a
/// directory or the worktree/submodule file form), with that entry.
fn search_up(start: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut current = Some(start);
    while let Some(directory) = current {
        let entry = directory.join(".git");
        if entry.is_dir() || entry.is_file() {
            return Some((directory.to_path_buf(), entry));
        }
        current = directory.parent();
    }
    None
}

/// The common directory behind one `.git` entry (directory or file form).
fn common_dir_of(entry: &Path) -> Option<PathBuf> {
    if entry.is_dir() {
        return Some(entry.to_path_buf());
    }
    let git_dir = git_dir_of(entry)?;
    match std::fs::read_to_string(git_dir.join(COMMONDIR_FILE)) {
        // A linked worktree: the content is relative to the git directory.
        Ok(text) => Some(git_dir.join(text.trim())),
        // A submodule: its git directory is its own repository.
        Err(_) => Some(git_dir),
    }
}

/// The git directory named by a `.git` file (`gitdir: <path>`).
///
/// The path is absolute in a linked worktree and relative in a submodule; a
/// relative one is resolved against the directory holding the `.git` file, the
/// way git resolves it.
fn git_dir_of(entry: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(entry).ok()?;
    let raw = text
        .lines()
        .find_map(|line| line.trim().strip_prefix(GITDIR_PREFIX))?
        .trim();
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(entry.parent()?.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One throwaway tree below `target/` (`/var/folders` is a
    /// never-a-destination shape, so fixtures live in the crate's own target
    /// directory).
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(tag: &str) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(format!("v114-checkout-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("sandbox");
            Self { root }
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let path = self.root.join(relative);
            std::fs::create_dir_all(&path).expect("directory");
            path
        }

        fn file(&self, relative: &str, body: &str) -> PathBuf {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("parent");
            }
            std::fs::write(&path, body).expect("file");
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_git_directory_is_its_own_common_directory() {
        let tree = Tree::new("directory");
        tree.dir("clone/.git");
        tree.dir("clone/docs");
        let file = tree.file("clone/docs/brief.md", "brief");

        let roots = git_roots(&file).expect("inside a checkout");
        let clone = std::fs::canonicalize(tree.root.join("clone")).expect("canonical");
        assert_eq!(roots.root, clone);
        assert_eq!(roots.common_dir, clone.join(".git"));
        assert_eq!(git_common_dir(&file), Some(clone.join(".git")));
    }

    #[test]
    fn a_git_file_shared_common_directory_is_a_worktree() {
        let tree = Tree::new("worktree");
        let primary = tree.dir("primary");
        tree.dir("primary/.git/worktrees/wt");
        tree.file("primary/.git/worktrees/wt/commondir", "../..\n");
        let worktree = tree.dir("worktree");
        tree.file(
            "worktree/.git",
            &format!("gitdir: {}\n", primary.join(".git/worktrees/wt").display()),
        );
        tree.dir("worktree/docs");
        let file = tree.file("worktree/docs/brief.md", "brief");

        let roots = git_roots(&file).expect("inside the worktree");
        let common = std::fs::canonicalize(primary.join(".git")).expect("canonical");
        assert_eq!(
            roots.root,
            std::fs::canonicalize(&worktree).expect("canonical")
        );
        assert_eq!(
            roots.common_dir, common,
            "commondir is read relative to gitdir"
        );
        // The primary checkout reports the same common directory.
        assert_eq!(git_common_dir(&primary), Some(common));
    }

    #[test]
    fn a_git_file_without_commondir_is_its_own_repository() {
        let tree = Tree::new("submodule");
        tree.dir("super/.git/modules/lib");
        let module = tree.dir("super/lib");
        tree.file("super/lib/.git", "gitdir: ../.git/modules/lib\n");
        tree.file("super/lib/notes.md", "notes");

        let roots = git_roots(&module.join("notes.md")).expect("inside the module");
        assert_eq!(
            roots.common_dir,
            std::fs::canonicalize(tree.root.join("super/.git/modules/lib")).expect("canonical"),
            "without a commondir the git directory is the repository"
        );
    }

    #[test]
    fn the_walk_finds_the_nearest_git_entry() {
        let tree = Tree::new("nearest");
        tree.dir("outer/.git");
        tree.dir("outer/inner/.git");
        tree.dir("outer/inner/deep");

        let roots = git_roots(&tree.root.join("outer/inner/deep")).expect("inside inner");
        assert_eq!(
            roots.root,
            std::fs::canonicalize(tree.root.join("outer/inner")).expect("canonical")
        );
    }

    #[test]
    fn outside_git_there_is_no_working_copy() {
        let missing = Path::new("/nonexistent-v114-checkout/docs/brief.md");
        assert_eq!(git_roots(missing), None);
        assert_eq!(git_common_dir(missing), None);
        assert_eq!(other_working_copy(missing, missing), None);
    }

    #[test]
    fn other_working_copy_compares_the_common_directory_and_the_root() {
        let tree = Tree::new("compare");
        // A primary checkout and its linked worktree.
        tree.dir("primary/.git/worktrees/wt");
        tree.file("primary/.git/worktrees/wt/commondir", "../..");
        let worktree = tree.dir("worktree");
        tree.file(
            "worktree/.git",
            &format!(
                "gitdir: {}\n",
                tree.root.join("primary/.git/worktrees/wt").display()
            ),
        );
        // A second, unrelated clone.
        tree.dir("other/.git");
        let file = tree.file("worktree/docs/brief.md", "brief");

        let destination = other_working_copy(&file, &tree.root.join("primary"))
            .expect("the primary is another working copy");
        assert_eq!(
            destination.root,
            std::fs::canonicalize(tree.root.join("primary")).expect("canonical")
        );
        assert!(
            other_working_copy(&file, &worktree).is_none(),
            "the file's own working copy is not another one"
        );
        assert!(
            other_working_copy(&file, &tree.root.join("other")).is_none(),
            "a separate clone is a different repository"
        );
    }
}

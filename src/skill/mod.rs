//! Distributing and checking kaseifu's own agent skill
//! (`.github/skills/kaseifu/SKILL.md`), the file another repository's agent
//! reads to call kaseifu.
//!
//! The skill is embedded in this binary (`include_str!`) and a copy is compared
//! by sha256: nobody copies the file by hand, and a stale copy is one exit code
//! away (`kaseifu skill check`). The decisions live here (preview, place,
//! refuse, which directory is a repository); every user-facing string is built
//! by the presentation layer. `sha2` and `walkdir` are already dependencies, so
//! no crate is added.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Where the skill goes, relative to the repository the command is pointed at.
pub const INSTALL_REL_PATH: &str = ".github/skills/kaseifu/SKILL.md";

/// Path elements of [`INSTALL_REL_PATH`] (`.github`, `skills`, `kaseifu`,
/// `SKILL.md`); the unit test below keeps the two in step.
const REL_PATH_ELEMENTS: usize = 4;

/// How deep `skill install --scan` looks for a repository, counted in
/// directories below the scanned one: the scanned directory itself is 0 and a
/// direct child is 1. The limit is on the repository, not on the skill file:
/// a repository at the limit is still found, because the walk only needs to
/// reach the file four elements below it. Six reaches
/// `.delta/worktrees/<id>/<repo>`, the deepest layout a scan is pointed at.
pub const MAX_SCAN_DEPTH: usize = 6;

/// Depth at which the walk stops descending: a directory this deep can only
/// hold a repository past [`MAX_SCAN_DEPTH`], so skipping it loses none.
const SCAN_WALK_DEPTH: usize = MAX_SCAN_DEPTH + REL_PATH_ELEMENTS;

/// The skill embedded in this binary: the canonical copy lives in the
/// repository at `src/skill/kaseifu/SKILL.md`, and the distributed copy in the
/// repository (`.github/skills/kaseifu/SKILL.md`) is kept byte-identical to it.
const EMBEDDED: &str = include_str!("kaseifu/SKILL.md");

/// The embedded skill's text.
pub fn embedded() -> &'static str {
    EMBEDDED
}

/// The one digest kaseifu states for a skill.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// The embedded skill's digest: what `skill check` compares a copy against and
/// what every report states.
pub fn embedded_sha256() -> String {
    sha256_hex(EMBEDDED.as_bytes())
}

/// What happened (or would happen) to one copy of the skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStatus {
    /// The copy was already byte-identical to the embedded skill.
    UpToDate,
    /// The embedded skill was written just now.
    Installed,
    /// Nothing was written: the default, and `--write` places the file.
    Preview,
    /// A copy that differs from the embedded skill was left alone.
    Refused,
}

/// One `skill install` target: the file, what happened to it, and the digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// The file the install was about (`<repo>/<INSTALL_REL_PATH>`).
    pub target: PathBuf,
    pub status: InstallStatus,
    /// The embedded skill's digest.
    pub embedded_sha256: String,
    /// The digest of the copy that was there before this call (none: there was
    /// no file).
    pub file_sha256: Option<String>,
}

/// Place the embedded skill at `<repo_dir>/<INSTALL_REL_PATH>`.
///
/// Nothing is written unless `write` is true, so the first run of a command
/// that touches a repository changes nothing. A copy that already matches is
/// left alone whatever the flags say, and a copy that differs is refused unless
/// `force` asks for the replacement: a file somebody edited is never
/// overwritten behind their back. Writing creates the directory chain the way a
/// cassette creates its directory.
pub fn install(repo_dir: &Path, write: bool, force: bool) -> Result<InstallReport> {
    let target = repo_dir.join(INSTALL_REL_PATH);
    let mut report = InstallReport {
        target,
        status: InstallStatus::Preview,
        embedded_sha256: embedded_sha256(),
        file_sha256: None,
    };
    match std::fs::read(&report.target) {
        Ok(existing) => {
            report.file_sha256 = Some(sha256_hex(&existing));
            if report.file_sha256.as_deref() == Some(report.embedded_sha256.as_str()) {
                report.status = InstallStatus::UpToDate;
                return Ok(report);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", report.target.display()));
        }
    }
    if !write {
        return Ok(report);
    }
    if report.file_sha256.is_some() && !force {
        report.status = InstallStatus::Refused;
        return Ok(report);
    }
    write_skill(&report.target)?;
    report.status = InstallStatus::Installed;
    report.file_sha256 = Some(report.embedded_sha256.clone());
    Ok(report)
}

/// Write the embedded skill, creating the directory chain.
fn write_skill(target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    std::fs::write(target, EMBEDDED).with_context(|| format!("cannot write {}", target.display()))
}

/// One `skill check` result: the copy and what the comparison found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// The file that was compared.
    pub path: PathBuf,
    /// True when the copy is byte-identical to the embedded skill.
    pub matches: bool,
    pub embedded_sha256: String,
    pub file_sha256: String,
}

/// Compare the file at `path` with the embedded skill.
///
/// A file that is not there, or cannot be read, is an error: the caller named a
/// file and kaseifu never invents its content. A different byte sequence is not
/// an error here - `matches: false` is the result the caller turns into exit 1.
pub fn check(path: &Path) -> Result<CheckReport> {
    let content = match std::fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(error).with_context(|| {
                format!(
                    "the skill {} does not exist (place it with `kaseifu skill install --to <repo> --write`)",
                    path.display()
                )
            });
        }
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
    };
    let file_sha256 = sha256_hex(&content);
    let embedded_sha256 = embedded_sha256();
    Ok(CheckReport {
        matches: file_sha256 == embedded_sha256,
        path: path.to_path_buf(),
        embedded_sha256,
        file_sha256,
    })
}

/// One repository a scan found: the copy and what the install did with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanTarget {
    /// The skill file the install was about (the path a single install names).
    pub path: PathBuf,
    /// The install's report, or why the copy could not be read or written.
    pub outcome: std::result::Result<InstallReport, String>,
}

/// What `skill install --scan` reports: every repository the scan found, in the
/// order the walk found them (lexical path order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// The directory the scan was pointed at, as given.
    pub parent: PathBuf,
    pub targets: Vec<ScanTarget>,
}

/// Place the embedded skill in every repository at or below `parent` that
/// already holds a copy.
///
/// A directory is a repository when `<dir>/<INSTALL_REL_PATH>` is a regular
/// file. The walk is recursive, lexical and bounded ([`MAX_SCAN_DEPTH`]); it
/// does not descend into `.git` and never follows a symbolic link. Each
/// repository is installed with the rules of a single target, and whatever
/// happened to one of them the scan reports the others and keeps going. Only
/// the scan itself failing is an error: an unreadable directory below `parent`
/// aborts, because a partial list would read like a complete one.
pub fn scan(parent: &Path, write: bool, force: bool) -> Result<ScanReport> {
    if !parent.is_dir() {
        anyhow::bail!(
            "the scan directory is not a directory: {}",
            parent.display()
        );
    }
    let mut report = ScanReport {
        parent: parent.to_path_buf(),
        targets: Vec::new(),
    };
    let walk = WalkDir::new(parent)
        .follow_links(false)
        .sort_by_file_name()
        .max_depth(SCAN_WALK_DEPTH)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != ".git");
    for entry in walk {
        let entry = entry.with_context(|| format!("cannot scan {}", parent.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(repo_dir) = repo_dir_of(parent, entry.path()) else {
            continue;
        };
        let outcome = install(&repo_dir, write, force).map_err(|error| format!("{error:#}"));
        report.targets.push(ScanTarget {
            path: entry.path().to_path_buf(),
            outcome,
        });
    }
    Ok(report)
}

/// The repository a skill file belongs to, for a path the walk found at or
/// below `parent`.
///
/// `None` means the path is not `<repo>/<INSTALL_REL_PATH>`, so it is not a
/// repository copy. The scanned directory itself is a repository when its own
/// skill file is the path.
fn repo_dir_of(parent: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(parent).ok()?;
    if !relative.ends_with(INSTALL_REL_PATH) {
        return None;
    }
    // The skill file sits at the end of the relative path: everything before
    // those elements is the repository directory.
    let mut repo = path.to_path_buf();
    for _ in 0..REL_PATH_ELEMENTS {
        if !repo.pop() {
            return None;
        }
    }
    Some(repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_elements_matches_the_install_path() {
        assert_eq!(INSTALL_REL_PATH.split('/').count(), REL_PATH_ELEMENTS);
    }

    #[test]
    fn embedded_skill_is_ascii_and_named_kaseifu() {
        assert!(EMBEDDED.is_ascii(), "the embedded skill must stay ASCII");
        assert!(EMBEDDED.starts_with("---\nname: kaseifu\n"));
        assert!(
            !EMBEDDED.contains('\u{2014}'),
            "no em dash in the embedded skill"
        );
    }

    #[test]
    fn repo_dir_of_only_accepts_an_install_path() {
        let parent = Path::new("/repos");
        assert_eq!(
            repo_dir_of(
                parent,
                Path::new("/repos/app/.github/skills/kaseifu/SKILL.md")
            ),
            Some(PathBuf::from("/repos/app"))
        );
        assert_eq!(
            repo_dir_of(parent, Path::new("/repos/.github/skills/kaseifu/SKILL.md")),
            Some(PathBuf::from("/repos"))
        );
        assert_eq!(
            repo_dir_of(
                parent,
                Path::new("/repos/app/.github/skills/other/SKILL.md")
            ),
            None
        );
        assert_eq!(repo_dir_of(parent, Path::new("/repos/app/SKILL.md")), None);
    }
}

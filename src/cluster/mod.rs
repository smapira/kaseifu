//! `kaseifu cluster` (issue #26): group the flat files of one directory and
//! file them into abstract-named subdirectories.
//!
//! The command exists because `organize` answers a different question (one file
//! into one existing place) and `move` asks the human for the destination. Here
//! the destination does not exist yet: membership is decided deterministically
//! ([`grouping`]), the name is chosen among deterministic candidates
//! ([`naming`], the only model step), and the move reuses the journaled batch of
//! issue #25 - including its directory creation and undo.
//!
//! Only regular files directly under the root are members by default. Existing
//! directories are discovered, reported and left alone (issue #26 acceptance),
//! never moved.

pub mod grouping;
pub mod naming;

use crate::operations::relocate;
use crate::storage::history::OperationHistory;
use anyhow::{Context, Result, anyhow};
use grouping::{Group, Member};
use naming::{GroupNamer, NameFailure, NameSource};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// How one run is scoped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClusterOptions {
    /// Members below this count are left in the ungrouped bucket.
    pub min: usize,
    /// No group grows past this many members (large types are split).
    pub max: usize,
    /// Treat the root's direct child directories as members too.
    pub include_folders: bool,
    /// Collect files from subdirectories as well (directories stay put).
    pub recursive: bool,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            min: 2,
            max: 20,
            include_folders: false,
            recursive: false,
        }
    }
}

/// One group that will become one directory.
#[derive(Debug, Clone)]
pub struct PlannedGroup {
    pub name: String,
    pub source: NameSource,
    pub candidates: Vec<String>,
    pub members: Vec<Member>,
    /// The validated move of every member into `root/<name>` (issue #25).
    pub batch: relocate::MoveBatch,
}

impl PlannedGroup {
    /// The directory this group creates and fills.
    pub fn directory(&self, root: &Path) -> PathBuf {
        root.join(&self.name)
    }

    /// Bytes over the group's file members.
    pub fn bytes(&self) -> u64 {
        self.members.iter().map(|member| member.bytes).sum()
    }

    /// Up to three member names, for the report.
    pub fn examples(&self) -> Vec<String> {
        self.members
            .iter()
            .take(3)
            .map(|member| member.name.clone())
            .collect()
    }
}

/// Everything a run would do, decided without touching the filesystem.
#[derive(Debug, Clone)]
pub struct ClusterPlan {
    pub root: PathBuf,
    pub groups: Vec<PlannedGroup>,
    pub ungrouped: Vec<Member>,
    /// Direct child directories that were not members and stay where they are.
    pub directories: Vec<PathBuf>,
    /// Entries that were skipped (hidden names, symlinks, special files).
    pub skipped: Vec<PathBuf>,
    /// Why the model could not name the groups (the fallback was used).
    pub naming_failure: Option<String>,
}

impl ClusterPlan {
    /// True when no group was named by the model.
    pub fn names_are_fallback(&self) -> bool {
        self.groups
            .iter()
            .all(|group| group.source == NameSource::Fallback)
    }
}

/// What one group's execution did.
#[derive(Debug, Clone)]
pub struct GroupOutcome {
    pub name: String,
    /// Why the whole group failed (its directory could not be planned/created).
    pub error: Option<String>,
    pub items: Vec<relocate::ItemOutcome>,
}

impl GroupOutcome {
    /// How many members were moved.
    pub fn moved(&self) -> usize {
        self.items.iter().filter(|item| item.moved()).count()
    }
}

/// The result of an executed run (one journal batch).
#[derive(Debug, Clone)]
pub struct ClusterOutcome {
    pub batch_id: String,
    pub groups: Vec<GroupOutcome>,
}

impl ClusterOutcome {
    /// Total members moved.
    pub fn moved(&self) -> usize {
        self.groups.iter().map(GroupOutcome::moved).sum()
    }
}

/// Collect the members a run would consider.
fn collect(root: &Path, options: &ClusterOptions) -> Result<Collected> {
    if !root.is_dir() {
        return Err(anyhow!("not a directory: {}", root.display()));
    }
    if options.recursive && options.include_folders {
        return Err(anyhow!(
            "--recursive walks into folders, so it cannot also treat them as members"
        ));
    }
    if options.recursive {
        return collect_recursive(root);
    }

    let mut collected = Collected::default();
    let entries = std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("read {}", root.display()))?;
        let path = entry.path();
        if is_hidden(&path) {
            collected.skipped.push(path);
            continue;
        }
        let metadata = entry.metadata();
        let Ok(metadata) = metadata else {
            collected.skipped.push(path);
            continue;
        };
        if metadata.is_dir() && !options.include_folders {
            collected.directories.push(path);
            continue;
        }
        if metadata.is_dir() || metadata.is_file() {
            match Member::read(&path) {
                Some(member) => collected.members.push(member),
                None => collected.skipped.push(path),
            }
        } else {
            collected.skipped.push(path);
        }
    }
    Ok(collected)
}

/// Walk the tree for files; directories themselves are never members.
fn collect_recursive(root: &Path) -> Result<Collected> {
    let mut collected = Collected::default();
    let walk = walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(MAX_RECURSIVE_DEPTH)
        .into_iter()
        .filter_entry(|entry| !is_hidden(entry.path()));
    for entry in walk {
        let entry = entry.with_context(|| format!("read {}", root.display()))?;
        let path = entry.path().to_path_buf();
        if entry.file_type().is_symlink() {
            collected.skipped.push(path);
            continue;
        }
        if entry.file_type().is_dir() {
            if entry.depth() == 1 {
                collected.directories.push(path);
            }
            continue;
        }
        if entry.file_type().is_file() {
            match Member::read(&path) {
                Some(member) => collected.members.push(member),
                None => collected.skipped.push(path),
            }
        } else {
            collected.skipped.push(path);
        }
    }
    Ok(collected)
}

/// How deep `--recursive` walks (a sanity bound, not a feature).
const MAX_RECURSIVE_DEPTH: usize = 16;

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

/// What [`collect`] found.
#[derive(Debug, Default)]
struct Collected {
    members: Vec<Member>,
    directories: Vec<PathBuf>,
    skipped: Vec<PathBuf>,
}

/// Decide the whole run: membership, names, destinations. No mutation.
pub async fn plan(
    root: &Path,
    options: &ClusterOptions,
    namer: &dyn GroupNamer,
) -> Result<ClusterPlan> {
    let collected = collect(root, options)?;
    if collected.members.is_empty() {
        return Err(anyhow!(
            "no files to cluster directly under {}",
            root.display()
        ));
    }
    let (groups, ungrouped) = grouping::group(collected.members, options.min, options.max);
    let (named, naming_failure) = names_for(&groups, namer).await;

    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut planned = Vec::with_capacity(groups.len());
    for (group, named) in groups.iter().zip(named) {
        let base = if named.name.is_empty() {
            group
                .candidate_names()
                .first()
                .cloned()
                .unwrap_or_else(|| group.category.default_name().to_string())
        } else {
            named.name
        };
        let distinguishing = if named.source == NameSource::Fallback {
            group.shared_tokens.first().map(String::as_str)
        } else {
            None
        };
        let name = unique_name(root, &base, distinguishing, &mut taken);
        let directory = root.join(&name);
        let sources: Vec<PathBuf> = group
            .members
            .iter()
            .map(|member| member.source.clone())
            .collect();
        let batch = relocate::plan_batch(&sources, &directory, true)
            .map_err(|error| anyhow!("cannot file '{}': {error}", name))?;
        planned.push(PlannedGroup {
            name,
            source: named.source,
            candidates: group.candidate_names(),
            members: group.members.clone(),
            batch,
        });
    }

    Ok(ClusterPlan {
        root: root.to_path_buf(),
        groups: planned,
        ungrouped,
        directories: collected.directories,
        skipped: collected.skipped,
        naming_failure,
    })
}

/// The names for every group; a failing model falls back to every group's
/// deterministic name (never to no plan).
async fn names_for(
    groups: &[Group],
    namer: &dyn GroupNamer,
) -> (Vec<naming::NamedGroup>, Option<String>) {
    match namer.name_groups(groups).await {
        Ok(named) if named.len() == groups.len() => (named, None),
        Ok(_) => (
            fallback_names(groups),
            Some("the model answered for a different number of groups".to_string()),
        ),
        Err(NameFailure { reason }) => (fallback_names(groups), Some(reason)),
    }
}

/// The deterministic names (one per group).
fn fallback_names(groups: &[Group]) -> Vec<naming::NamedGroup> {
    groups
        .iter()
        .map(|group| naming::NamedGroup {
            name: group.candidate_names().first().cloned().unwrap_or_default(),
            source: NameSource::Fallback,
        })
        .collect()
}

/// A directory name that does not collide with an existing entry or an earlier
/// group of this run.
///
/// The plain name wins when it is free. A fallback name that collides first
/// tries `name-<token>`, so two groups of the same type read as `images` and
/// `images-screenshot` instead of `images` and `images (2)`; a number is the
/// last resort (and the only one for a model-chosen name, which is kept as-is).
fn unique_name(
    root: &Path,
    base: &str,
    token: Option<&str>,
    taken: &mut BTreeSet<String>,
) -> String {
    let base = if base.is_empty() {
        "misc".to_string()
    } else {
        base.to_string()
    };
    if is_free(root, &base, taken) {
        taken.insert(base.clone());
        return base;
    }
    if let Some(token) = token {
        let candidate = grouping::sanitize_name(&format!("{base}-{token}"));
        if !candidate.is_empty() && is_free(root, &candidate, taken) {
            taken.insert(candidate.clone());
            return candidate;
        }
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base} ({suffix})");
        if is_free(root, &candidate, taken) {
            taken.insert(candidate.clone());
            return candidate;
        }
        suffix += 1;
    }
}

/// True when `name` is unused by this run and absent on disk.
fn is_free(root: &Path, name: &str, taken: &BTreeSet<String>) -> bool {
    !taken.contains(name) && !root.join(name).exists()
}

/// Execute a plan as one journal batch (issue #25 machinery).
///
/// Every group is its own [`relocate::MoveBatch`] but they share one batch id,
/// so `undo --batch <id>` reverses the whole run and removes the directories it
/// created once they are empty. One failing group does not stop the others.
pub fn execute(plan: &ClusterPlan, history: &OperationHistory) -> Result<ClusterOutcome> {
    let batch_id = crate::storage::history::new_batch_id();
    let mut groups = Vec::with_capacity(plan.groups.len());
    for group in &plan.groups {
        match relocate::execute_batch_in(&group.batch, history, &batch_id) {
            Ok(outcome) => groups.push(GroupOutcome {
                name: group.name.clone(),
                error: None,
                items: outcome.items,
            }),
            Err(error) => groups.push(GroupOutcome {
                name: group.name.clone(),
                error: Some(error.to_string()),
                items: Vec::new(),
            }),
        }
    }
    Ok(ClusterOutcome { batch_id, groups })
}

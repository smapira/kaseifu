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
//!
//! The group folders are created in the OS standard folder for each kind by
//! default (issue #27): `~/Pictures/images`, `~/Documents/documents`. The
//! scanned directory is not a destination: filing a Desktop folder into
//! subfolders of itself only moves the clutter around. `--to <base>` files
//! everything under one directory, and `--in-place` keeps the #26 layout
//! (`<root>/<name>`) for a folder that is being reorganised in place.

pub mod grouping;
pub mod naming;

use crate::discovery::is_excluded_destination;
use crate::operations::relocate;
use crate::platform::user_dirs::UserDirKind;
use crate::storage::history::OperationHistory;
use anyhow::{Context, Result, anyhow};
use grouping::{Category, Group, Member};
use naming::{GroupNamer, NameFailure, NameSource};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Where the group directories are created (issue #27).
///
/// The scanned directory is never modified; this only decides the *parent* of
/// each group folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationBase {
    /// The OS standard folder for each group's kind (`--to-defaults` is not a
    /// flag: this is the default), and `~/Documents` for a kind the platform
    /// table does not name.
    DefaultDirs,
    /// `<base>/<name>` for every group (`--to`).
    Directory(PathBuf),
    /// `<root>/<name>`: one folder inside the scanned directory (`--in-place`).
    /// The #26 layout, now the opt-in for reorganising a folder in place.
    InPlace,
}

impl DestinationBase {
    /// The word the report and `--format json` use.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultDirs => "default_dirs",
            Self::Directory(_) => "to",
            Self::InPlace => "in_place",
        }
    }

    /// `--to` as an absolute path.
    ///
    /// The report, the journal rows and `undo` have to name the same directory,
    /// and a relative base would be read against whichever directory the run
    /// (or the undo) happens to start in.
    fn resolved(&self) -> Result<Self> {
        match self {
            Self::Directory(base) => Ok(Self::Directory(absolute(base)?)),
            other => Ok(other.clone()),
        }
    }
}

/// The path as an absolute one, without resolving symlinks.
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::path::absolute(path).with_context(|| format!("resolve {}", path.display()))
}

/// How one run is scoped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterOptions {
    /// Members below this count are left in the ungrouped bucket.
    pub min: usize,
    /// No group grows past this many members (large types are split).
    pub max: usize,
    /// Treat the root's direct child directories as members too.
    pub include_folders: bool,
    /// Collect files from subdirectories as well (directories stay put).
    pub recursive: bool,
    /// Group by type only: one group per file kind, ignoring name tokens.
    pub by_type: bool,
    /// Where the group folders go (issue #27).
    pub destination: DestinationBase,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            min: 2,
            max: 20,
            include_folders: false,
            recursive: false,
            by_type: false,
            destination: DestinationBase::DefaultDirs,
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
    /// The parent this group's directory is created under: the scanned
    /// directory itself in the default layout (issue #27).
    pub base: PathBuf,
    /// The validated move of every member into [`Self::directory`] (issue #25).
    pub batch: relocate::MoveBatch,
}

impl PlannedGroup {
    /// The directory this group creates and fills.
    pub fn directory(&self) -> PathBuf {
        self.base.join(&self.name)
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
    /// Where the folders go, resolved (issue #27).
    pub destination: DestinationBase,
    pub groups: Vec<PlannedGroup>,
    pub ungrouped: Vec<Member>,
    /// Members already under the destination base, so the folder they would
    /// be filed into would sit around files that are where they belong. Left
    /// where they are and reported (issue #27).
    pub already_placed: Vec<Member>,
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
    let destination = options.destination.resolved()?;
    if let DestinationBase::Directory(base) = &destination {
        validate_base(base)?;
    }
    let (groups, ungrouped) =
        grouping::group(collected.members, options.min, options.max, options.by_type);
    let prepared = prepare_groups(groups, ungrouped, &destination, root, options.min)?;
    let (named, naming_failure) = names_for(&prepared.groups, namer).await;
    let planned = plan_folders(&prepared, named)?;

    Ok(ClusterPlan {
        root: root.to_path_buf(),
        destination,
        groups: planned,
        ungrouped: prepared.ungrouped,
        already_placed: prepared.already_placed,
        directories: collected.directories,
        skipped: collected.skipped,
        naming_failure,
    })
}

/// The groups a run will file, each with the base it goes to.
struct Prepared {
    /// Groups that keep at least `--min` members, with the surviving members
    /// only.
    groups: Vec<Group>,
    /// The base of each group, in the same order.
    bases: Vec<PathBuf>,
    ungrouped: Vec<Member>,
    already_placed: Vec<Member>,
}

/// Resolve each group's base and leave out the members that are already there.
///
/// A member that already sits under the base has nothing to gain from a folder
/// beside it (issue #27): cluster returns scattered files to where they belong,
/// and these are there already.
fn prepare_groups(
    groups: Vec<Group>,
    mut ungrouped: Vec<Member>,
    destination: &DestinationBase,
    root: &Path,
    min: usize,
) -> Result<Prepared> {
    let mut already_placed = Vec::new();
    let mut kept: Vec<Group> = Vec::with_capacity(groups.len());
    let mut bases: Vec<PathBuf> = Vec::with_capacity(groups.len());
    for group in groups {
        let base = base_for(destination, root, group.category)?;
        let members = filed_members(group.members, destination, &base, &mut already_placed);
        // The threshold is about the folder that would be made, so it is
        // re-applied after the ones already in place have left the group.
        if members.len() < min.max(1) {
            ungrouped.extend(members);
            continue;
        }
        kept.push(Group::from_members(group.category, members));
        bases.push(base);
    }
    Ok(Prepared {
        groups: kept,
        bases,
        ungrouped,
        already_placed,
    })
}

/// The members that still have to move; the ones under the base go to
/// `already_placed`.
fn filed_members(
    members: Vec<Member>,
    destination: &DestinationBase,
    base: &Path,
    already_placed: &mut Vec<Member>,
) -> Vec<Member> {
    // In place, every member is inside the scanned directory by definition, and
    // a folder inside it is the point of the flag.
    if *destination == DestinationBase::InPlace {
        return members;
    }
    members
        .into_iter()
        .filter(|member| {
            let placed = crate::benchmark::is_inside(base, &member.source);
            if placed {
                already_placed.push(member.clone());
            }
            !placed
        })
        .collect()
}

/// Allocate one name and one directory per group, and validate the moves.
fn plan_folders(prepared: &Prepared, named: Vec<naming::NamedGroup>) -> Result<Vec<PlannedGroup>> {
    let mut taken: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut planned = Vec::with_capacity(prepared.groups.len());
    for ((group, named), base) in prepared.groups.iter().zip(named).zip(&prepared.bases) {
        let base_name = if named.name.is_empty() {
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
        // Two groups in different bases cannot collide with each other, so the
        // directory named this run is tracked per base.
        let name = unique_name(
            base,
            &base_name,
            distinguishing,
            taken.entry(base.clone()).or_default(),
        );
        let directory = base.join(&name);
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
            base: base.clone(),
            batch,
        });
    }
    Ok(planned)
}

/// The parent one group's folder is created under (issue #27).
///
/// In place, the base stays exactly as the scanned directory was spelled,
/// because the report renders that layout relative to it. The OS answers keep
/// their own absolute path so the report, the journal and `undo` name one
/// place; a platform that defines no home at all leaves the scanned directory
/// as the only possible parent.
fn base_for(destination: &DestinationBase, root: &Path, category: Category) -> Result<PathBuf> {
    Ok(match destination {
        DestinationBase::InPlace => root.to_path_buf(),
        DestinationBase::Directory(base) => base.clone(),
        DestinationBase::DefaultDirs => match default_dir(category).resolve() {
            Some(base) => absolute(&base)?,
            None => root.to_path_buf(),
        },
    })
}

/// The OS standard folder a group's kind belongs in (issue #27).
///
/// This is [`crate::platform::user_dirs::kind_for_extension`]'s table at the
/// granularity a group knows: `cluster` decides membership by kind, not by
/// extension, and two extensions of one kind (`png`, `jpg`) have to land
/// together. The unit test pins the two tables to the same answer on the kinds
/// both know.
///
/// `~/Documents` answers for everything the table does not name (data, code,
/// folders, no extension): the issue's rule is that the scanned directory is
/// not a destination, so a document folder is a better answer than leaving the
/// clutter where it was.
fn default_dir(category: Category) -> UserDirKind {
    match category {
        Category::Image => UserDirKind::Picture,
        Category::Archive => UserDirKind::Download,
        Category::Video => UserDirKind::Video,
        Category::Audio => UserDirKind::Audio,
        Category::Document
        | Category::Data
        | Category::Code
        | Category::Folder
        | Category::Other => UserDirKind::Document,
    }
}

/// Refuse a `--to` the destination rules already refuse (issue #27).
///
/// The rule is the one every created level gets; stating it here too means the
/// refusal names the flag instead of arriving as one group's failure.
fn validate_base(base: &Path) -> Result<()> {
    if is_excluded_destination(&base.to_string_lossy()) {
        return Err(anyhow!(
            "--to is not a usable destination: {}",
            base.display()
        ));
    }
    if base.exists() && !base.is_dir() {
        return Err(anyhow!("--to is not a directory: {}", base.display()));
    }
    Ok(())
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
    base: &Path,
    name: &str,
    token: Option<&str>,
    taken: &mut BTreeSet<String>,
) -> String {
    let base_name = if name.is_empty() {
        "misc".to_string()
    } else {
        name.to_string()
    };
    if is_free(base, &base_name, taken) {
        taken.insert(base_name.clone());
        return base_name;
    }
    if let Some(token) = token {
        let candidate = grouping::sanitize_name(&format!("{base_name}-{token}"));
        if !candidate.is_empty() && is_free(base, &candidate, taken) {
            taken.insert(candidate.clone());
            return candidate;
        }
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base_name} ({suffix})");
        if is_free(base, &candidate, taken) {
            taken.insert(candidate.clone());
            return candidate;
        }
        suffix += 1;
    }
}

/// True when `name` is unused by this run and absent on disk.
fn is_free(base: &Path, name: &str, taken: &BTreeSet<String>) -> bool {
    !taken.contains(name) && !base.join(name).exists()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_answers_with_the_folder_the_extension_table_names() {
        // One representative extension per kind: the group's folder default has
        // to agree with #18's table, or `organize` and `cluster` would disagree
        // about where the same file belongs.
        for (extension, expected) in [
            ("png", UserDirKind::Picture),
            ("pdf", UserDirKind::Document),
            ("zip", UserDirKind::Download),
            ("mp4", UserDirKind::Video),
            ("mp3", UserDirKind::Audio),
        ] {
            let category = Category::from_extension(Some(extension));
            assert_eq!(default_dir(category), expected, "{extension}");
            assert_eq!(
                crate::platform::user_dirs::kind_for_extension(extension),
                Some(expected),
                "{extension}"
            );
        }
    }

    #[test]
    fn a_kind_the_table_does_not_name_falls_back_to_documents() {
        // The issue picks `~/Documents` for a type it cannot decide (csv, no
        // extension) instead of leaving the file where it was.
        for category in [
            Category::Data,
            Category::Code,
            Category::Folder,
            Category::Other,
        ] {
            assert_eq!(default_dir(category), UserDirKind::Document, "{category:?}");
        }
        assert_eq!(
            crate::platform::user_dirs::kind_for_extension("csv"),
            Some(UserDirKind::Document)
        );
    }

    #[test]
    fn files_are_never_offered_the_desktop() {
        // Files come from the Desktop; a standard folder for a type is never it.
        for extension in ["pdf", "png", "mp4", "mp3", "zip"] {
            assert_ne!(
                default_dir(Category::from_extension(Some(extension))),
                UserDirKind::Desktop,
                "{extension}"
            );
        }
    }

    #[test]
    fn an_absolute_base_is_kept_as_it_is() {
        let base = DestinationBase::Directory(PathBuf::from("/tmp/kaseifu-to"));
        assert_eq!(base.resolved().expect("resolved"), base);
        assert_eq!(
            DestinationBase::InPlace.resolved().expect("resolved"),
            DestinationBase::InPlace
        );
    }
}

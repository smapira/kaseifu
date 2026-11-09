//! `kaseifu cluster` (issue #26): file the flat files of one directory into the
//! folders they belong in.
//!
//! The command exists because `organize` answers a different question (one file
//! into one existing place) and `move` asks the human for the destination.
//!
//! Two modes, decided by `--create-dir` (issue #28):
//!
//! - **Default: create nothing.** A member goes into an existing directory -
//!   the type folder `<kind root>/<type>` when it exists, the kind root itself
//!   otherwise - and the move reuses the journaled batch of issue #25. A
//!   missing kind root is an error that names the path and `--create-dir`.
//! - **`--create-dir`: build the folders.** Membership is decided
//!   deterministically ([`grouping`]), the name is chosen among deterministic
//!   candidates ([`naming`], the only model step), and the folder is created
//!   under the type folder (`<kind root>/<type>/<name>`); a group whose name is
//!   the type name lands flat in `<kind root>/<type>` instead, because a folder
//!   called `images` inside `~/Pictures` says nothing the parent did not.
//!
//! The kind roots are the OS standard folders (issue #27): `~/Pictures`,
//! `~/Documents`, `~/Downloads`, `~/Movies`, `~/Music`, and `~/Documents` for a
//! kind the platform table does not name. `--to <base>` replaces the kind root
//! with one directory, and `--in-place` replaces it with the scanned directory
//! (which needs `--create-dir`, since it exists by definition).
//!
//! Only regular files directly under the root are members by default. Existing
//! directories are discovered, reported and left alone, never moved.

pub mod grouping;
pub mod naming;

use crate::discovery::is_excluded_destination;
use crate::operations::relocate;
use crate::platform::user_dirs::{UserDirKind, expand_home};
use crate::storage::history::OperationHistory;
use anyhow::{Context, Result, anyhow};
use grouping::{Category, Group, Member};
use naming::{GroupNamer, NameFailure, NameSource};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Where the folders come from (issues #27, #28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationBase {
    /// The OS standard folder for each group's kind: `~/Pictures`,
    /// `~/Documents`, and `~/Documents` for a kind the platform table does not
    /// name. The default.
    DefaultDirs,
    /// One directory for every group (`--to`).
    Directory(PathBuf),
    /// The scanned directory (`--in-place`): the #26 layout for reorganising a
    /// folder in place. Requires `--create-dir`.
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
            Self::Directory(base) => Ok(Self::Directory(absolute(&expand_home(base))?)),
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
    /// Where the kind roots come from (issue #27).
    pub destination: DestinationBase,
    /// `--create-dir`: build the named folders (issue #28). Without it a run
    /// only files into directories that already exist.
    pub create_dir: bool,
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
            create_dir: false,
        }
    }
}

/// One destination directory and the members that go into it.
#[derive(Debug, Clone)]
pub struct PlannedGroup {
    /// The classification of this block (`images`, `code`, ...): what the files
    /// are. It picks the kind root and is not a folder of its own. `None` when
    /// nothing classifies them.
    pub kind: Option<String>,
    /// The extension that types this block and names its folder, or `None` when
    /// its files have none.
    pub extension: Option<String>,
    /// The folder this run creates, or `None` when the members go into a
    /// directory that already exists (rules 2-3) or the name is the type name
    /// itself (rule 5).
    pub name: Option<String>,
    pub source: NameSource,
    pub candidates: Vec<String>,
    pub members: Vec<Member>,
    /// The type root this block files under (`~/Pictures`).
    pub base: PathBuf,
    /// The directory the members are filed into.
    pub directory: PathBuf,
    /// The validated move of every member into [`Self::directory`] (issue #25).
    pub batch: relocate::MoveBatch,
}

impl PlannedGroup {
    /// The type folder this block is filed under, without the root (`png`,
    /// `html`), or `unclassified` when nothing types it and the files go flat
    /// into the kind root.
    ///
    /// The type is the extension and nothing else: the kind root already says
    /// what the files are (`~/Pictures` for images), so a level named after the
    /// classification would repeat it (owner decision, 2026-11-09).
    pub fn type_label(&self) -> String {
        self.extension
            .clone()
            .unwrap_or_else(|| "unclassified".to_string())
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
    /// Where the kind roots come from, resolved (issue #27).
    pub destination: DestinationBase,
    /// True when the run was asked to build the folders (`--create-dir`).
    pub creates_directory: bool,
    pub groups: Vec<PlannedGroup>,
    /// Members below `--min` (only a run that creates folders has a threshold).
    pub ungrouped: Vec<Member>,
    /// Members already under their kind root, so filing them would only move
    /// them inside the folder they are in. Left where they are and reported.
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
    /// The folder this group created, or `None` when it filed into an existing
    /// directory.
    pub name: Option<String>,
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

/// Decide the whole run: membership and destinations. No mutation.
pub async fn plan(
    root: &Path,
    options: &ClusterOptions,
    namer: &dyn GroupNamer,
) -> Result<ClusterPlan> {
    // `~/Archive` and `~/Desktop/misc` are what a human types; a quoted one
    // arrives unexpanded, so the shorthand is resolved here rather than left
    // to the shell that did not run.
    let root = expand_home(root);
    let root = root.as_path();
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
    let run = if options.create_dir {
        plan_created(root, options, &destination, collected.members, namer).await?
    } else {
        plan_into_existing(&destination, collected.members)?
    };

    Ok(ClusterPlan {
        root: root.to_path_buf(),
        destination,
        creates_directory: options.create_dir,
        groups: run.groups,
        ungrouped: run.ungrouped,
        already_placed: run.already_placed,
        directories: collected.directories,
        skipped: collected.skipped,
        naming_failure: run.naming_failure,
    })
}

/// The parts of a plan that the two modes decide differently.
struct PlannedRun {
    groups: Vec<PlannedGroup>,
    ungrouped: Vec<Member>,
    already_placed: Vec<Member>,
    naming_failure: Option<String>,
}

/// Rules 2-3 (issue #28): file into directories that already exist.
///
/// Nothing is grouped and nothing is created - a member goes to the deepest
/// folder that already exists in its type chain (`R/<extension>`, then `R`). A
/// single file is worth filing, so `--min` has no say here. A kind root that does
/// not exist stops the run before anything moves, naming every missing one so the
/// caller sees the whole picture rather than one kind at a time.
fn plan_into_existing(destination: &DestinationBase, members: Vec<Member>) -> Result<PlannedRun> {
    let mut buckets: BTreeMap<(PathBuf, Category, Option<String>), Vec<Member>> = BTreeMap::new();
    let mut already_placed = Vec::new();
    let mut missing: BTreeMap<PathBuf, &'static str> = BTreeMap::new();
    for member in members {
        let base = kind_root(destination, member.category)?;
        if crate::benchmark::is_inside(&base, &member.source) {
            already_placed.push(member);
            continue;
        }
        if !base.is_dir() {
            missing.insert(base, default_dir(member.category).label());
            continue;
        }
        let extension = member.extension.clone();
        let directory = existing_destination(&base, extension.as_deref());
        buckets
            .entry((directory, member.category, extension))
            .or_default()
            .push(member);
    }
    if !missing.is_empty() {
        return Err(missing_roots_error(destination, &missing));
    }

    let mut claimed = BTreeSet::new();
    let mut groups = Vec::with_capacity(buckets.len());
    for ((directory, category, extension), members) in buckets {
        let batch = plan_batch_claiming(&members, &directory, false, &mut claimed)?;
        groups.push(PlannedGroup {
            kind: category.class_name().map(str::to_string),
            extension,
            name: None,
            source: NameSource::Fallback,
            candidates: Vec::new(),
            members,
            base: directory.clone(),
            directory,
            batch,
        });
    }
    Ok(PlannedRun {
        groups,
        ungrouped: Vec::new(),
        already_placed,
        naming_failure: None,
    })
}

/// The refusal for kind roots that do not exist (issue #28 acceptance: the run
/// does not create them, so nothing moves and the message asks for
/// `--create-dir`).
fn missing_roots_error(
    destination: &DestinationBase,
    missing: &BTreeMap<PathBuf, &'static str>,
) -> anyhow::Error {
    if let DestinationBase::Directory(base) = destination {
        return anyhow!(
            "--to is not a directory: {} (pass --create-dir to create it)",
            base.display()
        );
    }
    let listed = missing
        .iter()
        .map(|(path, kind)| format!("{kind} ({})", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    let pronoun = if missing.len() == 1 { "it" } else { "them" };
    anyhow!("no standard folder for {listed}; pass --create-dir to create {pronoun}")
}

/// Rules 4-5 (issue #28): group, name, and create the folders.
async fn plan_created(
    root: &Path,
    options: &ClusterOptions,
    destination: &DestinationBase,
    members: Vec<Member>,
    namer: &dyn GroupNamer,
) -> Result<PlannedRun> {
    let (groups, ungrouped) = grouping::group(members, options.min, options.max, options.by_type);
    let prepared = prepare_groups(groups, ungrouped, destination, root, options.min)?;
    let (named, naming_failure) = names_for(&prepared.groups, namer).await;
    let groups = plan_folders(&prepared, named, destination)?;
    Ok(PlannedRun {
        groups,
        ungrouped: prepared.ungrouped,
        already_placed: prepared.already_placed,
        naming_failure,
    })
}

/// The groups a run will file, each with its kind root.
struct Prepared {
    /// Groups that keep at least `--min` members, with the surviving members
    /// only.
    groups: Vec<Group>,
    /// The kind root of each group (R), in the same order.
    bases: Vec<PathBuf>,
    ungrouped: Vec<Member>,
    already_placed: Vec<Member>,
}

/// Resolve each group's kind root and leave out the members that are already
/// there.
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
        let base = create_base(destination, root, group.category)?;
        let members = filed_members(group.members, destination, &base, &mut already_placed);
        // The threshold is about the folder that would be made, so it is
        // re-applied after the ones already in place have left the group.
        if members.len() < min.max(1) {
            ungrouped.extend(members);
            continue;
        }
        kept.push(Group::from_members(
            group.category,
            group.extension,
            members,
        ));
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
fn plan_folders(
    prepared: &Prepared,
    named: Vec<naming::NamedGroup>,
    destination: &DestinationBase,
) -> Result<Vec<PlannedGroup>> {
    let in_place = *destination == DestinationBase::InPlace;
    let mut taken: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    let mut planned = Vec::with_capacity(prepared.groups.len());
    for ((group, named), base) in prepared.groups.iter().zip(named).zip(&prepared.bases) {
        // The type chain is the extension folder under the kind root (`R/png`); a
        // group nothing types has no chain at all and goes into the root.
        let type_folder = type_folder(group, base);
        let type_word = group.type_name();
        // In place there is no type root to explain the kind, so a semantic
        // name is created directly under the scanned directory (#26); the type
        // folder is only used when the name carries nothing more than the type.
        let parent = match (&type_folder, in_place) {
            (Some(folder), false) => folder.clone(),
            _ => base.clone(),
        };
        // Two groups in different parents cannot collide with each other, so the
        // names taken this run are tracked per parent.
        let name = chosen_name(group, &named).map(|name| {
            unique_name(
                &parent,
                &name,
                distinguishing_token(group, &named),
                taken.entry(parent.clone()).or_default(),
            )
        });
        // Rule 5: a folder named after the type says nothing the type root does
        // not, so the members land flat in the type folder instead. In place
        // there is no type root, so the folder is the grouping and is always
        // created. A group with no name at all lands flat in its kind root.
        let folder =
            name.filter(|name| in_place || type_word.is_none_or(|word| !is_type_name(name, word)));
        let directory = match &folder {
            Some(name) => parent.join(name),
            None => type_folder.clone().unwrap_or_else(|| base.clone()),
        };
        let batch = plan_batch_claiming(&group.members, &directory, true, &mut claimed)?;
        planned.push(PlannedGroup {
            kind: group.class_name().map(str::to_string),
            extension: group.extension.clone(),
            name: folder,
            source: named.source,
            candidates: group.candidate_names(),
            members: group.members.clone(),
            base: base.clone(),
            directory,
            batch,
        });
    }
    Ok(planned)
}

/// The type folder a group's files belong in: the extension under the kind root
/// (`R/png`, `R/html`). `None` when nothing types the group, which files it flat
/// in the kind root.
fn type_folder(group: &Group, base: &Path) -> Option<PathBuf> {
    Some(base.join(group.extension.as_deref()?))
}

/// The name a group was given: the namer's, or the group's own first candidate
/// (the type name) when the namer had nothing to say. `None` when neither
/// offers one, which leaves the group with no folder of its own.
fn chosen_name(group: &Group, named: &naming::NamedGroup) -> Option<String> {
    if !named.name.is_empty() {
        return Some(named.name.clone());
    }
    group.candidate_names().first().cloned()
}

/// The token a fallback name may be disambiguated with (`images-screenshot`).
fn distinguishing_token<'a>(group: &'a Group, named: &naming::NamedGroup) -> Option<&'a str> {
    if named.source != NameSource::Fallback {
        return None;
    }
    group.shared_tokens.first().map(String::as_str)
}

/// True when a name carries no more information than the type folder it would
/// be created in: the type name itself, or the type name with a collision
/// number (`videos (2)`), which is what a group with no shared token gets.
fn is_type_name(name: &str, type_folder: &str) -> bool {
    let Some(rest) = name.strip_prefix(type_folder) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    rest.strip_prefix(" (")
        .and_then(|tail| tail.strip_suffix(')'))
        .is_some_and(|number| {
            !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit())
        })
}

/// The kind root one group files under (issue #27/#28).
///
/// In place, the kind root is the scanned directory itself; the OS answers keep
/// their own absolute path so the report, the journal and `undo` name one
/// place.
fn create_base(destination: &DestinationBase, root: &Path, category: Category) -> Result<PathBuf> {
    Ok(match destination {
        DestinationBase::InPlace => absolute(root)?,
        DestinationBase::Directory(base) => base.clone(),
        DestinationBase::DefaultDirs => match default_dir(category).resolve() {
            Some(base) => absolute(&base)?,
            None => root.to_path_buf(),
        },
    })
}

/// The kind root for a run that creates nothing.
fn kind_root(destination: &DestinationBase, category: Category) -> Result<PathBuf> {
    match destination {
        DestinationBase::InPlace => Err(anyhow!(
            "--in-place only works with --create-dir: the scanned directory already exists"
        )),
        DestinationBase::Directory(base) => Ok(base.clone()),
        DestinationBase::DefaultDirs => match default_dir(category).resolve() {
            Some(path) => absolute(&path),
            None => Err(anyhow!(
                "no standard folder for {} on this platform; pass --create-dir or --to <base>",
                default_dir(category).label()
            )),
        },
    }
}

/// Rule 2-3: the extension folder that already exists, or the kind root.
fn existing_destination(base: &Path, extension: Option<&str>) -> PathBuf {
    let Some(extension) = extension else {
        // Nothing types these files, so there is no folder to walk into: the
        // kind root is the destination, and the file goes flat into it.
        return base.to_path_buf();
    };
    let extension_folder = base.join(extension);
    if extension_folder.is_dir() {
        extension_folder
    } else {
        base.to_path_buf()
    }
}

/// Plan one directory's batch, refusing a destination another group already
/// claimed.
///
/// `plan_batch` catches two members of one directory that share a name; two
/// blocks of the same kind root (`documents` and `data` both under
/// `~/Documents`) could otherwise plan the same destination twice and let the
/// second move fail halfway through the batch.
fn plan_batch_claiming(
    members: &[Member],
    directory: &Path,
    create: bool,
    claimed: &mut BTreeSet<PathBuf>,
) -> Result<relocate::MoveBatch> {
    let sources: Vec<PathBuf> = members.iter().map(|member| member.source.clone()).collect();
    let mut batch = relocate::plan_batch(&sources, directory, create)
        .map_err(|error| anyhow!("cannot file into {}: {error}", directory.display()))?;
    for item in &mut batch.items {
        let Ok(plan) = &item.plan else {
            continue;
        };
        let destination = plan.destination_path.clone();
        if !claimed.insert(destination.clone()) {
            item.plan = Err(relocate::RelocateError::Collision(destination));
        }
    }
    Ok(batch)
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
            ("xlsx", UserDirKind::Document),
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

    #[test]
    fn a_name_that_only_repeats_the_type_is_recognised() {
        assert!(is_type_name("images", "images"));
        assert!(is_type_name("videos (2)", "videos"));
        assert!(!is_type_name("at-screenshot", "images"));
        assert!(!is_type_name("images-old", "images"));
        assert!(!is_type_name("images (x)", "images"));
        assert!(!is_type_name("image", "images"));
    }

    /// A namer that always answers with the same word, so the folder layout can
    /// be tested without an HTTP stub.
    struct FixedName(&'static str);

    #[async_trait::async_trait]
    impl GroupNamer for FixedName {
        async fn name_groups(
            &self,
            groups: &[Group],
        ) -> Result<Vec<naming::NamedGroup>, NameFailure> {
            Ok(groups
                .iter()
                .map(|_| naming::NamedGroup {
                    name: self.0.to_string(),
                    source: NameSource::Jev,
                })
                .collect())
        }
    }

    /// A scanned folder with two images the deterministic grouping keeps
    /// together, and a destination base the test owns.
    fn creation_sandbox(tag: &str) -> (PathBuf, PathBuf) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-sandboxes")
            .join(format!("cluster-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("misc");
        std::fs::create_dir_all(&dir).expect("sandbox");
        std::fs::write(dir.join("cbd-chart-1.png"), b"x").expect("file");
        std::fs::write(dir.join("cbd-chart-2.png"), b"x").expect("file");
        (root, dir)
    }

    #[tokio::test]
    async fn a_semantic_name_is_created_under_the_type_folder() {
        let (root, dir) = creation_sandbox("semantic");
        let base = root.join("Pictures");
        let options = ClusterOptions {
            destination: DestinationBase::Directory(base.clone()),
            create_dir: true,
            ..ClusterOptions::default()
        };

        let plan = plan(&dir, &options, &FixedName("at-screenshot"))
            .await
            .expect("plan");
        let group = &plan.groups[0];
        assert_eq!(group.name.as_deref(), Some("at-screenshot"));
        // The type folder is the extension under the kind root (issue #28).
        assert_eq!(group.kind.as_deref(), Some("images"));
        assert_eq!(group.extension.as_deref(), Some("png"));
        assert_eq!(group.directory, base.join("png/at-screenshot"));
        assert_eq!(
            group.batch.missing_directories,
            vec![
                base.clone(),
                base.join("png"),
                base.join("png/at-screenshot")
            ],
            "the kind root, the extension and the named folder"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_type_name_is_filed_flat_in_the_type_folder() {
        let (root, dir) = creation_sandbox("type-name");
        let base = root.join("Movies");
        let options = ClusterOptions {
            destination: DestinationBase::Directory(base.clone()),
            create_dir: true,
            ..ClusterOptions::default()
        };

        // `png` is the type name here, so nothing is created under it.
        let plan = plan(&dir, &options, &FixedName("png")).await.expect("plan");
        let group = &plan.groups[0];
        assert_eq!(
            group.name, None,
            "a folder named after the type is not created"
        );
        assert_eq!(group.directory, base.join("png"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn the_default_files_into_the_existing_type_folder_without_creating() {
        let (root, dir) = creation_sandbox("existing");
        let base = root.join("Pictures");
        std::fs::create_dir_all(base.join("png")).expect("Pictures/png");
        let options = ClusterOptions {
            destination: DestinationBase::Directory(base.clone()),
            ..ClusterOptions::default()
        };

        let plan = plan(&dir, &options, &FixedName("never-asked"))
            .await
            .expect("plan");
        let group = &plan.groups[0];
        assert_eq!(group.name, None);
        assert_eq!(group.directory, base.join("png"));
        assert!(group.batch.missing_directories.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}

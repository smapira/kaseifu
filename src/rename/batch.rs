//! Batch rename: a bounded walk, conflict detection and an apply order
//! (original spec sections 8.5, 11.2 and 14).
//!
//! `kaseifu rename <folder> --recursive` renames what is inside a folder
//! instead of the folder itself. Two properties separate a batch from running
//! the command once per target, and both live here:
//!
//! - **A conflict is a property of the set.** The single-target planner already
//!   refuses a proposal whose destination exists, but it cannot see that two
//!   siblings propose the same name. [`hold_conflicts`] holds every such group
//!   before anything is applied, so a batch never half-writes a name.
//! - **The apply order is part of the answer.** Renaming a folder invalidates
//!   the recorded path of everything inside it, so a child is renamed before
//!   its parent ([`execution_order`]). Undo reverses the newest row first, so it
//!   restores the parent before the child, exactly as the single-target round
//!   established (original spec section 10).
//!
//! The walk is bounded by depth and by a target cap (original spec section 5.4:
//! recursion is never unbounded; section 11.2 asks for bounded work and for
//! leaving a target out early).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::context::directory::{
    is_bundle_container, is_cache_or_temp_name, is_hidden_name, is_library_tree, is_system_area,
};
use crate::models::file_context::EntryKind;
use crate::normalize::noise::is_noise_file;
use crate::platform::macos::filesystem;
use crate::semantic::provider::SemanticProvider;
use crate::storage::history::OperationHistory;

use super::service::{PlanOutcome, RenameOptions, RenamePlan, execute_plan_in_batch, plan_rename};
use super::store::RenameStore;

/// How deep the walk descends below the folder it was given.
///
/// The folder's own children are depth 1. Three levels cover the shape the
/// original spec section 8.5 shows (a folder beside the files it holds, plus
/// the folders those hold) while keeping a deep tree from running away.
pub const MAX_BATCH_DEPTH: usize = 3;

/// How many targets one batch covers.
///
/// Every proposal costs one on-device generation, so the cap is also the
/// worst-case cost bound (original spec section 11.2).
pub const MAX_BATCH_TARGETS: usize = 50;

/// How many entries are read from one directory while walking.
const MAX_ENTRIES_PER_DIRECTORY: usize = 2_000;

/// What the bounded walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchWalk {
    /// The targets, in the order the walk visited them.
    pub targets: Vec<PathBuf>,
    /// True when the tree held more targets than the walk could take.
    pub truncated: bool,
}

/// Walk one folder and collect the targets a batch covers.
///
/// Depth-first, names in sorted order, at most [`MAX_BATCH_DEPTH`] levels and at
/// most [`MAX_BATCH_TARGETS`] targets. A folder that is itself a target is still
/// descended into: children are named too, and [`execution_order`] keeps that
/// safe, except for a cache or temporary folder, which is listed but never
/// descended into. Hidden names, macOS noise names, app bundles, the macOS
/// library trees and OS areas are left out, and a symbolic link is never
/// followed or renamed (original spec sections 5.4, 9.1 and 11.2).
///
/// An unreadable directory is an error (the caller reports it and renames
/// nothing), never a silently missing subtree.
pub fn walk(root: &Path) -> anyhow::Result<BatchWalk> {
    let mut targets = Vec::new();
    let mut truncated = false;
    walk_level(root, 1, &mut targets, &mut truncated)?;
    Ok(BatchWalk { targets, truncated })
}

/// Collect one level, then descend (depth first, sorted names).
fn walk_level(
    dir: &Path,
    depth: usize,
    targets: &mut Vec<PathBuf>,
    truncated: &mut bool,
) -> anyhow::Result<()> {
    if depth > MAX_BATCH_DEPTH {
        return Ok(());
    }
    let mut entries = filesystem::read_directory(dir, MAX_ENTRIES_PER_DIRECTORY)?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    for entry in entries {
        if !is_batch_target(&entry.name) {
            continue;
        }
        if targets.len() >= MAX_BATCH_TARGETS {
            *truncated = true;
            return Ok(());
        }
        let path = dir.join(&entry.name);
        match entry.kind {
            EntryKind::File => targets.push(path),
            EntryKind::Directory => {
                targets.push(path.clone());
                // A cache or temporary folder is listed so the plan holds it
                // with its reason instead of dropping it silently, but the
                // walk never descends into it.
                if !is_cache_or_temp_name(&entry.name) {
                    walk_level(&path, depth + 1, targets, truncated)?;
                }
            }
            // A link is not followed and not renamed, and anything else (a
            // socket, a device node) is not a rename target at all.
            EntryKind::Symlink | EntryKind::Other => {}
        }
    }
    Ok(())
}

/// True for a name a batch may touch.
///
/// The rules are the folder-refusal shapes of the context layer, applied to one
/// child name: they already describe what macOS or an app owns. Because a
/// skipped folder is never descended into, matching one name covers its whole
/// subtree. Cache and temporary names stay listed on purpose: the plan holds
/// them with their reason instead of dropping them silently (the walk still
/// never descends into them).
fn is_batch_target(name: &str) -> bool {
    let single = [name];
    !is_noise_file(name)
        && !is_hidden_name(name)
        && !is_bundle_container(name)
        && !is_library_tree(&single)
        && !is_system_area(&single)
}

/// One planned target in a batch.
#[derive(Debug, Clone)]
pub struct BatchItem {
    pub target: PathBuf,
    pub current_name: String,
    pub outcome: BatchOutcome,
    /// What applying this item did; `None` until a batch applies it.
    pub applied: Option<Applied>,
}

impl BatchItem {
    /// One item, planned but not applied.
    pub fn new(target: PathBuf, current_name: String, outcome: BatchOutcome) -> Self {
        Self {
            target,
            current_name,
            outcome,
            applied: None,
        }
    }

    /// The ready plan, when this item proposed one.
    pub fn plan(&self) -> Option<&RenamePlan> {
        match &self.outcome {
            BatchOutcome::Propose(plan) => Some(plan),
            _ => None,
        }
    }
}

/// What planning one target decided.
#[derive(Debug, Clone)]
pub enum BatchOutcome {
    /// Ready to apply.
    Propose(Box<RenamePlan>),
    /// Two items in this batch would write the same name.
    Conflict { reason: String },
    /// The single-target rules held it (the reason says why).
    Held { reason: String },
    /// The proposal is the current name.
    Noop,
}

/// What applying one proposal did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// The rename happened; the string is the journal operation id.
    Executed(String),
    /// The rename was refused at apply time; nothing moved.
    Failed(String),
}

/// What the walk planned, before and after applying it.
#[derive(Debug, Clone)]
pub struct BatchReport {
    pub root: PathBuf,
    pub items: Vec<BatchItem>,
    pub truncated: bool,
    /// The id stamp of the last [`apply`] (undo reverses exactly these rows).
    ///
    /// `None` until the batch is applied: previews plan without an identity.
    pub batch_id: Option<String>,
}

/// One batch's identity for undo (`batch-` + process id + nanos).
fn new_batch_id() -> String {
    format!(
        "batch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|age| age.as_nanos())
            .unwrap_or_default()
    )
}

impl BatchReport {
    /// How many items proposed a name.
    pub fn proposals(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.plan().is_some())
            .count()
    }

    /// How many items a batch conflict held.
    pub fn conflicts(&self) -> usize {
        self.count(|outcome| matches!(outcome, BatchOutcome::Conflict { .. }))
    }

    /// How many items the single-target rules held.
    pub fn held(&self) -> usize {
        self.count(|outcome| matches!(outcome, BatchOutcome::Held { .. }))
    }

    /// How many items already carried the proposed name.
    pub fn noop(&self) -> usize {
        self.count(|outcome| matches!(outcome, BatchOutcome::Noop))
    }

    /// How many renames actually happened.
    pub fn recorded(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item.applied, Some(Applied::Executed(_))))
            .count()
    }

    fn count(&self, keep: impl Fn(&BatchOutcome) -> bool) -> usize {
        self.items.iter().filter(|item| keep(&item.outcome)).count()
    }
}

/// Hold every group of items that would write the same file name.
///
/// Names are compared inside one directory, case-insensitively and
/// NFC-normalized, because the default macOS volume folds all three:
/// `Invoice.pdf` and `invoice.pdf` are one file, and so are NFC `\u{e9}`
/// and NFD `e\u{301}` spellings of the same name. A normalization-only
/// difference is held here (not merely at apply time) so the preview names
/// the conflict instead of showing a proposal that later fails (original
/// spec sections 9.1 and 11.2).
pub fn hold_conflicts(items: &mut [BatchItem]) {
    let groups = destination_groups(items);
    for members in groups.values() {
        if members.len() > 1 {
            hold_group(items, members);
        }
    }
}

/// The index of every ready item, keyed by the path it would write.
fn destination_groups(items: &[BatchItem]) -> BTreeMap<(PathBuf, String), Vec<usize>> {
    let mut groups: BTreeMap<(PathBuf, String), Vec<usize>> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        let Some(plan) = item.plan() else {
            continue;
        };
        groups
            .entry((
                directory_of(&plan.destination),
                folded_name(&plan.destination),
            ))
            .or_default()
            .push(index);
    }
    groups
}

/// Replace one conflicting group with a held outcome.
fn hold_group(items: &mut [BatchItem], members: &[usize]) {
    let mut names: Vec<String> = members
        .iter()
        .filter_map(|index| items[*index].plan().map(|plan| plan.proposal.clone()))
        .collect();
    // Two items can propose the same name with different spelling (the volume
    // folds case), so the list is sorted and de-duplicated before it is read.
    names.sort();
    names.dedup();
    let reason = format!(
        "two or more items propose the same name ({}); all are held",
        names.join(", ")
    );
    for index in members {
        items[*index].outcome = BatchOutcome::Conflict {
            reason: reason.clone(),
        };
    }
}

/// The directory a destination lives in (the empty path when it has none).
fn directory_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The file name a destination would take, folded the way the volume folds it.
///
/// NFC first (so precomposed and decomposed spellings meet), then lowercase
/// (the volume is case-insensitive too).
fn folded_name(path: &Path) -> String {
    use unicode_normalization::UnicodeNormalization;
    path.file_name()
        .map(|name| {
            name.to_string_lossy()
                .nfc()
                .collect::<String>()
                .to_lowercase()
        })
        .unwrap_or_default()
}

/// The order a batch applies its proposals in: deepest first.
///
/// A folder rename invalidates the recorded path of everything inside it, so a
/// child is renamed before its parent; ties keep the walk's own order. Undo is
/// newest-first, so it reverses the parent before the child and each recorded
/// path exists again by the time its row is reached (original spec section 10).
pub fn execution_order(items: &[BatchItem]) -> Vec<usize> {
    let mut ready: Vec<usize> = (0..items.len())
        .filter(|index| items[*index].plan().is_some())
        .collect();
    ready.sort_by(|left, right| {
        path_depth(&items[*right].target)
            .cmp(&path_depth(&items[*left].target))
            .then_with(|| items[*left].target.cmp(&items[*right].target))
    });
    ready
}

/// How many path components a target has (a depth proxy for ordering).
fn path_depth(path: &Path) -> usize {
    path.components().count()
}

/// Plan one batch: walk, plan every target, then resolve the conflicts.
///
/// A target the single-target rules hold or refuse becomes a held item rather
/// than failing the run (original spec section 12): only a folder that cannot
/// be read at all is an error, and that already stops [`walk`].
pub async fn plan(
    provider: &impl SemanticProvider,
    store: &RenameStore,
    walk: &BatchWalk,
    root: &Path,
    options: &RenameOptions,
) -> BatchReport {
    let mut items = Vec::new();
    for target in &walk.targets {
        items.push(plan_item(provider, store, target, options).await);
    }
    hold_conflicts(&mut items);
    BatchReport {
        root: root.to_path_buf(),
        items,
        truncated: walk.truncated,
        batch_id: None,
    }
}

/// Plan one target, turning a refusal into a held item.
async fn plan_item(
    provider: &impl SemanticProvider,
    store: &RenameStore,
    target: &Path,
    options: &RenameOptions,
) -> BatchItem {
    let outcome = match plan_rename(provider, store, target, options).await {
        Ok(PlanOutcome::Ready(plan)) => BatchOutcome::Propose(plan),
        Ok(PlanOutcome::Noop { .. }) => BatchOutcome::Noop,
        Ok(PlanOutcome::Held { reason, .. }) => BatchOutcome::Held { reason },
        Err(error) => BatchOutcome::Held {
            reason: error.to_string(),
        },
    };
    BatchItem::new(target.to_path_buf(), current_name_of(target), outcome)
}

/// The name a target carries now (its last path component).
fn current_name_of(target: &Path) -> String {
    target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.display().to_string())
}

/// Apply every ready proposal, deepest first, recording what each one did.
///
/// One item failing never stops the rest (original spec section 12): the
/// failure is kept on that item, and it is one journal row per applied rename,
/// so `undo` can reverse the batch in the order it happened. Every row of one
/// apply shares the report's batch id, so `undo --batch` reverses exactly
/// this apply even when single renames happened in between.
pub fn apply(report: &mut BatchReport, history: &OperationHistory) {
    let batch_id = new_batch_id();
    report.batch_id = Some(batch_id.clone());
    for index in execution_order(&report.items) {
        let Some(plan) = report.items[index].plan().cloned() else {
            continue;
        };
        let applied = match execute_plan_in_batch(&plan, history, Some(&batch_id)) {
            Ok(operation_id) => Applied::Executed(operation_id),
            Err(error) => Applied::Failed(error.to_string()),
        };
        report.items[index].applied = Some(applied);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn work_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kaseifu-batch-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").expect("child file");
    }

    #[test]
    fn the_walk_is_bounded_by_depth_and_sort_order() {
        let dir = work_dir("depth");
        fs::create_dir_all(dir.join("level2/level3/level4/level5")).expect("nested");
        write(&dir, "b.txt");
        write(&dir, "a.txt");
        let walk = walk(&dir).expect("walk");
        let names: Vec<String> = walk
            .targets
            .iter()
            .map(|path| {
                path.strip_prefix(&dir)
                    .expect("inside")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "a.txt",
                "b.txt",
                "level2",
                "level2/level3",
                "level2/level3/level4"
            ],
            "sorted, and never deeper than {MAX_BATCH_DEPTH} levels"
        );
        assert!(!walk.truncated);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_walk_skips_what_kaseifu_must_not_touch() {
        let dir = work_dir("skip");
        write(&dir, ".DS_Store");
        write(&dir, "keep.txt");
        fs::create_dir_all(dir.join(".hidden")).expect("hidden");
        fs::create_dir_all(dir.join("App.app")).expect("bundle");
        fs::create_dir_all(dir.join("Caches")).expect("cache");
        fs::create_dir_all(dir.join("Library")).expect("library");
        std::os::unix::fs::symlink("keep.txt", dir.join("link.txt")).expect("symlink");
        let walk = walk(&dir).expect("walk");
        // A cache folder stays listed so the plan holds it with its reason
        // instead of dropping it silently; everything else stays out.
        assert_eq!(
            walk.targets,
            vec![dir.join("Caches"), dir.join("keep.txt")],
            "only the plain file and the cache folder stay"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_walk_lists_but_never_descends_into_a_cache_folder() {
        let dir = work_dir("cachedesc");
        fs::create_dir_all(dir.join("Caches")).expect("cache");
        write(&dir.join("Caches"), "inner.txt");
        fs::create_dir_all(dir.join("Caches/nested")).expect("nested");
        write(&dir, "keep.txt");
        let walk = walk(&dir).expect("walk");
        assert_eq!(
            walk.targets,
            vec![dir.join("Caches"), dir.join("keep.txt")],
            "the cache folder is listed, its contents are not"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ordinary_names_that_end_like_noise_stay_listed() {
        let dir = work_dir("names");
        fs::create_dir_all(dir.join("my-target")).expect("target-like");
        write(&dir.join("my-target"), "c.txt");
        write(&dir, "dist-target.txt");
        let walk = walk(&dir).expect("walk");
        assert!(
            walk.targets.contains(&dir.join("dist-target.txt")),
            "a file ending in -target stays: {:?}",
            walk.targets
        );
        assert!(
            walk.targets.contains(&dir.join("my-target")),
            "a folder ending in -target stays: {:?}",
            walk.targets
        );
        assert!(
            walk.targets.contains(&dir.join("my-target/c.txt")),
            "its contents stay too: {:?}",
            walk.targets
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_walk_reports_the_target_cap() {
        let dir = work_dir("cap");
        for index in 0..(MAX_BATCH_TARGETS + 5) {
            write(&dir, &format!("f{index:03}.txt"));
        }
        let walk = walk(&dir).expect("walk");
        assert_eq!(walk.targets.len(), MAX_BATCH_TARGETS);
        assert!(walk.truncated, "a tree over the cap must say so");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_folder_is_an_error() {
        let dir = work_dir("missing");
        assert!(walk(&dir.join("nope")).is_err());
    }

    fn item(name: &str, proposal: &str, directory: &str) -> BatchItem {
        let source = PathBuf::from(directory).join(name);
        let plan = RenamePlan {
            source: source.clone(),
            destination: PathBuf::from(directory).join(proposal),
            current_name: name.to_string(),
            proposal: proposal.to_string(),
            rule_label: "P02".to_string(),
            date_source: "modified",
            snapshot: super::super::service::SourceSnapshot::File(
                super::super::validate::FileSnapshot {
                    size: 1,
                    modified: None,
                },
            ),
            perception: None,
            video: None,
            directory: None,
        };
        BatchItem::new(
            source,
            name.to_string(),
            BatchOutcome::Propose(Box::new(plan)),
        )
    }

    fn held(name: &str) -> BatchItem {
        BatchItem::new(
            PathBuf::from("/tmp").join(name),
            name.to_string(),
            BatchOutcome::Held {
                reason: "held".to_string(),
            },
        )
    }

    #[test]
    fn two_items_that_would_write_one_name_are_both_held() {
        let mut items = vec![
            item("a.txt", "same.txt", "/tmp/x"),
            item("b.txt", "same.txt", "/tmp/x"),
            item("c.txt", "other.txt", "/tmp/x"),
            item("d.txt", "Same.TXT", "/tmp/x"),
            held("e.txt"),
        ];
        hold_conflicts(&mut items);
        for index in [0, 1, 3] {
            assert!(
                matches!(items[index].outcome, BatchOutcome::Conflict { .. }),
                "index {index} must be held: {:?}",
                items[index].outcome
            );
        }
        assert!(items[2].plan().is_some(), "an unshared name stays ready");
        assert!(matches!(items[4].outcome, BatchOutcome::Held { .. }));
    }

    #[test]
    fn normalization_only_differences_are_conflicts() {
        // NFC cafe\u{e9}.pdf vs NFD cafe\u{301}.pdf: one file on macOS.
        let mut items = vec![
            item("a.txt", "caf\u{e9}.pdf", "/tmp/x"),
            item("b.txt", "cafe\u{301}.pdf", "/tmp/x"),
            item("c.txt", "other.pdf", "/tmp/x"),
        ];
        hold_conflicts(&mut items);
        for index in [0, 1] {
            assert!(
                matches!(items[index].outcome, BatchOutcome::Conflict { .. }),
                "index {index} must be held: {:?}",
                items[index].outcome
            );
        }
        assert!(items[2].plan().is_some(), "an unshared name stays ready");
    }

    #[test]
    fn the_same_name_in_two_directories_is_not_a_conflict() {
        let mut items = vec![
            item("a.txt", "same.txt", "/tmp/x"),
            item("b.txt", "same.txt", "/tmp/y"),
        ];
        hold_conflicts(&mut items);
        assert!(items.iter().all(|item| item.plan().is_some()));
    }

    #[test]
    fn a_child_is_applied_before_its_parent() {
        let items = vec![
            item("outer.txt", "outer-new.txt", "/tmp/x"),
            item("d", "d-new", "/tmp/x"),
            item("inner.txt", "inner-new.txt", "/tmp/x/d"),
            held("held.txt"),
        ];
        let order = execution_order(&items);
        assert_eq!(order, vec![2, 1, 0], "deepest first, ties by path");
    }

    #[test]
    fn the_counts_are_the_ones_the_report_shows() {
        let mut items = vec![
            item("a.txt", "same.txt", "/tmp/x"),
            item("b.txt", "same.txt", "/tmp/x"),
            held("c.txt"),
            BatchItem::new(
                PathBuf::from("/tmp/x/d.txt"),
                "d.txt".to_string(),
                BatchOutcome::Noop,
            ),
        ];
        hold_conflicts(&mut items);
        let report = BatchReport {
            root: PathBuf::from("/tmp/x"),
            items,
            truncated: true,
            batch_id: None,
        };
        assert_eq!(report.proposals(), 0);
        assert_eq!(report.conflicts(), 2);
        assert_eq!(report.held(), 1);
        assert_eq!(report.noop(), 1);
        assert_eq!(report.recorded(), 0);
    }
}

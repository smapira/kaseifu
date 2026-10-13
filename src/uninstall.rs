//! `kaseifu uninstall`: remove what kaseifu installed on this machine, and
//! only that (ZC-11).
//!
//! [`plan`] only reads (filesystem metadata plus a read-only journal count)
//! and never receives a `SecretStore`, so a dry run cannot reach the
//! Keychain; [`execute`] is the only function that deletes anything, and it
//! reaches the Keychain exclusively through the injected store. `env` mode
//! (`KASEIFU_KEY_SOURCE=env`) is gated twice: the plan forces the Keychain
//! target to `Keep`, and `execute` checks `keychain_allowed` before any
//! call. The user's files that kaseifu moved are out of scope by design.
//!
//! This module carries data only; every user-facing string is built by the
//! presentation layer.

use crate::auth::keychain::{AuthError, SecretStore};
use crate::completion;
use crate::config::key_source::KeySourceMode;
use crate::storage::history;
use std::path::{Path, PathBuf};

/// Flags deciding what the plan treats as a removal target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UninstallOptions {
    /// Delete the targets (otherwise the plan stays a dry run).
    pub execute: bool,
    /// Also delete the user configuration and the journal.
    pub purge: bool,
    /// Also delete the API key from the macOS Keychain.
    pub remove_key: bool,
}

/// Resolved locations the plan inspects. Tests pass temporary paths here so
/// the developer's real configuration, journal and completion directories
/// are never involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallPaths {
    /// Completion directories (an explicit override or the detected candidates).
    pub completion_dirs: Vec<PathBuf>,
    /// The user configuration (`~/Library/Application Support/kaseifu/config.toml`).
    pub config: PathBuf,
    /// The journal (`~/Library/Application Support/kaseifu/kaseifu.db`).
    pub journal: PathBuf,
    /// The old CWD-relative `data/kaseifu.db`. Only this one is ever removed;
    /// journals in other directories are never searched for or touched.
    pub legacy_journal: PathBuf,
}

/// What kind of thing a target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Completion,
    Config,
    Journal,
    LegacyJournal,
    Keychain,
    Binary,
    MovedFiles,
}

/// What the options ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetAction {
    Remove,
    Keep,
}

/// What happened (or would happen) to one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    /// Dry run: the target would be deleted.
    WouldRemove,
    /// Deleted in this run.
    Removed,
    /// Kept (not requested, not allowed, or not a regular file).
    Kept,
    /// Nothing there to remove.
    Absent,
    /// The Keychain confirmation was declined.
    Cancelled,
    /// The removal failed (the cause is in [`UninstallEntry::error`]).
    Failed,
}

/// One target row of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallEntry {
    pub kind: TargetKind,
    pub path: Option<PathBuf>,
    pub action: TargetAction,
    pub status: EntryStatus,
    pub size_bytes: Option<u64>,
    /// Cause of a failure, or why a requested removal was refused for safety.
    pub error: Option<String>,
    /// Recorded operations of a legacy journal, when it could be read.
    pub legacy_operations: Option<u64>,
    /// True when the file is a symbolic link the plan refuses to remove
    /// (a user-managed link, matching the automatic refresh's rule).
    pub kept_symlink: bool,
}

/// The full uninstall plan (options plus one row per target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallPlan {
    pub options: UninstallOptions,
    pub entries: Vec<UninstallEntry>,
}

/// Build the plan. Reads the filesystem only: nothing is deleted, no
/// directory is created, and the Keychain is not consulted.
///
/// Execution order is the row order: completions first, then (requested)
/// configuration and journals, then the Keychain last (ZC-11).
///
/// ```text
/// completion dirs -> config -> journal -> legacy journal -> keychain
///                 -> binary (kept) -> moved files (out of scope)
/// ```
pub fn plan(
    options: &UninstallOptions,
    paths: &UninstallPaths,
    key_mode: KeySourceMode,
) -> UninstallPlan {
    let mut entries = completion_entries(paths);
    entries.push(file_entry(TargetKind::Config, &paths.config, options.purge));
    entries.push(file_entry(
        TargetKind::Journal,
        &paths.journal,
        options.purge,
    ));
    entries.push(legacy_journal_entry(&paths.legacy_journal, options.purge));
    entries.push(keychain_entry(options, key_mode));
    entries.push(informational_entry(
        TargetKind::Binary,
        std::env::current_exe().ok(),
    ));
    entries.push(informational_entry(TargetKind::MovedFiles, None));
    UninstallPlan {
        options: *options,
        entries,
    }
}

/// Every `_kaseifu` file in the detected completion directories.
fn completion_entries(paths: &UninstallPaths) -> Vec<UninstallEntry> {
    let mut entries = Vec::new();
    for dir in &paths.completion_dirs {
        let candidate = dir.join(completion::FILE_NAME);
        let Ok(metadata) = std::fs::symlink_metadata(&candidate) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            entries.push(UninstallEntry {
                kind: TargetKind::Completion,
                path: Some(candidate),
                action: TargetAction::Keep,
                status: EntryStatus::Kept,
                size_bytes: None,
                error: None,
                legacy_operations: None,
                kept_symlink: true,
            });
        } else if metadata.is_file() {
            entries.push(removal_candidate(
                TargetKind::Completion,
                candidate,
                Some(metadata.len()),
                None,
            ));
        }
    }
    if entries.is_empty() {
        entries.push(absent_entry(TargetKind::Completion, None));
    }
    entries
}

/// The user configuration / journal row (regular files only).
fn file_entry(kind: TargetKind, path: &Path, purge: bool) -> UninstallEntry {
    let size = std::fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len());
    match (purge, size) {
        (true, Some(size)) => removal_candidate(kind, path.to_path_buf(), Some(size), None),
        (_, Some(size)) => kept_entry(kind, path.to_path_buf(), Some(size)),
        (_, None) => absent_entry(kind, Some(path.to_path_buf())),
    }
}

/// The old CWD-relative journal: never searched beyond the given path, and
/// only removable when it could be read as a journal first.
fn legacy_journal_entry(path: &Path, purge: bool) -> UninstallEntry {
    if !path.exists() {
        return absent_entry(TargetKind::LegacyJournal, Some(path.to_path_buf()));
    }
    match history::inspect_journal(path) {
        Ok(Some(inspection)) => {
            let size = std::fs::metadata(path).ok().map(|metadata| metadata.len());
            let mut entry = if purge {
                removal_candidate(TargetKind::LegacyJournal, path.to_path_buf(), size, None)
            } else {
                kept_entry(TargetKind::LegacyJournal, path.to_path_buf(), size)
            };
            entry.legacy_operations = Some(inspection.total_operations);
            entry
        }
        Ok(None) => absent_entry(TargetKind::LegacyJournal, Some(path.to_path_buf())),
        Err(_) => {
            let mut entry = kept_entry(
                TargetKind::LegacyJournal,
                path.to_path_buf(),
                std::fs::metadata(path).ok().map(|metadata| metadata.len()),
            );
            entry.error = Some("could not be read as a journal".to_string());
            entry
        }
    }
}

/// The Keychain row. `env` mode forces `Keep` here (and `execute` gate it
/// again), so no code path can reach the Keychain in that mode.
fn keychain_entry(options: &UninstallOptions, key_mode: KeySourceMode) -> UninstallEntry {
    let intent = options.remove_key || options.purge;
    let action = if intent && key_mode.keychain_allowed() {
        TargetAction::Remove
    } else {
        TargetAction::Keep
    };
    let status = match action {
        TargetAction::Remove => EntryStatus::WouldRemove,
        TargetAction::Keep => EntryStatus::Kept,
    };
    UninstallEntry {
        kind: TargetKind::Keychain,
        path: None,
        action,
        status,
        size_bytes: None,
        error: None,
        legacy_operations: None,
        kept_symlink: false,
    }
}

/// The binary row: never removed here, `cargo uninstall` owns it.
fn informational_entry(kind: TargetKind, path: Option<PathBuf>) -> UninstallEntry {
    UninstallEntry {
        kind,
        path,
        action: TargetAction::Keep,
        status: EntryStatus::Kept,
        size_bytes: None,
        error: None,
        legacy_operations: None,
        kept_symlink: false,
    }
}

fn removal_candidate(
    kind: TargetKind,
    path: PathBuf,
    size: Option<u64>,
    error: Option<String>,
) -> UninstallEntry {
    UninstallEntry {
        kind,
        path: Some(path),
        action: TargetAction::Remove,
        status: EntryStatus::WouldRemove,
        size_bytes: size,
        error,
        legacy_operations: None,
        kept_symlink: false,
    }
}

fn kept_entry(kind: TargetKind, path: PathBuf, size: Option<u64>) -> UninstallEntry {
    UninstallEntry {
        kind,
        path: Some(path),
        action: TargetAction::Keep,
        status: EntryStatus::Kept,
        size_bytes: size,
        error: None,
        legacy_operations: None,
        kept_symlink: false,
    }
}

fn absent_entry(kind: TargetKind, path: Option<PathBuf>) -> UninstallEntry {
    UninstallEntry {
        kind,
        path,
        action: TargetAction::Keep,
        status: EntryStatus::Absent,
        size_bytes: None,
        error: None,
        legacy_operations: None,
        kept_symlink: false,
    }
}

/// Perform the removal steps of a plan, in row order (completions, then the
/// requested configuration and journals, then the Keychain last).
///
/// A failure is recorded on its row and never aborted or rolled back: the
/// caller prints the plan again so the partial result is visible. The
/// Keychain is only reached through `store` and only when
/// `keychain_allowed` is true; `env` mode passes `false` and therefore never
/// reads or deletes the stored API key.
///
/// The confirmation runs at the Keychain step (the last one), so a declined
/// prompt never stops the earlier removals, matching the documented order.
pub fn execute(
    plan: &mut UninstallPlan,
    store: &dyn SecretStore,
    keychain_allowed: bool,
    confirm: &dyn Fn() -> anyhow::Result<bool>,
) {
    let allowed = keychain_allowed;
    for entry in plan.entries.iter_mut() {
        if entry.status != EntryStatus::WouldRemove {
            continue;
        }
        match entry.kind {
            TargetKind::Keychain => execute_keychain(entry, store, allowed, confirm),
            _ => remove_file_entry(entry),
        }
    }
}

/// Delete one file target, recording the outcome on its row.
fn remove_file_entry(entry: &mut UninstallEntry) {
    let Some(path) = entry.path.clone() else {
        entry.status = EntryStatus::Failed;
        entry.error = Some("no path recorded for this target".to_string());
        return;
    };
    match std::fs::remove_file(&path) {
        Ok(()) => entry.status = EntryStatus::Removed,
        Err(error) => {
            entry.status = EntryStatus::Failed;
            entry.error = Some(error.to_string());
        }
    }
}

/// The Keychain step: presence check, confirmation, deletion.
fn execute_keychain(
    entry: &mut UninstallEntry,
    store: &dyn SecretStore,
    keychain_allowed: bool,
    confirm: &dyn Fn() -> anyhow::Result<bool>,
) {
    // Second gate: even a plan built for another mode can never let an
    // `env`-mode run touch the Keychain.
    if !keychain_allowed {
        entry.action = TargetAction::Keep;
        entry.status = EntryStatus::Kept;
        return;
    }
    match store.get() {
        Err(AuthError::NotFound) => entry.status = EntryStatus::Absent,
        Err(error) => record_keychain_failure(entry, &error.to_string()),
        Ok(_) => confirm_and_delete(entry, store, confirm),
    }
}

/// Ask for confirmation (unless already given) and delete the API key.
fn confirm_and_delete(
    entry: &mut UninstallEntry,
    store: &dyn SecretStore,
    confirm: &dyn Fn() -> anyhow::Result<bool>,
) {
    match confirm() {
        Ok(false) => entry.status = EntryStatus::Cancelled,
        Err(error) => record_keychain_failure(entry, &error.to_string()),
        Ok(true) => match store.delete() {
            Ok(()) => entry.status = EntryStatus::Removed,
            Err(AuthError::NotFound) => entry.status = EntryStatus::Absent,
            Err(error) => record_keychain_failure(entry, &error.to_string()),
        },
    }
}

fn record_keychain_failure(entry: &mut UninstallEntry, message: &str) {
    entry.status = EntryStatus::Failed;
    entry.error = Some(message.to_string());
}

//! `kaseifu uninstall` report (ZC-11, display-only).
//!
//! Doctor-like check rows: the first word says what happens to the target
//! (`remove` / `removed` / `keep` / `none` / `FAIL`), then `Name: detail`.
//! The summary line carries the target count and the total size. ASCII only;
//! Rich wraps the same lines in the shared frame (see `emit_report`).
//!
//! Nothing here decides anything: the plan rows come from [`crate::uninstall`]
//! already executed (or not), and the Keychain is never read from a renderer.

use crate::auth::keychain::KEYCHAIN_SERVICE;
use crate::config::key_source::KeySourceSetting;
use crate::uninstall::{EntryStatus, TargetAction, TargetKind, UninstallEntry, UninstallPlan};

/// Report lines for the text / Rich output.
pub fn report_lines(plan: &UninstallPlan, key_source: &KeySourceSetting) -> Vec<String> {
    let mut lines = vec![summary_line(plan)];
    lines.extend(
        plan.entries
            .iter()
            .map(|entry| entry_line(entry, key_source)),
    );
    lines
}

/// Structured report for `--format json` (this command has no earlier JSON
/// shape, so every key here is new; existing keys of other commands are not
/// involved).
pub fn json_report(plan: &UninstallPlan, key_source: &KeySourceSetting) -> serde_json::Value {
    let (remove_count, remove_bytes) = removal_totals(plan);
    serde_json::json!({
        "mode": if plan.options.execute { "execute" } else { "dry_run" },
        "purge": plan.options.purge,
        "remove_key": plan.options.remove_key,
        "key_source_mode": key_source.mode.label(),
        "key_source_default": !key_source.explicit,
        "targets": plan.entries.iter()
            .map(|entry| entry_json(entry, key_source))
            .collect::<Vec<_>>(),
        "totals": {"remove_count": remove_count, "remove_bytes": remove_bytes},
    })
}

/// First line: what this run does (or did) in one sentence.
fn summary_line(plan: &UninstallPlan) -> String {
    let (count, bytes) = removal_totals(plan);
    let failed = plan
        .entries
        .iter()
        .filter(|entry| entry.status == EntryStatus::Failed)
        .count();
    if !plan.options.execute {
        return if count == 0 {
            "nothing to remove (dry run; nothing was deleted)".to_string()
        } else {
            format!(
                "{count} target(s), {} total (dry run; nothing was deleted)",
                size_word(bytes)
            )
        };
    }
    if count == 0 && failed == 0 {
        return "nothing to remove (already clean)".to_string();
    }
    let mut line = format!("removed {count} target(s), {} total", size_word(bytes));
    if failed > 0 {
        line.push_str(&format!(
            "; {failed} failed (partial result; nothing is rolled back)"
        ));
    }
    line
}

/// Removal targets still on the table: requested and present (or deleted).
fn removal_totals(plan: &UninstallPlan) -> (usize, u64) {
    plan.entries
        .iter()
        .filter(|entry| {
            entry.action == TargetAction::Remove
                && matches!(
                    entry.status,
                    EntryStatus::WouldRemove | EntryStatus::Removed
                )
        })
        .fold((0, 0), |(count, bytes), entry| {
            (count + 1, bytes + entry.size_bytes.unwrap_or(0))
        })
}

/// One check row: `word Name: detail`.
fn entry_line(entry: &UninstallEntry, key_source: &KeySourceSetting) -> String {
    format!(
        "{:<7} {}: {}",
        status_word(entry),
        kind_label(entry.kind),
        detail(entry, key_source)
    )
}

fn status_word(entry: &UninstallEntry) -> &'static str {
    match entry.status {
        EntryStatus::Removed => "removed",
        EntryStatus::WouldRemove => "remove",
        EntryStatus::Failed => "FAIL",
        EntryStatus::Absent => "none",
        EntryStatus::Kept | EntryStatus::Cancelled => "keep",
    }
}

fn kind_label(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Completion => "Completion",
        TargetKind::Config => "Config",
        TargetKind::Journal => "Journal",
        TargetKind::LegacyJournal => "Legacy Journal",
        TargetKind::Keychain => "Keychain",
        TargetKind::Binary => "Binary",
        TargetKind::MovedFiles => "Moved files",
    }
}

/// The human sentence of one row (never contains a secret).
fn detail(entry: &UninstallEntry, key_source: &KeySourceSetting) -> String {
    match entry.kind {
        TargetKind::Completion => completion_detail(entry),
        TargetKind::Config => file_detail(entry, FileAdvice::Config),
        TargetKind::Journal => file_detail(entry, FileAdvice::Journal),
        TargetKind::LegacyJournal => legacy_detail(entry),
        TargetKind::Keychain => keychain_detail(entry, key_source),
        TargetKind::Binary => binary_detail(entry),
        TargetKind::MovedFiles => {
            "kaseifu never touches files it moved; run `kaseifu undo` to bring them back"
                .to_string()
        }
    }
}

fn completion_detail(entry: &UninstallEntry) -> String {
    let Some(path) = &entry.path else {
        return "no _kaseifu in the detected completion directories (nothing to remove)"
            .to_string();
    };
    let shown = path.display();
    match entry.status {
        EntryStatus::Failed => not_deleted(entry, &shown.to_string()),
        EntryStatus::Absent => format!("no file at {shown} (nothing to remove)"),
        _ if entry.kept_symlink => {
            format!("{shown} (symbolic link; not written by kaseifu, left alone)")
        }
        EntryStatus::Removed => format!(
            "{shown} ({}; the binary regenerates it on the next run; run `cargo uninstall kaseifu` to remove kaseifu itself)",
            size_word(entry.size_bytes.unwrap_or(0))
        ),
        EntryStatus::WouldRemove | EntryStatus::Kept | EntryStatus::Cancelled => format!(
            "{shown} ({}; regenerated on the next run while the binary stays)",
            size_word(entry.size_bytes.unwrap_or(0))
        ),
    }
}

/// Which advice sentence a configuration / journal row carries.
#[derive(Clone, Copy)]
enum FileAdvice {
    Config,
    Journal,
}

fn file_detail(entry: &UninstallEntry, advice: FileAdvice) -> String {
    let shown = entry
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    match advice {
        FileAdvice::Config => config_detail(entry, &shown),
        FileAdvice::Journal => journal_detail(entry, &shown),
    }
}

fn config_detail(entry: &UninstallEntry, shown: &str) -> String {
    if entry.status == EntryStatus::Failed {
        return not_deleted(entry, shown);
    }
    if entry.status == EntryStatus::Absent {
        return format!("no config at {shown} (nothing to remove)");
    }
    let size = size_word(entry.size_bytes.unwrap_or(0));
    if entry.status == EntryStatus::Removed {
        format!("{shown} ({size}; the configuration is gone)")
    } else if entry.action == TargetAction::Remove {
        format!("{shown} ({size}; this deletes your configuration)")
    } else {
        format!("{shown} ({size}; --purge deletes it)")
    }
}

fn journal_detail(entry: &UninstallEntry, shown: &str) -> String {
    if entry.status == EntryStatus::Failed {
        return not_deleted(entry, shown);
    }
    if entry.status == EntryStatus::Absent {
        return format!("no journal at {shown} (nothing to remove)");
    }
    let size = size_word(entry.size_bytes.unwrap_or(0));
    if entry.status == EntryStatus::Removed {
        format!("{shown} ({size}; the undo history is gone)")
    } else if entry.action == TargetAction::Remove {
        format!("{shown} ({size}; this loses the undo history)")
    } else {
        format!("{shown} ({size}; --purge deletes it; the undo history is lost)")
    }
}

fn legacy_detail(entry: &UninstallEntry) -> String {
    let shown = "data/kaseifu.db";
    let outer = "journals in other directories are never touched";
    match entry.status {
        EntryStatus::Absent => format!("no {shown} in the current directory ({outer})"),
        EntryStatus::Failed => not_deleted(entry, shown),
        EntryStatus::Kept if entry.error.is_some() => format!(
            "{shown} ({}; not deleted, check it yourself; {outer})",
            entry.error.as_deref().unwrap_or("unreadable")
        ),
        EntryStatus::Removed => format!(
            "{shown} ({} operation(s); deleted here; {outer})",
            entry.legacy_operations.unwrap_or(0)
        ),
        EntryStatus::WouldRemove => format!(
            "{shown} ({} operation(s); deleted only in the current directory; {outer})",
            entry.legacy_operations.unwrap_or(0)
        ),
        EntryStatus::Kept | EntryStatus::Cancelled => format!(
            "{shown} ({} operation(s); --purge --execute deletes this one only; {outer})",
            entry.legacy_operations.unwrap_or(0)
        ),
    }
}

fn keychain_detail(entry: &UninstallEntry, key_source: &KeySourceSetting) -> String {
    if !key_source.mode.keychain_allowed() {
        return "not touched in env mode (KASEIFU_KEY_SOURCE=env); nothing is read or deleted"
            .to_string();
    }
    match entry.status {
        EntryStatus::Failed => format!(
            "not deleted: {} (service {KEYCHAIN_SERVICE})",
            entry.error.as_deref().unwrap_or("unknown error")
        ),
        EntryStatus::Absent => {
            format!("no API key is stored (service {KEYCHAIN_SERVICE})")
        }
        EntryStatus::Cancelled => {
            format!("the confirmation was declined; the API key stays (service {KEYCHAIN_SERVICE})")
        }
        EntryStatus::Removed => {
            format!("deleted from the macOS Keychain (service {KEYCHAIN_SERVICE})")
        }
        EntryStatus::WouldRemove => {
            format!("service {KEYCHAIN_SERVICE} (asks for confirmation; --yes skips the prompt)")
        }
        EntryStatus::Kept => format!(
            "service {KEYCHAIN_SERVICE} (--remove-key or --purge deletes it; asks first unless --yes)"
        ),
    }
}

fn binary_detail(entry: &UninstallEntry) -> String {
    match &entry.path {
        Some(path) => format!(
            "{} is not removed here; run `cargo uninstall kaseifu`",
            path.display()
        ),
        None => "not removed here; run `cargo uninstall kaseifu`".to_string(),
    }
}

fn not_deleted(entry: &UninstallEntry, shown: &str) -> String {
    format!(
        "{shown} (not deleted: {})",
        entry.error.as_deref().unwrap_or("unknown error")
    )
}

/// Structured row; `detail` carries the same human sentence as the text row.
fn entry_json(entry: &UninstallEntry, key_source: &KeySourceSetting) -> serde_json::Value {
    let mut value = serde_json::json!({
        "kind": kind_word(entry.kind),
        "action": if entry.action == TargetAction::Remove { "remove" } else { "keep" },
        "status": status_word_json(entry.status),
        "detail": detail(entry, key_source),
    });
    if let Some(path) = &entry.path {
        value["path"] = path.display().to_string().into();
    }
    if entry.kind == TargetKind::Keychain {
        value["service"] = KEYCHAIN_SERVICE.into();
    }
    if let Some(size) = entry.size_bytes {
        value["size_bytes"] = size.into();
    }
    if let Some(operations) = entry.legacy_operations {
        value["operations"] = operations.into();
    }
    if let Some(error) = &entry.error {
        value["error"] = error.clone().into();
    }
    value
}

fn kind_word(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Completion => "completion",
        TargetKind::Config => "config",
        TargetKind::Journal => "journal",
        TargetKind::LegacyJournal => "legacy_journal",
        TargetKind::Keychain => "keychain",
        TargetKind::Binary => "binary",
        TargetKind::MovedFiles => "moved_files",
    }
}

fn status_word_json(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::WouldRemove => "would_remove",
        EntryStatus::Removed => "removed",
        EntryStatus::Kept => "kept",
        EntryStatus::Absent => "absent",
        EntryStatus::Cancelled => "cancelled",
        EntryStatus::Failed => "failed",
    }
}

/// Human byte size (ASCII only).
fn size_word(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < MB {
        format!("{:.1} KB", value / KB)
    } else if value < GB {
        format!("{:.1} MB", value / MB)
    } else {
        format!("{:.1} GB", value / GB)
    }
}

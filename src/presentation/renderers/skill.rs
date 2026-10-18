//! `kaseifu skill` report (install / scan / check, display-only).
//!
//! One row per target, with the same status words in the text report and in the
//! JSON (`up-to-date` / `installed` / `preview` / `refused` / `FAIL`); `check`
//! adds the two digests the comparison is about. The detail sentence of a row
//! is the same string in both shapes. ASCII only; Rich wraps the same lines in
//! the shared frame (see `emit_report`).
//!
//! Nothing here decides anything: the copies and their digests come from
//! [`crate::skill`], and only the caller's `--write` decides what is written.

use crate::skill::{CheckReport, INSTALL_REL_PATH, InstallReport, InstallStatus, ScanReport};

/// Report lines of `skill install` for one repository.
pub fn install_lines(report: &InstallReport) -> Vec<String> {
    let mut lines = vec![
        format!("target: {}", report.target.display()),
        format!(
            "status: {} ({})",
            install_word(report.status),
            install_detail(report)
        ),
        format!("embedded sha256: {}", report.embedded_sha256),
    ];
    if let Some(file) = &report.file_sha256 {
        lines.push(format!("file sha256: {file}"));
    }
    lines
}

/// Structured report of `skill install`.
pub fn install_json(report: &InstallReport) -> serde_json::Value {
    serde_json::json!({
        "skill": {
            "action": "install",
            "target": report.target.display().to_string(),
            "status": install_word_json(report.status),
            "detail": install_detail(report),
            "embedded_sha256": report.embedded_sha256,
        }
    })
}

/// Report lines of `skill check`.
pub fn check_lines(report: &CheckReport) -> Vec<String> {
    vec![
        format!("target: {}", report.path.display()),
        format!(
            "status: {} ({})",
            check_word(report.matches),
            check_detail(report.matches)
        ),
        format!("embedded sha256: {}", report.embedded_sha256),
        format!("file sha256: {}", report.file_sha256),
    ]
}

/// Structured report of `skill check`.
pub fn check_json(report: &CheckReport) -> serde_json::Value {
    serde_json::json!({
        "skill": {
            "action": "check",
            "target": report.path.display().to_string(),
            "status": check_word(report.matches),
            "detail": check_detail(report.matches),
            "embedded_sha256": report.embedded_sha256,
            "file_sha256": report.file_sha256,
        }
    })
}

/// Report lines of `skill install --scan`: one row per repository, in the order
/// the walk found them.
pub fn scan_lines(report: &ScanReport) -> Vec<String> {
    if report.targets.is_empty() {
        return vec![format!(
            "no repository at or below {} has {INSTALL_REL_PATH}",
            report.parent.display()
        )];
    }
    let mut lines = vec![format!(
        "scan: {} ({} repository copy/copies)",
        report.parent.display(),
        report.targets.len()
    )];
    lines.extend(report.targets.iter().map(scan_row));
    lines
}

/// Structured report of `skill install --scan`.
pub fn scan_json(report: &ScanReport) -> serde_json::Value {
    serde_json::json!({
        "skill": {
            "action": "install",
            "scan": report.parent.display().to_string(),
            "targets": report
                .targets
                .iter()
                .map(|target| match &target.outcome {
                    Ok(install) => install_object(install, &target.path),
                    Err(error) => serde_json::json!({
                        "target": target.path.display().to_string(),
                        "status": "failed",
                        "detail": error.clone(),
                    }),
                })
                .collect::<Vec<_>>(),
        }
    })
}

/// One install row as an object: the scan walks files and states the path the
/// walk found, a single install already carries it.
fn install_object(report: &InstallReport, path: &std::path::Path) -> serde_json::Value {
    let mut value = serde_json::json!({
        "target": path.display().to_string(),
        "status": install_word_json(report.status),
        "detail": install_detail(report),
        "embedded_sha256": report.embedded_sha256,
    });
    if let Some(file) = &report.file_sha256 {
        value["file_sha256"] = file.clone().into();
    }
    value
}

/// One scan row: the status word, the file, and the same detail sentence a
/// single install uses.
fn scan_row(target: &crate::skill::ScanTarget) -> String {
    let path = target.path.display();
    match &target.outcome {
        Ok(install) => format!(
            "{:<10} {path} ({})",
            install_word(install.status),
            install_detail(install)
        ),
        Err(error) => format!("{:<10} {path} ({error})", "FAIL"),
    }
}

/// Text word for what happened to one copy.
fn install_word(status: InstallStatus) -> &'static str {
    match status {
        InstallStatus::UpToDate => "up-to-date",
        InstallStatus::Installed => "installed",
        InstallStatus::Preview => "preview",
        InstallStatus::Refused => "refused",
    }
}

/// The same words for JSON (`FAIL` has no variant: a failure carries the cause).
fn install_word_json(status: InstallStatus) -> &'static str {
    match status {
        InstallStatus::UpToDate => "up_to_date",
        InstallStatus::Installed => "installed",
        InstallStatus::Preview => "preview",
        InstallStatus::Refused => "refused",
    }
}

/// The human sentence of one install row (one sentence, no secrets).
fn install_detail(report: &InstallReport) -> String {
    match (report.status, report.file_sha256.is_some()) {
        (InstallStatus::UpToDate, _) => "the copy already matches the embedded skill".to_string(),
        (InstallStatus::Installed, _) => "the embedded skill was written".to_string(),
        (InstallStatus::Refused, _) => {
            "the copy differs from the embedded skill; --force replaces it".to_string()
        }
        (InstallStatus::Preview, true) => {
            "nothing was written; the copy differs from the embedded skill (--write needs --force)"
                .to_string()
        }
        (InstallStatus::Preview, false) => {
            "nothing was written; --write places the file and creates the directory chain"
                .to_string()
        }
    }
}

/// The word of a check row.
fn check_word(matches: bool) -> &'static str {
    if matches { "match" } else { "stale" }
}

/// The human sentence of a check row.
fn check_detail(matches: bool) -> String {
    if matches {
        "the copy is byte-identical to the embedded skill".to_string()
    } else {
        "the copy differs from the embedded skill".to_string()
    }
}

//! Environment diagnostics (`kaseifu doctor`).
//!
//! Local checks always run; network checks only run with an explicit flag so
//! a plain `doctor` never touches the network. Output is ASCII-only and the
//! API key value never appears. The API key check reports the resolved source
//! (environment / `.env` / Keychain) from the already-resolved configuration;
//! `doctor` itself never touches the Keychain, so it can never trigger an
//! access-approval dialog.
//!
//! The journal checks are read-only: the journal's resolved location and any
//! `pending` rows are *reported* (with the current state of the files they
//! name), never repaired. An interrupted move cannot be told apart from one
//! still in flight without the filesystem, so automatic recovery could
//! destroy a correct state — detection is the whole contract (R1/R2).

use crate::completion::{self, Status};
use crate::config::ResolvedConfig;
use crate::config::key_source::{ApiKeyOrigin, KeySourceMode};
use crate::storage::history::{self, JournalInspection, PendingOperation};
use std::path::Path;
use std::time::Duration;

/// Maximum number of pending rows whose file state is spelled out in the
/// journal check detail; further rows are counted only.
const MAX_PENDING_DETAILS: usize = 3;

/// Outcome of one diagnostic check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// Short check name (e.g. `Configuration`).
    pub name: &'static str,
    /// True when the check passed.
    pub ok: bool,
    /// Human-readable detail (never contains secrets).
    pub detail: String,
}

/// Connectivity outcome for the Jev endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connectivity {
    /// No network access was attempted.
    NotTested,
    /// The endpoint answered (any HTTP status counts as reachable).
    Reachable,
    /// No usable answer arrived.
    Unreachable(String),
}

/// Full doctor report (local checks plus optional connectivity).
#[derive(Debug, Clone)]
pub struct DoctorReport {
    /// Local environment checks.
    pub checks: Vec<CheckResult>,
    /// Connectivity result.
    pub connectivity: Connectivity,
    /// Effective endpoint shown for context (never the key).
    pub endpoint: String,
}

/// Run local diagnostics; probe the network only when `check_connection`.
pub async fn diagnose(config: &ResolvedConfig, check_connection: bool) -> DoctorReport {
    diagnose_with(
        config,
        check_connection,
        &history::journal_path(),
        Path::new(history::LEGACY_DATABASE_PATH),
    )
    .await
}

/// [`diagnose`] with explicit journal locations.
///
/// Tests pass temporary paths here so they never read (or depend on) the
/// developer's real journal; production resolves the defaults in [`diagnose`].
pub async fn diagnose_with(
    config: &ResolvedConfig,
    check_connection: bool,
    journal_path: &Path,
    legacy_journal: &Path,
) -> DoctorReport {
    let checks = vec![
        check_platform(),
        check_configuration(),
        check_api_key(config),
        check_endpoint(&config.jev.endpoint),
        check_spotlight(),
        check_storage(),
        check_journal(journal_path),
        check_legacy_journal(legacy_journal, journal_path),
        check_completion(),
    ];
    let connectivity = probe_connectivity(&config.jev.endpoint, check_connection).await;
    DoctorReport {
        checks,
        connectivity,
        endpoint: config.jev.endpoint.clone(),
    }
}

fn pass(name: &'static str, detail: &str) -> CheckResult {
    CheckResult {
        name,
        ok: true,
        detail: detail.to_string(),
    }
}

fn fail(name: &'static str, detail: &str) -> CheckResult {
    CheckResult {
        name,
        ok: false,
        detail: detail.to_string(),
    }
}

fn check_platform() -> CheckResult {
    if cfg!(target_os = "macos") {
        pass("Platform", "macOS supported")
    } else {
        fail("Platform", "not macOS; Keychain and Spotlight need macOS")
    }
}

fn check_configuration() -> CheckResult {
    match crate::config::loader::load_user() {
        Ok(_) => pass("Configuration", "TOML loaded"),
        Err(error) => fail("Configuration", &short_error(&error)),
    }
}

/// API key source check.
///
/// The resolver already recorded where the key came from (including why a
/// lookup failed), so this check never reads the Keychain itself: in `env`
/// mode the Keychain is not reached by any doctor code path.
fn check_api_key(config: &ResolvedConfig) -> CheckResult {
    match &config.api_key_origin {
        ApiKeyOrigin::Environment => pass("API Key", "provided via environment (JEV_API_KEY)"),
        ApiKeyOrigin::Dotenv => pass("API Key", "provided via .env"),
        ApiKeyOrigin::Keychain => pass("API Key", "resolved from macOS Keychain"),
        ApiKeyOrigin::Missing => fail("API Key", missing_key_detail(config.key_source.mode)),
        ApiKeyOrigin::Unavailable(message) => fail("API Key", message),
    }
}

/// Missing-key detail depends on the selected source so the fix is obvious.
fn missing_key_detail(mode: KeySourceMode) -> &'static str {
    match mode {
        KeySourceMode::Env => {
            "not configured (KASEIFU_KEY_SOURCE=env); set JEV_API_KEY in the environment or in .env"
        }
        KeySourceMode::Keychain => {
            "no API key stored in the macOS Keychain (run the release-build kaseifu setup)"
        }
        KeySourceMode::Auto => "no API key configured (run kaseifu setup)",
    }
}

fn check_endpoint(endpoint: &str) -> CheckResult {
    match url::Url::parse(endpoint) {
        Ok(url) if url.scheme() == "https" || url.scheme() == "http" => {
            pass("Jev Endpoint", "URL form looks valid")
        }
        _ => fail("Jev Endpoint", "URL form is invalid"),
    }
}

fn check_spotlight() -> CheckResult {
    let mdls = command_available("mdls");
    let mdfind = command_available("mdfind");
    if mdls && mdfind {
        pass("Spotlight", "mdls and mdfind available")
    } else {
        fail("Spotlight", "mdls or mdfind is missing")
    }
}

fn check_storage() -> CheckResult {
    let dir = crate::config::storage::app_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => pass("Storage", "data directory is writable"),
        Err(error) => fail("Storage", &short_io_error(&error)),
    }
}

/// Read-only journal check: resolved path, operation count, pending rows.
///
/// Reports `FAIL` only when a pending row is old enough to be an interrupted
/// operation (or the file is not a readable journal). A fresh pending row may
/// belong to a concurrent process and passes with the count in the detail.
fn check_journal(path: &Path) -> CheckResult {
    match history::inspect_journal(path) {
        Ok(None) => pass("Journal", &format!("{} (no journal yet)", path.display())),
        Ok(Some(inspection)) => {
            let detail = journal_detail(path, &inspection);
            if inspection.pending.iter().any(|pending| pending.stale) {
                fail("Journal", &detail)
            } else {
                pass("Journal", &detail)
            }
        }
        Err(error) => fail(
            "Journal",
            &format!("{}: {}", path.display(), short_error(&error)),
        ),
    }
}

/// One-line journal summary: path, operation count, pending count with ages
/// and the current state of the pending files.
fn journal_detail(path: &Path, inspection: &JournalInspection) -> String {
    let mut detail = format!(
        "{} ({} operation(s)",
        path.display(),
        inspection.total_operations
    );
    if inspection.pending.is_empty() {
        detail.push_str("; no pending)");
        return detail;
    }
    let stale = inspection.pending.iter().filter(|row| row.stale).count();
    detail.push_str(&format!(
        "; {} pending, {} possibly interrupted; pending file state: ",
        inspection.pending.len(),
        stale
    ));
    let states: Vec<String> = inspection
        .pending
        .iter()
        .take(MAX_PENDING_DETAILS)
        .map(|row| format!("{}: {}", row.entry.operation, file_state(row)))
        .collect();
    detail.push_str(&states.join("; "));
    if inspection.pending.len() > MAX_PENDING_DETAILS {
        detail.push_str(&format!(
            " (+{} more)",
            inspection.pending.len() - MAX_PENDING_DETAILS
        ));
    }
    detail.push(')');
    detail
}

/// Where a pending row's file currently is.
fn file_state(row: &PendingOperation) -> &'static str {
    match (row.source_exists, row.destination_exists) {
        (true, false) => "source present, destination absent",
        (false, true) => "source absent, destination present",
        (true, true) => "source and destination present",
        (false, false) => "source and destination absent",
    }
}

/// Detect the old CWD-relative journal. The file is never read, migrated or
/// deleted — it may hold a real user's undo history — so this check only
/// makes its existence visible.
fn check_legacy_journal(legacy: &Path, current: &Path) -> CheckResult {
    if legacy.exists() {
        fail(
            "Legacy Journal",
            &format!(
                "found {} (old CWD-relative journal; not read, not migrated; current journal: {})",
                legacy.display(),
                current.display()
            ),
        )
    } else {
        pass(
            "Legacy Journal",
            &format!("no {} in the current directory", legacy.display()),
        )
    }
}

fn command_available(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// zsh completion file state (installed / stale / missing) and its directory.
fn check_completion() -> CheckResult {
    let (status, dir) = completion::state();
    match status {
        Status::Installed => pass(
            "Completion",
            &format!(
                "zsh completions installed ({})",
                location_word(dir.as_deref())
            ),
        ),
        Status::Stale => fail(
            "Completion",
            &format!(
                "zsh completions stale ({}; {} -> v{}); run kaseifu completions zsh --install",
                location_word(dir.as_deref()),
                version_word(dir.as_deref()),
                completion::current_version(),
            ),
        ),
        Status::Missing => fail(
            "Completion",
            &format!(
                "zsh completions missing ({}); run kaseifu completions zsh --install",
                location_word(dir.as_deref()),
            ),
        ),
    }
}

fn location_word(dir: Option<&Path>) -> String {
    dir.map(|dir| dir.display().to_string())
        .unwrap_or_else(|| format!("no writable directory; set {}", completion::ENV_DIR))
}

fn version_word(dir: Option<&Path>) -> String {
    dir.and_then(completion::installed_version)
        .map(|version| format!("v{version}"))
        .unwrap_or_else(|| "unknown version".to_string())
}

async fn probe_connectivity(endpoint: &str, check_connection: bool) -> Connectivity {
    if !check_connection {
        return Connectivity::NotTested;
    }
    match reach_endpoint(endpoint).await {
        Ok(()) => Connectivity::Reachable,
        Err(detail) => Connectivity::Unreachable(detail),
    }
}

async fn reach_endpoint(endpoint: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| first_line(&error.to_string()))?;
    let response = client
        .get(endpoint)
        .send()
        .await
        .map_err(|error| first_line(&error.to_string()))?;
    let _ = response
        .bytes()
        .await
        .map_err(|error| first_line(&error.to_string()))?;
    Ok(())
}

fn short_error(error: &anyhow::Error) -> String {
    first_line(&error.to_string())
}

fn short_io_error(error: &std::io::Error) -> String {
    first_line(&error.to_string())
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or(text)
        .chars()
        .take(120)
        .collect()
}

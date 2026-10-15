//! v1.0.3 debug scenarios W2-W3 (M3 `debug-scenarios-w2-w5`).
//!
//! Each scenario below was rechecked against the current code before pinning;
//! design expectations that drifted are recorded, not copied:
//!
//! - W2 fully covered elsewhere, mapped here: S07 by `v32_image_metadata_test`
//!   (35 tests: source precedence, gap-only EXIF fill, corrupt/unsupported,
//!   GPS validation, no timezone guessing, mtime never a capture time) and
//!   S08 by `phase4_real_jev_adapter_test` (17: 401/422/500/transport/malformed
//!   mappings) + `phase4_full_pipeline_test` (`transport_error_propagated`
//!   and friends). No new W2 tests: the suites are green below.
//! - S21 drift: `--format json` now emits a document (M1 `json-for-undo-scan-watch`),
//!   so the design's "JSON を出さない" expectation is superseded (pinned in
//!   `v099`; the new scan contract is pinned here).
//! - S25 drift: doctor runs 9 checks now (Platform, Configuration, API Key,
//!   Jev Endpoint, Spotlight, Storage, Journal, Legacy Journal, Completion),
//!   not the designed 6.
//!
//! CLI children run with an isolated HOME in temp dirs (never the real
//! journal, Keychain or files) with stdin closed and a deadline, so a hang
//! fails instead of freezing the suite.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use kaseifu::discovery::discover_with;
use kaseifu::models::CandidateDestination;
use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-w2w3-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn base_command(cwd: &Path, home: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR");
    command
}

fn run(dir: &Path, home: &Path, args: &[&str]) -> Output {
    let mut command = base_command(dir, home);
    command.args(args);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kaseifu spawns");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect output");
                panic!(
                    "kaseifu {args:?} exceeded deadline: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn isolated_journal(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("kaseifu.db")
}

fn cli_fixture(name: &str) -> (PathBuf, PathBuf) {
    let dir = temp_dir(name);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    (dir, home)
}

// ---------------------------------------------------------------------------
// Discovery helpers (S14, S27 use the injectable search boundary)
// ---------------------------------------------------------------------------

fn photo_context(target: &Path) -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: target.to_path_buf(),
            filename: "IMG_3020.jpeg".to_string(),
            extension: Some("jpeg".to_string()),
            ..Default::default()
        },
        semantic_signals: vec!["F:IMG".to_string(), "F:3020".to_string()],
        parent_directory: target
            .parent()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default(),
        ..Default::default()
    }
}

/// Discovery fixture root: the crate's own `target/` directory, removed on
/// drop. Temp dirs are unusable here: they live under `/var/folders`, which
/// the destination rules refuse as a system root tree (correct for real use,
/// so the discovery tests follow the phase4 fixture pattern instead).
struct DiscoveryFixture {
    root: PathBuf,
}

impl DiscoveryFixture {
    fn new(tag: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("v103-discovery-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root");
        Self { root }
    }

    fn write(&self, dir: &str, name: &str) -> PathBuf {
        let folder = self.root.join(dir);
        std::fs::create_dir_all(&folder).expect("fixture dir");
        let path = folder.join(name);
        std::fs::write(&path, b"fixture").expect("fixture file");
        path
    }
}

impl Drop for DiscoveryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn discover_with_hits(target: &Path, hits: Vec<String>) -> Vec<CandidateDestination> {
    discover_with(
        photo_context(target),
        move |_query, _limit| Ok(hits.clone()),
    )
    .expect("discover_with injectable search")
}

fn candidate_paths(candidates: &[CandidateDestination]) -> Vec<String> {
    candidates
        .iter()
        .map(|candidate| candidate.path.to_string_lossy().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// W3 / S14: system paths never become candidates
// ---------------------------------------------------------------------------

#[test]
fn w3_s14_system_path_hits_never_become_candidates() {
    let fixture = DiscoveryFixture::new("s14-system");
    let target = fixture.write("inbox", "IMG_3020.jpeg");
    let hits = vec![
        "/System/Library/CoreServices/photo.jpeg".to_string(),
        "/usr/share/icons/IMG_3020.jpeg".to_string(),
        "/private/var/folders/xx/IMG_3020.jpeg".to_string(),
        "/Users/test/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info/IMG_3020.jpeg".to_string(),
        "/Users/test/Library/Caches/IMG_3020.jpeg".to_string(),
    ];
    let candidates = discover_with_hits(&target, hits);
    assert!(
        candidates.is_empty(),
        "system hits must vanish: {candidates:?}"
    );
}

#[test]
fn w3_s14_own_parent_and_user_folders() {
    let fixture = DiscoveryFixture::new("s14-parent");
    let target = fixture.write("inbox", "IMG_3020.jpeg");
    let inbox_hit = fixture.write("inbox", "older-IMG_3020.jpeg");
    let photos_hit = fixture.write("Photos", "IMG_3020-copy.jpeg");
    let hits = vec![
        inbox_hit.to_string_lossy().to_string(),
        photos_hit.to_string_lossy().to_string(),
    ];
    let candidates = discover_with_hits(&target, hits);
    let paths = candidate_paths(&candidates);
    assert!(
        !paths.iter().any(|path| path.ends_with("inbox")),
        "the target's own parent is never a candidate: {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path.ends_with("Photos")),
        "a user folder stays: {paths:?}"
    );
    let ids: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    assert_eq!(ids, vec!["D1"], "ids are renumbered from D1: {ids:?}");
}

// ---------------------------------------------------------------------------
// W3 / S27: discover contract (hidden command, same guarantees)
// ---------------------------------------------------------------------------

#[test]
fn w3_s27_ranking_survives_truncation_at_fifty() {
    let fixture = DiscoveryFixture::new("s27-cap");
    let target = fixture.write("inbox", "IMG_3020.jpeg");
    let mut hits = Vec::new();
    for index in 0..120 {
        let hit = fixture.write(&format!("album-{index:03}"), "IMG_3020-copy.jpeg");
        hits.push(hit.to_string_lossy().to_string());
    }
    let candidates = discover_with_hits(&target, hits);
    assert!(
        candidates.len() <= 50,
        "the cap holds: {}",
        candidates.len()
    );
    assert!(
        !candidates.is_empty(),
        "truncation keeps the top, not nothing"
    );
    let mut scores: Vec<f64> = candidates.iter().map(|candidate| candidate.score).collect();
    let ranked = scores.clone();
    scores.sort_by(|left, right| right.partial_cmp(left).unwrap());
    assert_eq!(ranked, scores, "score order survives truncation");
}

#[test]
fn w3_s27_same_input_ranks_the_same_and_empty_stays_empty() {
    let fixture = DiscoveryFixture::new("s27-stable");
    let target = fixture.write("inbox", "IMG_3020.jpeg");
    let first_hit = fixture.write("Photos", "a-IMG_3020.jpeg");
    let second_hit = fixture.write("Photos", "b-IMG_3020.jpeg");
    let hits = vec![
        first_hit.to_string_lossy().to_string(),
        second_hit.to_string_lossy().to_string(),
    ];
    let first = discover_with_hits(&target, hits.clone());
    let second = discover_with_hits(&target, hits);
    let shape = |candidates: &[CandidateDestination]| {
        candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.id.clone(),
                    candidate.score,
                    candidate.path.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&first), shape(&second), "ranking is deterministic");
    let empty = discover_with_hits(&target, Vec::new());
    assert!(empty.is_empty(), "no hits is an empty list, never an error");
}

// ---------------------------------------------------------------------------
// W3 / S21: scan contract (new JSON behavior supersedes the design)
// ---------------------------------------------------------------------------

#[test]
fn w3_s21_missing_path_still_exits_zero_and_writes_nothing() {
    let (dir, home) = cli_fixture("s21-missing");
    let output = run(&dir, &home, &["scan", "/nonexistent-xyz"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(!isolated_journal(&home).exists(), "scan writes no journal");
}

#[test]
fn w3_s21_missing_argument_is_a_usage_error() {
    let (dir, home) = cli_fixture("s21-usage");
    let output = run(&dir, &home, &["scan"]);
    assert_eq!(output.status.code(), Some(2), "clap usage error");
    assert!(stdout_of(&output).is_empty(), "usage goes to stderr");
}

// ---------------------------------------------------------------------------
// W3 / S22: setup refuses without a terminal and touches nothing
// ---------------------------------------------------------------------------

#[test]
fn w3_s22_setup_without_a_terminal_fails_fast_and_writes_nothing() {
    let (dir, home) = cli_fixture("s22-nontty");
    let output = run(&dir, &home, &["setup"]);
    assert!(!output.status.success(), "no silent wizard without a TTY");
    assert!(
        stderr_of(&output).contains("needs a terminal"),
        "guidance names the cause: {}",
        stderr_of(&output)
    );
    let toml = home
        .join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("config.toml");
    assert!(!toml.exists(), "no partial configuration is written");
}

// ---------------------------------------------------------------------------
// W3 / S23 + S16: auth lifecycle without a key
// ---------------------------------------------------------------------------

#[test]
fn w3_s23_auth_status_reports_not_configured_without_a_key() {
    let (dir, home) = cli_fixture("s23-status");
    let output = run(&dir, &home, &["auth", "status"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(text.contains("Not configured"), "got: {text}");
    let json = run(&dir, &home, &["--format", "json", "auth", "status"]);
    assert!(json.status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(stdout_of(&json).trim()).expect("status JSON");
    assert_eq!(parsed["api_key_configured"], false);
}

#[test]
fn w3_s23_remove_key_when_nothing_is_stored_is_a_quiet_success() {
    let (dir, home) = cli_fixture("s23-remove");
    let output = run(&dir, &home, &["auth", "remove-key", "--yes"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
}

// ---------------------------------------------------------------------------
// W3 / S24: config validation at the CLI
// ---------------------------------------------------------------------------

#[test]
fn w3_s24_config_set_rejects_unknown_keys_and_bad_values() {
    let (dir, home) = cli_fixture("s24-set");
    let mut failures = HashMap::new();
    for (key, value) in [
        ("bogus.key", "x"),
        ("jev.endpoint", "ftp://x"),
        ("classifier.thresholds.auto_move_confidence", "9"),
        ("jev.api_key", "hunter2"),
    ] {
        let output = run(&dir, &home, &["config", "set", key, value]);
        assert!(!output.status.success(), "{key} must be refused");
        failures.insert(key, stderr_of(&output));
    }
    assert!(
        failures["bogus.key"].contains("unknown setting"),
        "{}",
        failures["bogus.key"]
    );
    assert!(
        failures["jev.endpoint"].contains("URL scheme"),
        "{}",
        failures["jev.endpoint"]
    );
    assert!(
        failures["classifier.thresholds.auto_move_confidence"].contains("between 0 and 1"),
        "{}",
        failures["classifier.thresholds.auto_move_confidence"]
    );
    assert!(
        failures["jev.api_key"].contains("auth set-key"),
        "{}",
        failures["jev.api_key"]
    );
}

#[test]
fn w3_s24_config_show_reports_key_state_without_the_value() {
    let (dir, home) = cli_fixture("s24-show");
    let output = run(&dir, &home, &["config", "show"]);
    assert!(output.status.success());
    assert!(
        stdout_of(&output).contains("Not configured"),
        "{}",
        stdout_of(&output)
    );
    let json = run(&dir, &home, &["--format", "json", "config", "show"]);
    assert!(json.status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(stdout_of(&json).trim()).expect("config JSON");
    assert!(parsed.get("api_key_configured").is_some(), "got: {parsed}");
}

// ---------------------------------------------------------------------------
// W3 / S25: doctor contract
// ---------------------------------------------------------------------------

#[test]
fn w3_s25_doctor_always_exits_zero_and_names_its_checks() {
    let (dir, home) = cli_fixture("s25-doctor");
    let output = run(&dir, &home, &["doctor"]);
    assert!(output.status.success(), "a failing check is not a crash");
    let text = stdout_of(&output);
    assert!(
        text.contains("Ready: no"),
        "the key check fails here: {text}"
    );
    assert!(
        text.contains("Not tested"),
        "no network without the flag: {text}"
    );
    for name in [
        "Platform",
        "Configuration",
        "API Key",
        "Spotlight",
        "Storage",
    ] {
        assert!(text.contains(name), "check {name} is shown: {text}");
    }
}

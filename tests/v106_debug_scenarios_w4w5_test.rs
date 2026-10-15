//! v1.0.6 debug scenarios W4-W5 (M3 `debug-scenarios-w2-w5`).
//!
//! Drift notes (rechecked, not copied from the design):
//!
//! - S15: the Jev path still gates GPS (`v32_jev_sends_*`, green); the rename
//!   path (Foundation Models) structurally cannot send it (`FmImageFacts` has
//!   no location field), pinned here.
//! - S16: covered by `key_source_cli_test` (env mode) plus the S23/S25 CLI
//!   tests in `v103`; one vocabulary delta is pinned here.
//! - S18: exercised at the coordinator level (no FSEvents, no model); the
//!   long-watch memory observation (self_events growth) stays a manual
//!   observation point, recorded in the issue.
//! - S19: the design wants N>=5 runs recorded; with a fixed mock the verdicts
//!   must be identical, pinned here. Threshold-near flapping is input variance
//!   (R6), not a bug, so no flapping test is committed.
//! - S20: the machine-checkable baseline parts (version string, journal schema
//!   columns, exit codes) are pinned here; cross-version diffing stays manual.
//! - C08: covered by `completion_test` (green, verified below); C01 by `v070`.
//! - C11: SIGINT cannot be tested in this environment (spawned shells inherit
//!   SIGINT-ignore, so even `sleep` survives it; verified 2026-10-15).
//!   SIGTERM prompt death is pinned instead. No signal handler exists by
//!   design; Ctrl+C in a real terminal kills via the default disposition.
//!
//! CLI children run with an isolated HOME in temp dirs with stdin closed and
//! a deadline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use kaseifu::classifier::mock_jev::MockJevIntegration;
use kaseifu::classifier::pipeline::classify_with_search;
use kaseifu::models::decision::DecisionAction;
use kaseifu::models::file_context::{ImageLocation, ImageMetadata};
use kaseifu::semantic::contract::FmImageFacts;
use kaseifu::watch::coordinator::{
    Disposition, EventCoordinator, RawEvent, SuppressReason, WatchConfig,
};

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-w4w5-{name}-{}", std::process::id()));
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
    run_with_env(dir, home, args, &[])
}

fn run_with_env(dir: &Path, home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = base_command(dir, home);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
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
// W4 / S26: inspect contract
// ---------------------------------------------------------------------------

#[test]
fn w4_s26_missing_path_fails_on_stderr_with_empty_stdout() {
    let (dir, home) = cli_fixture("s26-missing");
    let output = run(&dir, &home, &["inspect", "/nonexistent-xyz"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout_of(&output).is_empty(), "errors never touch stdout");
    assert!(
        stderr_of(&output).contains("cannot resolve"),
        "got: {}",
        stderr_of(&output)
    );
}

#[test]
fn w4_s26_directory_entries_stay_capped() {
    let (dir, home) = cli_fixture("s26-cap");
    let big = dir.join("big");
    std::fs::create_dir_all(&big).expect("big dir");
    for index in 0..120 {
        std::fs::write(big.join(format!("file-{index:03}.txt")), b"x").expect("entry");
    }
    let output = run(
        &dir,
        &home,
        &["--format", "json", "inspect", big.to_str().unwrap()],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("inspect JSON");
    let entries = json["directory_context"]["entries"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!entries.is_empty(), "entries are listed: {json}");
    assert!(
        entries.len() <= 100,
        "the entry cap holds: {}",
        entries.len()
    );
}

// ---------------------------------------------------------------------------
// W4 / S17 + C09/C10: non-ASCII and space paths
// ---------------------------------------------------------------------------

#[test]
fn w4_s17_japanese_and_space_names_survive_json_byte_exact() {
    let (dir, home) = cli_fixture("s17-names");
    let name = "THCO-COA-Non-Detect Updated 日本語ファイル.md";
    let target = dir.join(name);
    std::fs::write(&target, b"# hello\n").expect("fixture");
    let output = run(
        &dir,
        &home,
        &["--format", "json", "inspect", target.to_str().unwrap()],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let stdout = stdout_of(&output);
    assert!(!stdout.contains('\u{FFFD}'), "no replacement characters");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("inspect JSON");
    assert_eq!(json["target"]["filename"], serde_json::json!(name));
    assert!(
        json["target"]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with(name))
    );
}

#[test]
fn w4_s17_organize_to_round_trips_a_japanese_name() {
    let (dir, home) = cli_fixture("s17-move");
    let inbox = dir.join("inbox");
    std::fs::create_dir_all(&inbox).expect("inbox");
    std::fs::create_dir_all(dir.join("archive")).expect("archive");
    let name = "写真 2024年.md";
    let target = inbox.join(name);
    std::fs::write(&target, b"# hello\n").expect("fixture");
    let moved = run(
        &dir,
        &home,
        &[
            "organize",
            target.to_str().unwrap(),
            "--to",
            "archive",
            "--execute",
        ],
    );
    assert!(moved.status.success(), "stderr: {}", stderr_of(&moved));
    assert!(
        dir.join("archive").join(name).exists(),
        "moved under its own bytes"
    );
    let undo = run(&dir, &home, &["undo"]);
    assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
    assert!(target.exists(), "undo restores the byte-exact name");
}

// ---------------------------------------------------------------------------
// W4 / S15: the rename path cannot send GPS (structural pin)
// ---------------------------------------------------------------------------

#[test]
fn w4_s15_fm_image_facts_have_no_location_key() {
    let facts = FmImageFacts {
        capture_time: Some("2026-10-14T09:00:00".to_string()),
        camera: Some("TestMake TestModel".to_string()),
        width: Some(64),
        height: Some(48),
    };
    let json = serde_json::to_value(&facts).expect("serializes");
    let mut keys: Vec<&str> = json
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["camera", "capture_time", "height", "width"]);
    assert!(
        ImageMetadata {
            location: Some(ImageLocation {
                latitude: 35.658,
                longitude: 139.702,
                altitude_m: None,
                source: "exif".to_string(),
            }),
            ..Default::default()
        }
        .location
        .is_some(),
        "the observation can hold GPS; the wire type cannot"
    );
}

// ---------------------------------------------------------------------------
// W4 / S16: missing-key vocabulary
// ---------------------------------------------------------------------------

#[test]
fn w4_s16_missing_key_names_the_next_action() {
    let (dir, home) = cli_fixture("s16-vocab");
    let doctor = run(&dir, &home, &["doctor"]);
    assert!(doctor.status.success());
    assert!(
        stdout_of(&doctor).contains("FAIL"),
        "{}",
        stdout_of(&doctor)
    );
    assert!(
        stdout_of(&doctor).contains("API Key"),
        "{}",
        stdout_of(&doctor)
    );
}

// ---------------------------------------------------------------------------
// W4 / S19: repeated classification is stable with a fixed mock
// ---------------------------------------------------------------------------

fn s19_search(files: Vec<PathBuf>) -> impl Fn(&str, usize) -> anyhow::Result<Vec<String>> + Clone {
    move |_query: &str, _limit: usize| {
        Ok(files
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect())
    }
}

fn mock_selecting(destination: &str, probabilities: &[(&str, f64)]) -> MockJevIntegration {
    use kaseifu::classifier::jev::JevResponse;
    let mut map = HashMap::new();
    for (id, prob) in probabilities {
        map.insert((*id).to_string(), *prob);
    }
    MockJevIntegration::configured_decision(JevResponse {
        destination_id: Some(destination.to_string()),
        probabilities: map,
        confidence: 0.0,
        margin: 0.0,
    })
}

#[tokio::test]
async fn w4_s19_same_input_reaches_the_same_verdict_five_times() {
    // Discovery fixtures live under target/: temp dirs sit under /var/folders
    // and the destination rules refuse them (see v103), which would leave no
    // candidates and a vacuous NONE verdict.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("v106-s19-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let photos = root.join("Photos");
    let inbox = root.join("inbox");
    std::fs::create_dir_all(&photos).expect("photos");
    std::fs::create_dir_all(&inbox).expect("inbox");
    let target = inbox.join("IMG_3020.jpeg");
    std::fs::write(&target, b"fixture").expect("target");
    let similar = photos.join("IMG_3020-copy.jpeg");
    std::fs::write(&similar, b"fixture").expect("similar");
    let search = s19_search(vec![similar]);
    let mock = mock_selecting("D1", &[("D1", 0.82), ("NONE", 0.18)]);
    let mut verdicts = Vec::new();
    for _ in 0..5 {
        let decision = classify_with_search(&target, &mock, search.clone())
            .await
            .expect("classify succeeds");
        verdicts.push((decision.destination_id.clone(), decision.action.clone()));
    }
    assert!(
        verdicts.windows(2).all(|pair| pair[0] == pair[1]),
        "stable: {verdicts:?}"
    );
    assert_eq!(verdicts[0].0, Some("D1".to_string()));
    assert_eq!(verdicts[0].1, DecisionAction::MoveTo("D1".to_string()));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// W4 / S20: version and journal-schema baseline
// ---------------------------------------------------------------------------

#[test]
fn w4_s20_version_and_command_surface() {
    let (dir, home) = cli_fixture("s20-version");
    let output = run(&dir, &home, &["--version"]);
    assert!(output.status.success());
    assert_eq!(stdout_of(&output).trim(), "kaseifu 0.10.0");
    assert!(stderr_of(&output).is_empty());
}

#[test]
fn w4_s20_journal_schema_keeps_its_columns() {
    let (dir, home) = cli_fixture("s20-schema");
    let undo = run(&dir, &home, &["undo"]);
    assert!(undo.status.success());
    let journal = isolated_journal(&home);
    assert!(journal.exists(), "undo opens (and creates) the journal");
    let connection = rusqlite::Connection::open(&journal).expect("open journal");
    let mut statement = connection
        .prepare("PRAGMA table_info(operations)")
        .expect("pragma");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("rows")
        .map(|name| name.expect("column name"))
        .collect();
    for expected in [
        "id",
        "timestamp",
        "operation",
        "source_path",
        "destination_path",
        "candidate_id",
        "confidence",
        "margin",
        "result",
        "error",
        "undone_at",
    ] {
        assert!(
            columns.contains(&expected.to_string()),
            "column {expected} survives: {columns:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// W5 / S18: watch coordinator under load (no FSEvents, no model)
// ---------------------------------------------------------------------------

fn fast_config() -> WatchConfig {
    WatchConfig {
        debounce: Duration::from_millis(5),
        stability: Duration::from_millis(5),
        self_suppression: Duration::from_millis(50),
        managed_paths: Vec::new(),
    }
}

fn write_many(dir: &Path, count: usize) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for index in 0..count {
        let path = dir.join(format!("file-{index:04}.txt"));
        std::fs::write(&path, b"fixture").expect("fixture");
        paths.push(path);
    }
    paths
}

#[test]
fn w5_s18_one_thousand_rapid_events_coalesce_to_one_request() {
    let dir = temp_dir("s18-coalesce");
    let target = dir.join("note.txt");
    std::fs::write(&target, b"fixture").expect("target");
    let mut coordinator = EventCoordinator::new(fast_config());
    for _ in 0..1000 {
        coordinator.submit(
            RawEvent {
                path: target.clone(),
            },
            Instant::now(),
        );
    }
    // Two drain cycles: the first observes the fingerprint, the second (past
    // the stability window) emits the single request.
    std::thread::sleep(Duration::from_millis(30));
    assert!(coordinator.drain_due(Instant::now()).is_empty());
    std::thread::sleep(Duration::from_millis(30));
    let due = coordinator.drain_due(Instant::now());
    assert_eq!(due.len(), 1, "one path is one request: {}", due.len());
    assert_eq!(due[0].path, target);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w5_s18_distinct_paths_stay_bounded_and_in_flight_is_suppressed() {
    let dir = temp_dir("s18-bounded");
    let paths = write_many(&dir, 100);
    let mut coordinator = EventCoordinator::new(fast_config());
    for path in &paths {
        assert_eq!(
            coordinator.submit(RawEvent { path: path.clone() }, Instant::now()),
            Disposition::Accepted
        );
    }
    std::thread::sleep(Duration::from_millis(30));
    assert!(
        coordinator.drain_due(Instant::now()).is_empty(),
        "first cycle only observes"
    );
    std::thread::sleep(Duration::from_millis(30));
    let due = coordinator.drain_due(Instant::now());
    assert_eq!(due.len(), 100, "distinct paths are all processed once");
    let first = paths[0].clone();
    coordinator.begin_processing(&first);
    assert_eq!(
        coordinator.submit(
            RawEvent {
                path: first.clone()
            },
            Instant::now()
        ),
        Disposition::Suppressed(SuppressReason::DuplicateInFlight),
        "an in-flight path is not reprocessed"
    );
    coordinator.finish_processing(&first);
    assert_eq!(
        coordinator.submit(
            RawEvent {
                path: first.clone()
            },
            Instant::now()
        ),
        Disposition::Accepted,
        "a finished path accepts events again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w5_s18_self_generated_events_are_suppressed() {
    let dir = temp_dir("s18-self");
    let target = dir.join("note.txt");
    std::fs::write(&target, b"fixture").expect("target");
    let mut coordinator = EventCoordinator::new(fast_config());
    coordinator.record_self_operation(std::slice::from_ref(&target), Instant::now());
    assert_eq!(
        coordinator.submit(
            RawEvent {
                path: target.clone()
            },
            Instant::now()
        ),
        Disposition::Suppressed(SuppressReason::SelfGenerated),
        "our own move never loops back"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W5 / C02-C07: CLI surface (C01 help and C08 completions live in v070 +
// completion_test; C09/C10 unicode paths are S17 above)
// ---------------------------------------------------------------------------

#[test]
fn w5_c02_c06_version_success_and_usage_exit_codes() {
    let (dir, home) = cli_fixture("c-codes");
    let version = run(&dir, &home, &["--version"]);
    assert!(version.status.success());
    let missing = run(&dir, &home, &["scan"]);
    assert_eq!(missing.status.code(), Some(2), "missing arg is usage");
    let unknown = run(&dir, &home, &["bogus-cmd"]);
    assert_eq!(unknown.status.code(), Some(2), "unknown command is usage");
    let bad_value = run(&dir, &home, &["undo", "--last", "abc"]);
    assert_eq!(bad_value.status.code(), Some(2), "invalid value is usage");
    let resolve_fail = run(&dir, &home, &["inspect", "/nonexistent-xyz"]);
    assert_eq!(resolve_fail.status.code(), Some(1), "resolve failure is 1");
}

#[test]
fn w5_c07_help_and_json_stay_on_stdout_errors_on_stderr() {
    let (dir, home) = cli_fixture("c07-streams");
    let help = run(&dir, &home, &["--help"]);
    assert!(help.status.success());
    assert!(!stdout_of(&help).is_empty());
    assert!(stderr_of(&help).is_empty(), "help writes no diagnostics");
    let scan = run(&dir, &home, &["--format", "json", "scan", "/tmp"]);
    assert!(scan.status.success());
    assert!(stderr_of(&scan).is_empty(), "JSON stdout stays clean");
    serde_json::from_str::<serde_json::Value>(stdout_of(&scan).trim()).expect("parses");
}

// ---------------------------------------------------------------------------
// W5 / C11: interruption (SIGTERM prompt death; SIGINT is untestable here)
// ---------------------------------------------------------------------------

#[test]
fn w5_c11_watch_dies_promptly_on_sigterm_without_side_effects() {
    let (dir, home) = cli_fixture("c11-term");
    let watched = dir.join("watched");
    std::fs::create_dir_all(&watched).expect("watched");
    let target = watched.join("note.txt");
    std::fs::write(&target, b"fixture").expect("watched file");
    let mut child = base_command(&dir, &home)
        .args(["watch", "--path", watched.to_str().unwrap()])
        .env("JEV_API_KEY", "test-key-not-a-secret-0123456789")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("watch spawns");
    let pid = child.id();
    // Give the banner a moment, then terminate: no handler exists by design,
    // so the default disposition must kill promptly.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "watch is still running"
    );
    let terminated = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    assert_eq!(terminated, 0, "SIGTERM delivered");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("watch ignored SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(!status.success(), "killed by signal, not exit 0");
    assert!(target.exists(), "dry-run watch moves nothing");
    // Starting watch opens (and creates) the journal file; the pin is that no
    // operation row was ever recorded.
    let journal = isolated_journal(&home);
    if journal.exists() {
        let connection = rusqlite::Connection::open(&journal).expect("open journal");
        let rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
            .expect("count rows");
        assert_eq!(rows, 0, "no move was journaled");
    }
}

// ---------------------------------------------------------------------------
// W5 / C12: secrets never leak (P0)
// ---------------------------------------------------------------------------

#[test]
fn w5_c12_no_output_carries_the_api_key() {
    let (dir, home) = cli_fixture("c12-secrets");
    let secret = "supersecret-dummy-key-12345";
    let env = [("JEV_API_KEY", secret)];
    for args in [
        vec!["doctor"],
        vec!["--format", "json", "doctor"],
        vec!["config", "show"],
        vec!["--format", "json", "config", "show"],
        vec!["auth", "status"],
        vec!["--format", "json", "auth", "status"],
    ] {
        let output = run_with_env(&dir, &home, &args, &env);
        let combined = format!("{}{}", stdout_of(&output), stderr_of(&output));
        assert!(
            !combined.contains(secret),
            "{args:?} leaks the key: {combined}"
        );
    }
}

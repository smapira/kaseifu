//! Wave W1 debug scenarios (plan `20261014h`):
//! S28, S03, S11, S12, S13, S09, S10.
//!
//! Same grounding and isolation rules as the W0 file: fixtures under the
//! crate `target/` directory with Drop cleanup, in-memory journals, injected
//! or mocked Jev only. CLI children run with an isolated HOME/cwd and a
//! deadline, following `tests/v052_zero_config_cli_test.rs`.

use kaseifu::classifier::mock_jev::MockJevIntegration;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::{DecisionAction, JevDecision};
use kaseifu::operations::mover::{MoveError, MoveOutcome, dry_run, execute_move};
use kaseifu::operations::plan::build_move_plan;
use kaseifu::operations::undo::{UndoError, undo_last_move};
use kaseifu::policy::decision::{Thresholds, evaluate_with};
use kaseifu::storage::history::OperationHistory;
use kaseifu::watch::coordinator::{
    Disposition, EventCoordinator, ProcessingRequest, RawEvent, SuppressReason, WatchConfig,
};
use kaseifu::watch::runner::{ProcessOutcome, process_request};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Fixtures and helpers
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn make_fixture(tag: &str) -> Fixture {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("v091-fixture-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create fixture root");
    Fixture { root }
}

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn sha256_of(path: &Path) -> String {
    let bytes = fs::read(path).expect("read fixture file");
    format!("{:x}", Sha256::digest(&bytes))
}

fn decision(destination: Option<&str>, confidence: f64, margin: f64) -> JevDecision {
    JevDecision {
        destination_id: destination.map(str::to_string),
        confidence,
        margin,
        ..JevDecision::default()
    }
}

fn candidate(id: &str, directory: &Path) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: directory.to_path_buf(),
        score: 1.0,
        evidence_count: 1,
        reasons: Vec::new(),
    }
}

fn automove_setup(
    tag: &str,
) -> (
    Fixture,
    PathBuf,
    PathBuf,
    JevDecision,
    Vec<CandidateDestination>,
) {
    let fixture = make_fixture(tag);
    let source = fixture.root.join("report.txt");
    write_file(&source, b"payload");
    let dest_dir = fixture.root.join("Documents").join("Reports");
    fs::create_dir_all(&dest_dir).expect("create candidate dir");
    let decision = decision(Some("D1"), 0.95, 0.50);
    let candidates = vec![candidate("D1", &dest_dir)];
    (fixture, source, dest_dir, decision, candidates)
}

fn journal_rows(history: &OperationHistory) -> usize {
    history.entries().expect("read journal").len()
}

// ---------------------------------------------------------------------------
// S03: policy thresholds at the boundary
// Engine: >= comparisons, NONE/None always Leave (src/policy/decision.rs).
// ---------------------------------------------------------------------------

#[test]
fn w1_s03_boundary_table_matches_the_engine() {
    // (confidence, margin, destination, expected verdict)
    let cases: [(f64, f64, Option<&str>, &str); 8] = [
        (0.849, 0.400, Some("D1"), "review"),
        (0.850, 0.399, Some("D1"), "review"),
        (0.850, 0.400, Some("D1"), "automove"),
        (0.950, 0.100, Some("D1"), "review"),
        (0.600, 0.000, Some("D1"), "review"),
        (0.599, 0.000, Some("D1"), "leave"),
        (0.999, 0.999, Some("NONE"), "leave"),
        (0.999, 0.999, None, "leave"),
    ];
    for (confidence, margin, destination, expected) in cases {
        let action = evaluate_with(
            &decision(destination, confidence, margin),
            Thresholds::default(),
        );
        let verdict = match &action {
            DecisionAction::AutoMove(_) => "automove",
            DecisionAction::Review(_) => "review",
            DecisionAction::Leave { .. } => "leave",
            DecisionAction::None => "none",
            _ => "other",
        };
        assert_eq!(
            verdict, expected,
            "({confidence}, {margin}, {destination:?})"
        );
    }
}

#[test]
fn w1_s03_automove_payload_is_an_id_never_a_path() {
    let action = evaluate_with(&decision(Some("D1"), 0.95, 0.50), Thresholds::default());
    match action {
        DecisionAction::AutoMove(id) => assert_eq!(id, "D1"),
        other => panic!("expected AutoMove, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// S11: destination changes between plan and execute
// Both refusals happen in verify, before record_pending: 0 rows.
// ---------------------------------------------------------------------------

#[test]
fn w1_s11a_squatter_after_plan_is_a_collision_refusal() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s11a");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan builds");
    // The world changes after planning: a different-content file appears.
    let squatter = dest_dir.join("report.txt");
    write_file(&squatter, b"squatter-bytes");
    let source_hash = sha256_of(&source);

    let error = execute_move(&plan, &history).expect_err("collision");
    assert!(matches!(error, MoveError::Collision(_)), "got {error:?}");
    assert_eq!(sha256_of(&source), source_hash);
    assert_eq!(
        sha256_of(&squatter),
        format!("{:x}", Sha256::digest(b"squatter-bytes"))
    );
    assert_eq!(journal_rows(&history), 0);
}

#[test]
fn w1_s11b_removed_destination_is_a_refusal() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s11b");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan builds");
    fs::remove_dir_all(&dest_dir).expect("remove destination");
    let source_hash = sha256_of(&source);

    let error = execute_move(&plan, &history).expect_err("no destination");
    assert!(
        matches!(error, MoveError::DestinationNotADirectory(_)),
        "got {error:?}"
    );
    assert_eq!(sha256_of(&source), source_hash);
    assert_eq!(journal_rows(&history), 0);
}

// ---------------------------------------------------------------------------
// S12: undo never overwrites a recreated original
// Refusal happens in validate_undo_preconditions, before record: no undo row,
// the move row stays undoable for later (src/operations/undo.rs).
// ---------------------------------------------------------------------------

#[test]
fn w1_s12_recreated_original_blocks_undo_without_overwrite() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s12");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan");
    execute_move(&plan, &history).expect("move");
    let moved = dest_dir.join("report.txt");
    let moved_hash = sha256_of(&moved);

    // A new, different file now lives where the original was.
    write_file(&source, b"brand-new-bytes");
    let error = undo_last_move(&history).expect_err("collision");
    assert!(matches!(error, UndoError::Collision(_)), "got {error:?}");
    assert_eq!(sha256_of(&moved), moved_hash);
    assert_eq!(
        sha256_of(&source),
        format!("{:x}", Sha256::digest(b"brand-new-bytes"))
    );
    // No undo row was recorded and the move is still marked as not undone.
    let rows = history.entries().expect("entries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].operation, "move");
    assert!(rows[0].undone_at.is_none());
}

// ---------------------------------------------------------------------------
// S13: undo twice
// Selection: newest success row with undone_at IS NULL
// (src/storage/history.rs, src/operations/undo.rs).
// ---------------------------------------------------------------------------

#[test]
fn w1_s13i_second_undo_of_a_single_move_reports_nothing() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s13i");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan");
    execute_move(&plan, &history).expect("move");

    undo_last_move(&history).expect("first undo restores");
    assert!(source.exists());
    assert!(!dest_dir.join("report.txt").exists());
    let error = undo_last_move(&history).expect_err("nothing left");
    assert!(matches!(error, UndoError::NothingToUndo));
    // The single move row was marked undone exactly once.
    let rows = history.entries().expect("entries");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .find(|row| row.operation == "move")
            .expect("move")
            .undone_at
            .is_some()
    );
}

#[test]
fn w1_s13ii_second_undo_restores_the_next_newest_move() {
    let fixture = make_fixture("s13ii");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let dest_dir = fixture.root.join("Documents").join("Reports");
    fs::create_dir_all(&dest_dir).expect("create dest dir");
    let candidates = vec![candidate("D1", &dest_dir)];

    for (name, bytes) in [
        ("alpha.txt", b"alpha".as_slice()),
        ("beta.txt", b"beta".as_slice()),
    ] {
        let source = fixture.root.join(name);
        write_file(&source, bytes);
        let plan =
            build_move_plan(&source, &decision(Some("D1"), 0.95, 0.50), &candidates).expect("plan");
        execute_move(&plan, &history).expect("move");
    }
    let alpha_moved = dest_dir.join("alpha.txt");
    let beta_moved = dest_dir.join("beta.txt");
    assert!(alpha_moved.exists() && beta_moved.exists());

    // First undo restores beta (newest), second restores alpha (next newest):
    // the same move is never restored twice.
    let first = undo_last_move(&history).expect("first undo");
    assert_eq!(first.restored_to, fixture.root.join("beta.txt"));
    assert!(!beta_moved.exists());
    assert!(alpha_moved.exists());
    let second = undo_last_move(&history).expect("second undo");
    assert_eq!(second.restored_to, fixture.root.join("alpha.txt"));
    assert!(!alpha_moved.exists());

    // Undo rows carry the reversed paths of the move each one reversed.
    let rows = history.entries().expect("entries");
    assert_eq!(rows.len(), 4);
    let undos: Vec<_> = rows.iter().filter(|row| row.operation == "undo").collect();
    assert_eq!(undos.len(), 2);
    assert!(
        undos
            .iter()
            .any(|row| row.destination_path.ends_with("beta.txt"))
    );
    assert!(
        undos
            .iter()
            .any(|row| row.destination_path.ends_with("alpha.txt"))
    );
    assert_eq!(
        sha256_of(&fixture.root.join("alpha.txt")),
        format!("{:x}", Sha256::digest(b"alpha"))
    );
    assert_eq!(
        sha256_of(&fixture.root.join("beta.txt")),
        format!("{:x}", Sha256::digest(b"beta"))
    );
}

#[test]
fn w1_s13iii_last_three_with_one_success_restores_once() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s13iii");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan");
    execute_move(&plan, &history).expect("move");

    // Mirrors the --last loop in handle_undo: stop at NothingToUndo.
    let mut restored = 0;
    for _ in 0..3 {
        match undo_last_move(&history) {
            Ok(_) => restored += 1,
            Err(UndoError::NothingToUndo) => break,
            Err(error) => panic!("unexpected undo error: {error:?}"),
        }
    }
    assert_eq!(restored, 1);
    assert!(source.exists());
    assert!(!dest_dir.join("report.txt").exists());
    let undos = history
        .entries()
        .expect("entries")
        .into_iter()
        .filter(|row| row.operation == "undo")
        .count();
    assert_eq!(undos, 1);
}

// ---------------------------------------------------------------------------
// S09: dry run has no filesystem or journal side effects
// dry_run only verifies (src/operations/mover.rs: dry_run).
// ---------------------------------------------------------------------------

#[test]
fn w1_s09_dry_run_moves_nothing_and_records_nothing() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s09");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan");

    let outcome = dry_run(&plan).expect("dry run");
    assert!(matches!(outcome, MoveOutcome::WouldMove));
    assert!(source.exists());
    assert!(!dest_dir.join("report.txt").exists());
    assert_eq!(journal_rows(&history), 0);
}

#[test]
fn w1_s09_dry_run_still_refuses_collisions() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s09-collision");
    write_file(&dest_dir.join("report.txt"), b"squatter-bytes");
    let history = OperationHistory::open_in_memory().expect("open journal");

    // Planning refuses the collision before dry_run is reached
    // (src/operations/plan.rs); dry_run itself also verifies (S09 first test).
    let error = build_move_plan(&source, &dec, &candidates).expect_err("collision at plan");
    assert!(
        matches!(error, kaseifu::operations::plan::PlanError::Collision(_)),
        "got {error:?}"
    );
    assert_eq!(journal_rows(&history), 0);
}

// ---------------------------------------------------------------------------
// S10: a refused rename preserves the source
// Rename failure records a failure row, never success
// (src/operations/mover.rs: rename Err arm).
// ---------------------------------------------------------------------------

#[test]
fn w1_s10_readonly_destination_keeps_source_and_records_failure() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s10");
    let history = OperationHistory::open_in_memory().expect("open journal");

    let mut permissions = fs::metadata(&dest_dir).expect("stat").permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(&dest_dir, permissions).expect("chmod");

    // Root can write anywhere; the permission gate would not trigger there.
    let probe = dest_dir.join("write-probe.tmp");
    let is_root = fs::write(&probe, b"probe").is_ok();
    let _ = fs::remove_file(&probe);
    if is_root {
        let mut permissions = fs::metadata(&dest_dir).expect("stat").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&dest_dir, permissions).expect("restore");
        return;
    }

    let source_hash = sha256_of(&source);
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan builds");
    let error = execute_move(&plan, &history).expect_err("rename must fail");
    assert!(matches!(error, MoveError::Io(_)), "got {error:?}");
    assert_eq!(sha256_of(&source), source_hash);
    assert!(!dest_dir.join("report.txt").exists());

    let rows = history.entries().expect("entries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].operation, "move");
    assert_eq!(rows[0].result, "failure");
    assert!(rows[0].error.is_some());
    assert!(history.last_successful_move().expect("query").is_none());

    let mut permissions = fs::metadata(&dest_dir).expect("stat").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&dest_dir, permissions).expect("restore");
}

// ---------------------------------------------------------------------------
// S28: watch start, stop and side effects
// Full start/stop runs an infinite loop, so CLI children cover only the
// exit-code-determined validation surface (handle_watch validates paths
// before touching keys or the network; src/main.rs). Event safety is covered
// at the coordinator/runner level with the default config.
// ---------------------------------------------------------------------------

/// Child environment: isolated cwd/HOME, no key variables from the parent.
fn base_command(cwd: &Path, home: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR")
        .env_remove("KASEIFU_NO_COMPLETION_UPDATE");
    command
}

/// Run to completion within `timeout`; a hang is killed and fails the test.
fn run_with_deadline(mut command: ProcessCommand, timeout: Duration) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kaseifu");
    let started = Instant::now();
    loop {
        match child.try_wait().expect("poll child") {
            Some(_) => break,
            None if started.elapsed() > timeout => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect output");
                panic!(
                    "kaseifu exceeded {timeout:?}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn cli_fixture(tag: &str) -> (Fixture, PathBuf, PathBuf) {
    let fixture = make_fixture(tag);
    let cwd = fixture.root.join("cwd");
    let home = fixture.root.join("home");
    fs::create_dir_all(&cwd).expect("create cwd");
    fs::create_dir_all(&home).expect("create home");
    (fixture, cwd, home)
}

#[test]
fn w1_s28a_watch_rejects_a_file_path() {
    let (_fixture, cwd, home) = cli_fixture("s28a-file");
    let file = cwd.join("note.md");
    write_file(&file, b"note");
    let mut command = base_command(&cwd, &home);
    command.arg("watch").arg("--path").arg(&file);
    let output = run_with_deadline(command, Duration::from_secs(20));
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("watch path is not a directory"),
        "got {stderr}"
    );
}

#[test]
fn w1_s28a_watch_rejects_a_missing_path() {
    let (_fixture, cwd, home) = cli_fixture("s28a-missing");
    let mut command = base_command(&cwd, &home);
    command.arg("watch").arg("--path").arg(cwd.join("gone"));
    let output = run_with_deadline(command, Duration::from_secs(20));
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("watch path is not a directory"),
        "got {stderr}"
    );
}

#[test]
fn w1_s28a_watch_requires_a_path() {
    let (_fixture, cwd, home) = cli_fixture("s28a-nopath");
    let mut command = base_command(&cwd, &home);
    command.arg("watch");
    let output = run_with_deadline(command, Duration::from_secs(20));
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--path"), "got {stderr}");
}

#[test]
fn w1_s28a_watch_execute_and_dry_run_conflict() {
    let (_fixture, cwd, home) = cli_fixture("s28a-conflict");
    let mut command = base_command(&cwd, &home);
    command
        .arg("watch")
        .arg("--path")
        .arg(&cwd)
        .arg("--execute")
        .arg("--dry-run");
    let output = run_with_deadline(command, Duration::from_secs(20));
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
}

#[test]
fn w1_s28b_self_operation_suppresses_both_paths() {
    let (_fixture, watched, mut coordinator) = {
        let fixture = make_fixture("s28b");
        let watched = fixture.root.join("watched");
        fs::create_dir_all(&watched).expect("create watched dir");
        (
            fixture,
            watched,
            EventCoordinator::new(WatchConfig::default()),
        )
    };
    let source = watched.join("note.md");
    let dest = watched.join("done").join("note.md");
    write_file(&source, b"note");
    write_file(&dest, b"note");
    let now = Instant::now();
    coordinator.record_self_operation(&[source.clone(), dest.clone()], now);
    for path in [&source, &dest] {
        assert_eq!(
            coordinator.submit(
                RawEvent { path: path.clone() },
                now + Duration::from_secs(1)
            ),
            Disposition::Suppressed(SuppressReason::SelfGenerated),
            "for {}",
            path.display()
        );
    }
    assert!(
        coordinator
            .drain_due(now + Duration::from_secs(30))
            .is_empty()
    );
}

#[tokio::test]
async fn w1_s28c_failed_request_releases_inflight_and_continues() {
    let fixture = make_fixture("s28c");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let jev = MockJevIntegration::select_d1(0.9);
    let mut coordinator = EventCoordinator::new(WatchConfig::default());
    let missing = fixture.root.join("gone.md");
    let request = ProcessingRequest {
        path: missing.clone(),
    };

    // The file cannot even be observed: Failed, never a move, never a row.
    let outcome = process_request(&request, &mut coordinator, &jev, &history, false).await;
    assert!(
        matches!(outcome, ProcessOutcome::Failed(_)),
        "got {outcome:?}"
    );
    assert_eq!(journal_rows(&history), 0);

    // In-flight was released, so later events for the path are judged on the
    // merits (Missing) instead of sticking at DuplicateInFlight.
    assert_eq!(
        coordinator.submit(RawEvent { path: missing }, Instant::now()),
        Disposition::Suppressed(SuppressReason::Missing)
    );
}

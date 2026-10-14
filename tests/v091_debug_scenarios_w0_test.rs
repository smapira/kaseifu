//! Wave W0 debug scenarios (plan `20261014h`): S05, S01, S04, S02, S06.
//!
//! Every assertion is grounded in current behavior (`file:line` in comments),
//! not in `status: DESIGNED` copies. Fixtures live under the crate `target/`
//! directory and are removed on drop; journals are in-memory. No user files,
//! no live Jev, no network.

use kaseifu::classifier::jev::JevResponse;
use kaseifu::classifier::mock_jev::MockJevIntegration;
use kaseifu::classifier::pipeline::classify_with_search;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::JevDecision;
use kaseifu::operations::mover::{MoveOutcome, execute_move};
use kaseifu::operations::plan::build_move_plan;
use kaseifu::operations::undo::{UndoError, undo_last_move};
use kaseifu::storage::history::OperationHistory;
use kaseifu::watch::coordinator::{
    Disposition, EventCoordinator, RawEvent, SuppressReason, WatchConfig,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
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

/// Source file plus one empty candidate directory, with an AutoMove decision.
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

fn pending_rows(history: &OperationHistory) -> usize {
    history.pending_entries().expect("read pending").len()
}

// ---------------------------------------------------------------------------
// S05: crash between rename and journal finalization
// mover order: verify, record_pending, rename, mark (src/operations/mover.rs)
// ---------------------------------------------------------------------------

#[test]
fn w0_s05a_pending_row_is_detectable_and_not_undoable() {
    let fixture = make_fixture("s05a");
    let source = fixture.root.join("report.txt");
    write_file(&source, b"payload");
    let dest = fixture
        .root
        .join("Documents")
        .join("Reports")
        .join("report.txt");
    let history = OperationHistory::open_in_memory().expect("open journal");

    // Interruption right after record_pending: the row exists, the file did
    // not move (src/storage/history.rs: record_pending / pending_entries).
    let _id = history
        .record_pending("move", &source, &dest, Some("D1"), Some(0.95), Some(0.50))
        .expect("record pending");
    assert!(source.exists());
    assert!(!dest.exists());
    assert_eq!(pending_rows(&history), 1);
    // A pending move is never an undo target (last_successful_move requires
    // result='success'; src/storage/history.rs).
    assert!(history.last_successful_move().expect("query").is_none());
    assert_eq!(journal_rows(&history), 1);
}

#[test]
fn w0_s05b_moved_but_pending_is_detectable_and_not_undoable() {
    let fixture = make_fixture("s05b");
    let source = fixture.root.join("report.txt");
    write_file(&source, b"payload");
    let dest_dir = fixture.root.join("Documents").join("Reports");
    fs::create_dir_all(&dest_dir).expect("create dest dir");
    let dest = dest_dir.join("report.txt");
    let history = OperationHistory::open_in_memory().expect("open journal");

    // Interruption right after rename: replay the observable state manually
    // (record then rename, no mark_success). R1 holds: detection only.
    let _id = history
        .record_pending("move", &source, &dest, Some("D1"), Some(0.95), Some(0.50))
        .expect("record pending");
    fs::rename(&source, &dest).expect("replay rename");
    assert!(!source.exists());
    assert!(dest.exists());
    assert_eq!(
        sha256_of(&dest),
        format!("{:x}", Sha256::digest(b"payload"))
    );
    assert_eq!(pending_rows(&history), 1);
    assert!(history.last_successful_move().expect("query").is_none());
}

#[test]
fn w0_s05c_verify_failure_leaves_no_row() {
    let fixture = make_fixture("s05c");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let missing = fixture.root.join("gone.txt");
    let dest_dir = fixture.root.join("Documents").join("Reports");
    fs::create_dir_all(&dest_dir).expect("create dest dir");
    let candidates = vec![candidate("D1", &dest_dir)];
    // Interruption before record: planning itself refuses the missing source
    // (src/operations/plan.rs), so record_pending is never reached and no row
    // is left. The execute-time verify (src/operations/mover.rs) is covered by
    // the S04/S11 race tests, where the plan succeeds first.
    let error = build_move_plan(&missing, &decision(Some("D1"), 0.95, 0.50), &candidates)
        .expect_err("missing source");
    assert!(matches!(
        error,
        kaseifu::operations::plan::PlanError::SourceMissing(_)
    ));
    assert_eq!(journal_rows(&history), 0);
}

#[test]
fn w0_s05d_undo_side_interruption_never_double_restores() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s05d");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let plan = build_move_plan(&source, &dec, &candidates).expect("plan");
    execute_move(&plan, &history).expect("move");
    let moved = dest_dir.join("report.txt");
    assert!(moved.exists());

    // Undo interrupted after its rename: pending undo row exists, the original
    // move is still the undo target, the file is already back.
    let move_id = history
        .entries()
        .expect("entries")
        .into_iter()
        .find(|row| row.operation == "move")
        .expect("move row")
        .id;
    let undo_id = history
        .record_pending("undo", &moved, &source, Some("D1"), Some(0.95), Some(0.50))
        .expect("record undo pending");
    fs::rename(&moved, &source).expect("replay undo rename");
    assert_eq!(pending_rows(&history), 1);
    assert!(history.last_successful_move().expect("query").is_some());

    // Completion marks both rows; a later undo finds nothing to restore.
    history.mark_success(&undo_id).expect("mark undo success");
    history.mark_undone(&move_id).expect("mark move undone");
    assert_eq!(pending_rows(&history), 0);
    assert!(history.last_successful_move().expect("query").is_none());
    assert!(source.exists());
    assert!(!moved.exists());
    let error = undo_last_move(&history).expect_err("nothing left");
    assert!(matches!(error, UndoError::NothingToUndo));
}

// ---------------------------------------------------------------------------
// S01: move and undo consistency
// ---------------------------------------------------------------------------

#[test]
fn w0_s01_move_then_undo_restores_bytes_and_journal() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s01");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let before = sha256_of(&source);

    let plan = build_move_plan(&source, &dec, &candidates).expect("plan");
    let outcome = execute_move(&plan, &history).expect("move");
    assert!(matches!(outcome, MoveOutcome::Moved { .. }));
    let moved = dest_dir.join("report.txt");
    assert!(!source.exists());
    assert_eq!(sha256_of(&moved), before);

    let outcome = undo_last_move(&history).expect("undo");
    assert_eq!(outcome.restored_to, source);
    assert!(source.exists());
    assert!(!moved.exists());
    assert_eq!(sha256_of(&source), before);

    // Journal: one success move row (now undone) + one success undo row
    // (src/storage/history.rs, src/operations/undo.rs).
    let rows = history.entries().expect("entries");
    assert_eq!(rows.len(), 2);
    let move_row = rows
        .iter()
        .find(|row| row.operation == "move")
        .expect("move row");
    let undo_row = rows
        .iter()
        .find(|row| row.operation == "undo")
        .expect("undo row");
    assert_eq!(move_row.result, "success");
    assert_eq!(undo_row.result, "success");
    assert!(move_row.undone_at.is_some());
}

// ---------------------------------------------------------------------------
// S04: destination collision is refused
// Collision is detected in verify, before record_pending, so no row is left
// (src/operations/mover.rs: verify_preconditions, execute_move_inner).
// ---------------------------------------------------------------------------

#[test]
fn w0_s04_collision_refuses_and_preserves_both_files() {
    let (_fixture, source, dest_dir, dec, candidates) = automove_setup("s04");
    let squatter = dest_dir.join("report.txt");
    write_file(&squatter, b"squatter-bytes");
    let history = OperationHistory::open_in_memory().expect("open journal");
    let source_hash = sha256_of(&source);
    let squatter_hash = sha256_of(&squatter);

    // Planning itself refuses the pre-existing collision
    // (src/operations/plan.rs), so execute is never reached and no row is
    // left. The post-plan race window is covered by S11a at execute level.
    let error = build_move_plan(&source, &dec, &candidates).expect_err("collision");
    assert!(
        matches!(error, kaseifu::operations::plan::PlanError::Collision(_)),
        "got {error:?}"
    );
    assert_eq!(sha256_of(&source), source_hash);
    assert_eq!(sha256_of(&squatter), squatter_hash);
    assert_eq!(journal_rows(&history), 0);
}

// ---------------------------------------------------------------------------
// S02: unknown destination ids are rejected
// Validation lives in the domain layer, the adapter passes answers through
// (src/models/decision.rs: validate_decision, src/classifier/pipeline.rs).
// ---------------------------------------------------------------------------

#[test]
fn w0_s02b_validate_decision_rejects_unoffered_ids() {
    let fixture = make_fixture("s02b");
    let dir_a = fixture.root.join("Alpha");
    let dir_b = fixture.root.join("Beta");
    fs::create_dir_all(&dir_a).expect("mkdir");
    fs::create_dir_all(&dir_b).expect("mkdir");
    let target = fixture.root.join("note.txt");
    write_file(&target, b"note");
    let raw = kaseifu::context::builder::build(&target).expect("observe");
    let normalized = kaseifu::normalize::normalize(raw).expect("normalize");
    let candidates = vec![candidate("D1", &dir_a), candidate("D2", &dir_b)];
    let context =
        kaseifu::models::decision::DecisionContext::from_normalized(&normalized, &candidates);

    // Unknown id, path form and empty string are all rejected ...
    for bad in ["D999", "/Users/someone/Desktop/evil", ""] {
        assert!(
            context.validate_decision(bad).is_err(),
            "{bad} must be rejected"
        );
    }
    // ... while offered ids and NONE pass.
    assert_eq!(context.validate_decision("D1").expect("D1"), "D1");
    assert_eq!(context.validate_decision("D2").expect("D2"), "D2");
    assert_eq!(context.validate_decision("NONE").expect("NONE"), "NONE");
}

#[tokio::test]
async fn w0_s02a_pipeline_rejects_d999_without_fallback() {
    let fixture = make_fixture("s02a");
    let dir_a = fixture.root.join("Alpha");
    let dir_b = fixture.root.join("Beta");
    fs::create_dir_all(&dir_a).expect("mkdir");
    fs::create_dir_all(&dir_b).expect("mkdir");
    let target = fixture.root.join("report.txt");
    write_file(&target, b"quarterly report alpha beta");
    let hit_a = dir_a.join("similar-a.md");
    let hit_b = dir_b.join("similar-b.md");
    write_file(&hit_a, b"similar a");
    write_file(&hit_b, b"similar b");

    // Spotlight boundary is injected; every other stage is production code.
    let hits = vec![
        hit_a.to_string_lossy().into_owned(),
        hit_b.to_string_lossy().into_owned(),
    ];
    let search = move |_query: &str, _limit: usize| Ok::<Vec<String>, anyhow::Error>(hits.clone());

    let mut probabilities = HashMap::new();
    probabilities.insert("D999".to_string(), 0.95);
    probabilities.insert("D1".to_string(), 0.02);
    let jev = MockJevIntegration::configured_decision(JevResponse {
        destination_id: Some("D999".to_string()),
        probabilities,
        ..JevResponse::default()
    });

    let error = classify_with_search(&target, &jev, search)
        .await
        .expect_err("D999 must be rejected");
    let message = format!("{error:?}");
    assert!(
        message.contains("rejected Jev decision") && message.contains("D999"),
        "got {message}"
    );
    // No fallback: the call fails instead of returning NONE or another id.
}

// ---------------------------------------------------------------------------
// S06: watch debounce, dedupe and stability (default config)
// Coordinator rules: src/watch/coordinator.rs (submit, drain_due,
// begin/finish_processing, record_self_operation).
// ---------------------------------------------------------------------------

fn watch_fixture(tag: &str) -> (Fixture, PathBuf, EventCoordinator) {
    let fixture = make_fixture(tag);
    let watched = fixture.root.join("watched");
    fs::create_dir_all(&watched).expect("create watched dir");
    let coordinator = EventCoordinator::new(WatchConfig::default());
    (fixture, watched, coordinator)
}

fn watched_file(watched: &Path, name: &str) -> PathBuf {
    let path = watched.join(name);
    write_file(&path, b"v1");
    path
}

#[test]
fn w0_s06e_default_config_values_are_pinned() {
    let config = WatchConfig::default();
    assert_eq!(config.debounce, Duration::from_millis(500));
    assert_eq!(config.stability, Duration::from_secs(1));
    assert_eq!(config.self_suppression, Duration::from_secs(10));
    assert!(
        config
            .managed_paths
            .contains(&kaseifu::storage::history::journal_path())
    );
}

#[test]
fn w0_s06a_rapid_events_coalesce_into_one_request() {
    let (_fixture, watched, mut coordinator) = watch_fixture("s06a");
    let path = watched_file(&watched, "note.md");
    let t0 = Instant::now();
    for millis in [0, 100, 200] {
        let disposition = coordinator.submit(
            RawEvent { path: path.clone() },
            t0 + Duration::from_millis(millis),
        );
        assert_eq!(disposition, Disposition::Accepted);
    }
    // Debounce window extends with the last event; the first drain only takes
    // a snapshot, the drain after stability yields exactly one request.
    assert!(
        coordinator
            .drain_due(t0 + Duration::from_millis(800))
            .is_empty()
    );
    let requests = coordinator.drain_due(t0 + Duration::from_millis(800) + Duration::from_secs(1));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, path);
}

#[test]
fn w0_s06b_changing_file_is_not_processed_until_stable() {
    let (_fixture, watched, mut coordinator) = watch_fixture("s06b");
    let path = watched_file(&watched, "draft.md");
    let t0 = Instant::now();
    assert_eq!(
        coordinator.submit(RawEvent { path: path.clone() }, t0),
        Disposition::Accepted
    );
    // Snapshot taken, no request yet.
    assert!(
        coordinator
            .drain_due(t0 + Duration::from_millis(600))
            .is_empty()
    );
    // The file keeps changing: re-observed, still no request after 1s.
    write_file(&path, b"v1-plus-more-bytes");
    assert!(
        coordinator
            .drain_due(t0 + Duration::from_millis(600) + Duration::from_secs(1))
            .is_empty()
    );
    // Quiescent for over a second: exactly one request.
    let requests = coordinator.drain_due(
        t0 + Duration::from_millis(600) + Duration::from_secs(1) + Duration::from_millis(1100),
    );
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, path);
}

#[test]
fn w0_s06c_inflight_and_self_events_are_suppressed() {
    let (_fixture, watched, mut coordinator) = watch_fixture("s06c");
    let path = watched_file(&watched, "note.md");
    let moved = watched.join("moved-note.md");
    write_file(&moved, b"v1");
    let t0 = Instant::now();

    coordinator.begin_processing(&path);
    assert_eq!(
        coordinator.submit(RawEvent { path: path.clone() }, t0),
        Disposition::Suppressed(SuppressReason::DuplicateInFlight)
    );
    coordinator.finish_processing(&path);

    // A move we performed suppresses events for both paths (source and
    // destination are registered together; src/watch/runner.rs).
    coordinator.record_self_operation(&[path.clone(), moved.clone()], t0);
    assert_eq!(
        coordinator.submit(RawEvent { path: path.clone() }, t0 + Duration::from_secs(1)),
        Disposition::Suppressed(SuppressReason::SelfGenerated)
    );
    assert_eq!(
        coordinator.submit(
            RawEvent {
                path: moved.clone()
            },
            t0 + Duration::from_secs(1)
        ),
        Disposition::Suppressed(SuppressReason::SelfGenerated)
    );
    // Nothing was queued for the suppressed paths.
    assert!(
        coordinator
            .drain_due(t0 + Duration::from_secs(30))
            .is_empty()
    );
}

#[test]
fn w0_s06d_missing_directory_and_managed_paths_are_suppressed() {
    let (fixture, watched, mut coordinator) = watch_fixture("s06d");
    let t0 = Instant::now();
    assert_eq!(
        coordinator.submit(
            RawEvent {
                path: watched.join("gone.md")
            },
            t0
        ),
        Disposition::Suppressed(SuppressReason::Missing)
    );
    assert_eq!(
        coordinator.submit(
            RawEvent {
                path: watched.clone()
            },
            t0
        ),
        Disposition::Suppressed(SuppressReason::Directory)
    );

    // Managed data (e.g. the journal directory) is never processed.
    let managed_dir = fixture.root.join("managed");
    fs::create_dir_all(&managed_dir).expect("mkdir");
    let managed_file = managed_dir.join("data.db");
    write_file(&managed_file, b"journal");
    let mut managed = EventCoordinator::new(WatchConfig {
        managed_paths: vec![managed_dir],
        ..WatchConfig::default()
    });
    assert_eq!(
        managed.submit(RawEvent { path: managed_file }, t0),
        Disposition::Suppressed(SuppressReason::ManagedData)
    );
}

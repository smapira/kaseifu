//! Phase 5 - Safe execution tests.
//!
//! All filesystem tests use temporary fixtures under the crate's own
//! `target/` directory; no user file is ever touched.

use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::{DecisionAction, JevDecision};
use kaseifu::operations::mover::{MoveError, MoveOutcome, dry_run, execute_move};
use kaseifu::operations::plan::{PlanError, build_move_plan};
use kaseifu::operations::undo::{UndoError, undo_last_move};
use kaseifu::policy;
use kaseifu::storage::history::OperationHistory;

use std::fs;
use std::path::{Path, PathBuf};

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
        .join(format!("phase5-fixture-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create fixture root");
    Fixture { root }
}

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write fixture file");
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

/// A source file plus one candidate directory (no collision).
fn plan_fixture(tag: &str) -> (Fixture, PathBuf, PathBuf) {
    let fixture = make_fixture(tag);
    let source = fixture.root.join("report.txt");
    write_file(&source, "payload");
    let destination_directory = fixture.root.join("Documents").join("Reports");
    fs::create_dir_all(&destination_directory).expect("create candidate dir");
    (fixture, source, destination_directory)
}

// ---------------------------------------------------------------------------
// Cycle 1: Policy
// ---------------------------------------------------------------------------

#[test]
fn phase5_policy_auto_moves_at_the_documented_thresholds() {
    let action = policy::decision::evaluate(&decision(Some("D1"), 0.85, 0.40));

    match action {
        DecisionAction::AutoMove(id) => assert_eq!(id, "D1", "payload must be a candidate ID"),
        other => panic!("expected AutoMove at the thresholds, got {other:?}"),
    }
}

#[test]
fn phase5_policy_reviews_when_margin_is_below_the_auto_threshold() {
    // High confidence but an unclear margin is not enough to auto-move.
    let action = policy::decision::evaluate(&decision(Some("D1"), 0.90, 0.10));

    match action {
        DecisionAction::Review(id) => assert_eq!(id, "D1"),
        other => panic!("expected Review, got {other:?}"),
    }
}

#[test]
fn phase5_policy_reviews_at_the_confidence_boundary() {
    let action = policy::decision::evaluate(&decision(Some("D1"), 0.60, 0.0));

    assert!(
        matches!(action, DecisionAction::Review(_)),
        "expected Review at 0.60, got {action:?}"
    );
}

#[test]
fn phase5_policy_leaves_below_the_review_threshold() {
    // The Phase 4E live decision (0.57 / 0.14) must be a Leave: no move.
    let action = policy::decision::evaluate(&decision(Some("D1"), 0.57, 0.14));

    assert!(
        matches!(action, DecisionAction::Leave { .. }),
        "expected Leave, got {action:?}"
    );
}

#[test]
fn phase5_policy_leaves_when_no_candidate_was_selected() {
    for destination in [None, Some("NONE")] {
        let action = policy::decision::evaluate(&decision(destination, 0.99, 0.99));
        assert!(
            matches!(action, DecisionAction::Leave { .. }),
            "expected Leave for {destination:?}, got {action:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Cycle 1: MovePlan
// ---------------------------------------------------------------------------

#[test]
fn phase5_plan_builds_from_a_validated_candidate() {
    let (fixture, source, destination_directory) = plan_fixture("plan-valid");
    let candidates = vec![candidate("D1", &destination_directory)];

    let plan = build_move_plan(&source, &decision(Some("D1"), 0.90, 0.50), &candidates)
        .expect("a validated auto-move candidate must produce a plan");

    assert_eq!(plan.source, source);
    assert_eq!(plan.destination_directory, destination_directory);
    assert_eq!(
        plan.destination_path,
        destination_directory.join("report.txt")
    );
    assert_eq!(plan.candidate_id, "D1");
    assert_eq!(plan.confidence, 0.90);
    assert_eq!(plan.margin, 0.50);

    // Planning never mutates the filesystem.
    assert!(source.exists());
    assert!(fixture.root.is_dir());
}

#[test]
fn phase5_plan_rejects_unknown_candidate() {
    let (_fixture, source, destination_directory) = plan_fixture("plan-unknown");
    let candidates = vec![candidate("D1", &destination_directory)];

    let error = build_move_plan(&source, &decision(Some("D99"), 0.90, 0.50), &candidates)
        .expect_err("D99 was never offered");

    assert_eq!(error, PlanError::UnknownCandidate("D99".to_string()));
}

#[test]
fn phase5_plan_rejects_a_filesystem_path_as_selection() {
    let (_fixture, source, destination_directory) = plan_fixture("plan-path");
    let candidates = vec![candidate("D1", &destination_directory)];

    let error = build_move_plan(
        &source,
        &decision(Some("/Users/foo/Documents"), 0.99, 0.99),
        &candidates,
    )
    .expect_err("Jev may never supply a path");

    assert_eq!(
        error,
        PlanError::UnknownCandidate("/Users/foo/Documents".to_string())
    );
}

#[test]
fn phase5_plan_rejects_none_selection() {
    let (_fixture, source, destination_directory) = plan_fixture("plan-none");
    let candidates = vec![candidate("D1", &destination_directory)];

    let error = build_move_plan(&source, &decision(Some("NONE"), 0.99, 0.99), &candidates)
        .expect_err("NONE means no move");

    assert_eq!(error, PlanError::NotAutoMove);
}

#[test]
fn phase5_plan_rejects_review_decision() {
    let (_fixture, source, destination_directory) = plan_fixture("plan-review");
    let candidates = vec![candidate("D1", &destination_directory)];

    let error = build_move_plan(&source, &decision(Some("D1"), 0.70, 0.10), &candidates)
        .expect_err("a Review decision must not produce a plan");

    assert_eq!(error, PlanError::NotAutoMove);
}

#[test]
fn phase5_plan_rejects_leave_decision() {
    let (_fixture, source, destination_directory) = plan_fixture("plan-leave");
    let candidates = vec![candidate("D1", &destination_directory)];

    let error = build_move_plan(&source, &decision(Some("D1"), 0.20, 0.05), &candidates)
        .expect_err("a Leave decision must not produce a plan");

    assert_eq!(error, PlanError::NotAutoMove);
}

#[test]
fn phase5_plan_rejects_missing_source() {
    let (fixture, source, destination_directory) = plan_fixture("plan-source-missing");
    let candidates = vec![candidate("D1", &destination_directory)];
    fs::remove_file(&source).expect("remove source");
    let _ = fixture;

    let error = build_move_plan(&source, &decision(Some("D1"), 0.90, 0.50), &candidates)
        .expect_err("a missing source must be rejected");

    assert_eq!(error, PlanError::SourceMissing(source));
}

#[test]
fn phase5_plan_rejects_missing_destination_directory() {
    let (fixture, source, destination_directory) = plan_fixture("plan-dest-missing");
    let candidates = vec![candidate("D1", &destination_directory)];
    fs::remove_dir_all(&destination_directory).expect("remove candidate dir");
    let _ = fixture;

    let error = build_move_plan(&source, &decision(Some("D1"), 0.90, 0.50), &candidates)
        .expect_err("a missing destination directory must be rejected");

    assert_eq!(
        error,
        PlanError::DestinationNotADirectory(destination_directory)
    );
}

#[test]
fn phase5_plan_rejects_collision_and_preserves_files() {
    let (_fixture, source, destination_directory) = plan_fixture("plan-collision");
    let existing = destination_directory.join("report.txt");
    write_file(&existing, "already here");
    let candidates = vec![candidate("D1", &destination_directory)];

    let error = build_move_plan(&source, &decision(Some("D1"), 0.90, 0.50), &candidates)
        .expect_err("a collision must never overwrite");

    assert_eq!(error, PlanError::Collision(existing.clone()));

    // Both files are untouched.
    assert_eq!(fs::read_to_string(&source).unwrap(), "payload");
    assert_eq!(fs::read_to_string(&existing).unwrap(), "already here");
}

// ---------------------------------------------------------------------------
// Cycle 2: Journal (record before the mutation, result afterwards)
// ---------------------------------------------------------------------------

#[test]
fn phase5_history_records_before_the_mutation() {
    let history = OperationHistory::open_in_memory().expect("open journal");

    let id = history
        .record_pending(
            "move",
            Path::new("/a/source.txt"),
            Path::new("/b/source.txt"),
            Some("D1"),
            Some(0.90),
            Some(0.50),
        )
        .expect("record pending");

    let entries = history.entries().expect("read journal");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, id);
    assert_eq!(entries[0].operation, "move");
    assert_eq!(entries[0].source_path, "/a/source.txt");
    assert_eq!(entries[0].destination_path, "/b/source.txt");
    assert_eq!(entries[0].candidate_id.as_deref(), Some("D1"));
    assert_eq!(entries[0].confidence, Some(0.90));
    assert_eq!(entries[0].margin, Some(0.50));
    assert_eq!(entries[0].result, "pending");
    assert!(entries[0].undone_at.is_none());

    // A pending operation is not undoable.
    assert!(history.last_successful_move().unwrap().is_none());
}

#[test]
fn phase5_history_marks_success_and_exposes_the_last_move() {
    let history = OperationHistory::open_in_memory().expect("open journal");
    let id = history
        .record_pending(
            "move",
            Path::new("/a/source.txt"),
            Path::new("/b/source.txt"),
            Some("D2"),
            Some(0.91),
            Some(0.60),
        )
        .unwrap();
    history.mark_success(&id).expect("mark success");

    let last = history
        .last_successful_move()
        .unwrap()
        .expect("a successful move must be undoable");
    assert_eq!(last.id, id);
    assert_eq!(last.result, "success");
    assert_eq!(last.candidate_id.as_deref(), Some("D2"));
}

#[test]
fn phase5_history_marks_failure_with_a_detail() {
    let history = OperationHistory::open_in_memory().expect("open journal");
    let id = history
        .record_pending(
            "move",
            Path::new("/a/source.txt"),
            Path::new("/b/source.txt"),
            Some("D1"),
            Some(0.90),
            Some(0.50),
        )
        .unwrap();
    history
        .mark_failure(&id, "permission denied")
        .expect("mark failure");

    let entries = history.entries().unwrap();
    assert_eq!(entries[0].result, "failure");
    assert_eq!(entries[0].error.as_deref(), Some("permission denied"));
    // A failure is never undoable and never a success.
    assert!(history.last_successful_move().unwrap().is_none());
}

#[test]
fn phase5_history_marks_undone_moves_as_no_longer_undoable() {
    let history = OperationHistory::open_in_memory().expect("open journal");
    let id = history
        .record_pending(
            "move",
            Path::new("/a/source.txt"),
            Path::new("/b/source.txt"),
            Some("D1"),
            Some(0.90),
            Some(0.50),
        )
        .unwrap();
    history.mark_success(&id).unwrap();
    assert!(history.last_successful_move().unwrap().is_some());

    history.mark_undone(&id).expect("mark undone");

    assert!(
        history.last_successful_move().unwrap().is_none(),
        "an undone move must not be undoable twice"
    );
    let entries = history.entries().unwrap();
    assert!(entries[0].undone_at.is_some());
}

// ---------------------------------------------------------------------------
// Cycle 3: Executor (dry run first, then the real move)
// ---------------------------------------------------------------------------

/// Build a valid plan for the standard fixture.
fn valid_plan(
    tag: &str,
) -> (
    Fixture,
    std::path::PathBuf,
    std::path::PathBuf,
    kaseifu::operations::plan::MovePlan,
) {
    let (fixture, source, destination_directory) = plan_fixture(tag);
    let candidates = vec![candidate("D1", &destination_directory)];
    let plan = build_move_plan(&source, &decision(Some("D1"), 0.90, 0.50), &candidates)
        .expect("valid plan");
    (fixture, source, destination_directory, plan)
}

#[test]
fn phase5_dry_run_reports_without_touching_anything() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("dry-run");
    let history = OperationHistory::open_in_memory().unwrap();

    let outcome = dry_run(&plan).expect("a valid plan must dry-run cleanly");

    assert_eq!(outcome, MoveOutcome::WouldMove);
    assert!(source.exists(), "dry run must not touch the source");
    assert!(
        !plan.destination_path.exists(),
        "dry run must not create the destination"
    );
    assert!(
        history.entries().unwrap().is_empty(),
        "dry run must not write to the journal"
    );
}

#[test]
fn phase5_execute_move_moves_and_journals() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("execute");
    let history = OperationHistory::open_in_memory().unwrap();

    let outcome = execute_move(&plan, &history).expect("move must succeed");
    let MoveOutcome::Moved { operation_id } = outcome else {
        panic!("expected a real move, got {outcome:?}");
    };

    // The file moved (content preserved, source gone).
    assert!(!source.exists());
    assert_eq!(
        fs::read_to_string(&plan.destination_path).unwrap(),
        "payload"
    );

    // The move is journaled and undoable.
    let entry = history
        .last_successful_move()
        .unwrap()
        .expect("a successful move must be journaled");
    assert_eq!(entry.id, operation_id);
    assert_eq!(entry.result, "success");
    assert_eq!(entry.candidate_id.as_deref(), Some("D1"));
    assert_eq!(entry.source_path, plan.source.to_string_lossy());
    assert_eq!(
        entry.destination_path,
        plan.destination_path.to_string_lossy()
    );
    assert_eq!(entry.confidence, Some(0.90));
    assert_eq!(entry.margin, Some(0.50));
}

#[test]
fn phase5_execute_move_refuses_collision_and_preserves_files() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("execute-collision");
    let history = OperationHistory::open_in_memory().unwrap();

    // Simulate a race: something appears at the destination after planning.
    write_file(&plan.destination_path, "squatter");

    let error = execute_move(&plan, &history).expect_err("collision must refuse execution");

    assert_eq!(error, MoveError::Collision(plan.destination_path.clone()));
    assert_eq!(fs::read_to_string(&source).unwrap(), "payload");
    assert_eq!(
        fs::read_to_string(&plan.destination_path).unwrap(),
        "squatter"
    );
    assert!(
        history.entries().unwrap().is_empty(),
        "a refused move must not write journal rows"
    );
}

#[test]
fn phase5_execute_move_fails_when_source_is_gone() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("execute-source-gone");
    let history = OperationHistory::open_in_memory().unwrap();
    fs::remove_file(&source).unwrap();

    let error = execute_move(&plan, &history).expect_err("a missing source must fail");

    assert_eq!(error, MoveError::SourceMissing(source.clone()));
    assert!(!plan.destination_path.exists());
    assert!(history.entries().unwrap().is_empty());
}

#[test]
fn phase5_execute_move_fails_when_destination_directory_is_gone() {
    let (_fixture, _source, destination_directory, plan) = valid_plan("execute-dest-gone");
    let history = OperationHistory::open_in_memory().unwrap();
    fs::remove_dir_all(&destination_directory).unwrap();

    let error = execute_move(&plan, &history).expect_err("a missing destination must fail");

    assert_eq!(
        error,
        MoveError::DestinationNotADirectory(destination_directory)
    );
    assert_eq!(fs::read_to_string(&plan.source).unwrap(), "payload");
    assert!(history.entries().unwrap().is_empty());
}

#[test]
fn phase5_execute_move_permission_failure_preserves_the_source() {
    use std::os::unix::fs::PermissionsExt;

    let (_fixture, source, destination_directory, plan) = valid_plan("execute-permission");
    let history = OperationHistory::open_in_memory().unwrap();

    // Make the destination directory read-only for the rename to fail.
    fs::set_permissions(&destination_directory, fs::Permissions::from_mode(0o555)).unwrap();

    let error = execute_move(&plan, &history).expect_err("a forbidden rename must fail");

    // Restore permissions before the fixture cleanup.
    fs::set_permissions(&destination_directory, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(matches!(error, MoveError::Io(_)), "got {error:?}");
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        "payload",
        "the source must survive a failed move"
    );
    assert!(!plan.destination_path.exists());

    // The failure is journaled as a failure, never as a success.
    assert!(history.last_successful_move().unwrap().is_none());
    let entries = history.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].result, "failure");
    assert!(entries[0].error.is_some());
}

// ---------------------------------------------------------------------------
// Cycle 4: Undo
// ---------------------------------------------------------------------------

#[test]
fn phase5_undo_restores_the_file_and_journals_the_undo() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("undo-restore");
    let history = OperationHistory::open_in_memory().unwrap();
    let MoveOutcome::Moved { operation_id } = execute_move(&plan, &history).unwrap() else {
        panic!("expected a real move");
    };

    let outcome = undo_last_move(&history).expect("a journaled move must be undoable");

    assert_eq!(outcome.undone_move_id, operation_id);
    assert_eq!(outcome.restored_to, source);
    assert_eq!(fs::read_to_string(&source).unwrap(), "payload");
    assert!(
        !plan.destination_path.exists(),
        "the moved file must be gone after undo"
    );

    // The undo is journaled, and the original move is marked undone.
    let entries = history.entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].operation, "undo");
    assert_eq!(entries[1].result, "success");
    assert!(entries[0].undone_at.is_some(), "original must be marked");
}

#[test]
fn phase5_undo_without_history_reports_nothing_to_undo() {
    let history = OperationHistory::open_in_memory().unwrap();

    let error = undo_last_move(&history).expect_err("nothing was moved");

    assert_eq!(error, UndoError::NothingToUndo);
}

#[test]
fn phase5_undo_refuses_collision_and_preserves_everything() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("undo-collision");
    let history = OperationHistory::open_in_memory().unwrap();
    execute_move(&plan, &history).unwrap();

    // Something appears at the original location after the move.
    write_file(&source, "squatter");

    let error = undo_last_move(&history).expect_err("undo must never overwrite");

    assert_eq!(error, UndoError::Collision(source.clone()));
    assert_eq!(fs::read_to_string(&source).unwrap(), "squatter");
    assert_eq!(
        fs::read_to_string(&plan.destination_path).unwrap(),
        "payload",
        "the moved file must be preserved"
    );
    assert_eq!(
        history.entries().unwrap().len(),
        1,
        "no undo row on refusal"
    );
}

#[test]
fn phase5_undo_fails_when_the_moved_file_is_gone() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("undo-missing");
    let history = OperationHistory::open_in_memory().unwrap();
    execute_move(&plan, &history).unwrap();
    fs::remove_file(&plan.destination_path).unwrap();

    let error = undo_last_move(&history).expect_err("nothing to restore");

    assert_eq!(
        error,
        UndoError::SourceMissing(plan.destination_path.clone())
    );
    assert!(!source.exists());
    assert_eq!(history.entries().unwrap().len(), 1);
}

#[test]
fn phase5_undo_is_not_repeatable() {
    let (_fixture, source, _destination_directory, plan) = valid_plan("undo-once");
    let history = OperationHistory::open_in_memory().unwrap();
    execute_move(&plan, &history).unwrap();

    undo_last_move(&history).expect("first undo succeeds");
    let second = undo_last_move(&history);

    assert_eq!(
        second.expect_err("a move must not be undone twice"),
        UndoError::NothingToUndo
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), "payload");
}

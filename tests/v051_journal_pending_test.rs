//! v0.5.1 pending-journal tests (R1/R2).
//!
//! An interrupted move leaves a `pending` row: the journal recorded the
//! mutation but never its result. These tests pin that the state is
//! *detectable* (`inspect_journal`, `doctor`, `undo`) and that nothing
//! repairs, rewrites or deletes it automatically. Every journal here is a
//! synthetic one in a temporary directory; the developer's real journal is
//! never opened.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use chrono::{Duration as ChronoDuration, Utc};
use kaseifu::storage::history::{OperationHistory, inspect_journal, inspect_journal_at};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-journal-pending-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn write_file(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("fixture dir");
    }
    std::fs::write(path, b"fixture").expect("fixture file");
}

/// Journal path the binary resolves for an isolated HOME.
fn isolated_journal(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("kaseifu.db")
}

fn run_with_deadline(mut command: ProcessCommand, timeout: Duration) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kaseifu spawns");
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect output");
                panic!(
                    "kaseifu exceeded {timeout:?}: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn run(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR");
    command.args(args);
    let output = run_with_deadline(command, Duration::from_secs(10));
    assert!(
        output.status.success(),
        "kaseifu {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Insert a `pending` row with an explicitly old timestamp (the file-state
/// columns point at real paths, so the reported state is deterministic).
fn insert_aged_pending(journal: &Path, age_hours: i64, source: &Path, destination: &Path) {
    let conn = rusqlite::Connection::open(journal).expect("open journal");
    conn.execute(
        "INSERT INTO operations (id, timestamp, operation, source_path, destination_path, result)
         VALUES (?1, ?2, 'move', ?3, ?4, 'pending')",
        rusqlite::params![
            format!("op-aged-{}", std::process::id()),
            (Utc::now() - ChronoDuration::hours(age_hours)).to_rfc3339(),
            source.to_string_lossy(),
            destination.to_string_lossy(),
        ],
    )
    .expect("insert aged pending row");
}

// ---------------------------------------------------------------------------
// Library level: read-only inspection
// ---------------------------------------------------------------------------

#[test]
fn v051_inspection_reports_a_pending_move_with_its_file_state() {
    let dir = temp_dir("inspect");
    let journal = dir.join("kaseifu.db");
    let history = OperationHistory::open(&journal).expect("open journal");

    let source = dir.join("source").join("IMG_100.jpeg");
    let destination = dir.join("albums").join("2024").join("IMG_100.jpeg");
    write_file(&destination);

    let id = history
        .record_pending(
            "move",
            &source,
            &destination,
            Some("D1"),
            Some(0.9),
            Some(0.5),
        )
        .expect("record pending");

    let inspection = inspect_journal(&journal)
        .expect("inspect")
        .expect("journal exists");
    assert_eq!(inspection.total_operations, 1);
    assert_eq!(inspection.pending.len(), 1);
    let row = &inspection.pending[0];
    assert_eq!(row.entry.id, id);
    assert_eq!(row.entry.result, "pending");
    assert!(!row.stale, "a fresh row may belong to a concurrent process");
    assert!(
        !row.source_exists,
        "the source is gone (the move may have run)"
    );
    assert!(row.destination_exists);
    assert!(
        row.age_seconds.is_some_and(|age| (0..60).contains(&age)),
        "fresh rows have a small age: {:?}",
        row.age_seconds
    );
}

#[test]
fn v051_inspection_marks_old_pending_rows_stale() {
    let dir = temp_dir("stale");
    let journal = dir.join("kaseifu.db");
    let history = OperationHistory::open(&journal).expect("open journal");
    history
        .record_pending(
            "move",
            &dir.join("source").join("IMG_100.jpeg"),
            &dir.join("albums").join("IMG_100.jpeg"),
            Some("D1"),
            Some(0.9),
            Some(0.5),
        )
        .expect("record pending");
    let just_before = Utc::now()
        + ChronoDuration::seconds(kaseifu::storage::history::PENDING_STALE_AFTER_SECS - 120);
    assert!(
        !inspect_journal_at(&journal, just_before)
            .expect("inspect")
            .expect("journal exists")
            .pending[0]
            .stale,
        "just under the threshold is not stale"
    );

    let later = Utc::now() + ChronoDuration::hours(2);
    let row = &inspect_journal_at(&journal, later)
        .expect("inspect")
        .expect("journal exists")
        .pending[0];
    assert!(row.stale, "a two-hour-old pending row is interrupted");
    assert!(row.age_seconds.is_some_and(|age| age >= 3600));
}

#[test]
fn v051_inspection_of_a_missing_journal_creates_nothing() {
    let dir = temp_dir("missing");
    let journal = dir.join("nested").join("kaseifu.db");

    assert!(inspect_journal(&journal).expect("inspect").is_none());
    assert!(!journal.exists(), "inspection must not create the journal");
    assert!(
        !dir.join("nested").exists(),
        "inspection must not create directories"
    );
}

#[test]
fn v051_inspection_of_a_foreign_file_is_an_error() {
    let dir = temp_dir("foreign");
    let journal = dir.join("not-a-journal.db");
    std::fs::write(&journal, b"this is not a sqlite database").expect("foreign file");

    let error = inspect_journal(&journal).expect_err("a foreign file is not a journal");
    assert!(!format!("{error:#}").is_empty());
    assert_eq!(
        std::fs::read(&journal).expect("read back"),
        b"this is not a sqlite database",
        "the foreign file is not modified"
    );
}

// ---------------------------------------------------------------------------
// CLI level: `doctor`
// ---------------------------------------------------------------------------

#[test]
fn v051_doctor_reports_a_fresh_pending_row_without_changing_the_journal() {
    let dir = temp_dir("doctor-pending");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    let journal = isolated_journal(&home);

    let history = OperationHistory::open(&journal).expect("open journal");
    let source = dir.join("inbox").join("IMG_300.jpeg");
    let destination = dir.join("albums").join("2024").join("IMG_300.jpeg");
    write_file(&destination);
    history
        .record_pending(
            "move",
            &source,
            &destination,
            Some("D1"),
            Some(0.9),
            Some(0.5),
        )
        .expect("record pending");
    drop(history);

    let before = std::fs::read(&journal).expect("read journal");
    let output = run(&dir, &home, &["doctor"]);
    let text = stdout_of(&output);

    assert!(text.contains("ok Journal:"), "{text}");
    assert!(text.contains("1 pending, 0 possibly interrupted"), "{text}");
    assert!(
        text.contains("move: source absent, destination present"),
        "the state of the pending file is reported: {text}"
    );

    let after = std::fs::read(&journal).expect("read journal");
    assert_eq!(before, after, "doctor must not modify the journal");
    assert!(destination.exists(), "doctor must not move the file");
}

#[test]
fn v051_doctor_fails_on_a_stale_pending_row() {
    let dir = temp_dir("doctor-stale");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    let journal = isolated_journal(&home);

    drop(OperationHistory::open(&journal).expect("create journal schema"));
    let destination = dir.join("albums").join("IMG_400.jpeg");
    write_file(&destination);
    insert_aged_pending(
        &journal,
        3,
        &dir.join("inbox").join("IMG_400.jpeg"),
        &destination,
    );

    let output = run(&dir, &home, &["doctor"]);
    let text = stdout_of(&output);

    assert!(text.contains("FAIL Journal:"), "{text}");
    assert!(text.contains("1 pending, 1 possibly interrupted"), "{text}");
    assert!(
        std::fs::metadata(&journal).is_ok_and(|metadata| metadata.len() > 0),
        "the journal is still there"
    );
    assert!(destination.exists(), "doctor must not move the file");
}

// ---------------------------------------------------------------------------
// CLI level: `undo`
// ---------------------------------------------------------------------------

#[test]
fn v051_undo_warns_about_pending_rows_and_never_repairs_them() {
    let dir = temp_dir("undo-pending");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    let journal = isolated_journal(&home);

    let history = OperationHistory::open(&journal).expect("open journal");

    // 1) An interrupted move: recorded as pending; its file may already be at
    // the destination (the rename succeeded but the result was never written).
    let interrupted_source = dir.join("interrupted").join("IMG_100.jpeg");
    let interrupted_target = dir.join("albums").join("2024").join("IMG_100.jpeg");
    write_file(&interrupted_target);
    let interrupted_id = history
        .record_pending(
            "move",
            &interrupted_source,
            &interrupted_target,
            Some("D1"),
            Some(0.9),
            Some(0.5),
        )
        .expect("record pending");

    // 2) A fully recorded successful move: undo is expected to restore it.
    let restored_source = dir.join("inbox").join("IMG_200.jpeg");
    std::fs::create_dir_all(restored_source.parent().expect("source parent")).expect("source dir");
    let restored_target = dir.join("albums").join("2024").join("IMG_200.jpeg");
    write_file(&restored_target);
    let success_id = history
        .record_pending(
            "move",
            &restored_source,
            &restored_target,
            Some("D1"),
            Some(0.9),
            Some(0.5),
        )
        .expect("record pending");
    history.mark_success(&success_id).expect("mark success");
    drop(history);

    let output = run(&dir, &home, &["undo"]);
    let text = stdout_of(&output);
    assert!(
        text.contains("warning: 1 pending journal row (1 move, 0 undo);"),
        "the pending row must be reported: {text}"
    );
    assert!(
        text.contains("interrupted moves may already be at the destination"),
        "{text}"
    );
    assert!(
        text.contains("not recovered automatically; run `kaseifu doctor` to inspect"),
        "{text}"
    );
    assert!(
        text.contains(&format!("restored: {}", restored_source.display())),
        "the successful move is still undone: {text}"
    );

    // The interrupted move is untouched (no auto-recovery).
    assert!(
        interrupted_target.exists(),
        "the moved file is never reversed"
    );
    assert!(!interrupted_source.exists());

    let history = OperationHistory::open(&journal).expect("open journal");
    let pending = history.pending_entries().expect("pending rows");
    assert_eq!(pending.len(), 1, "pending rows are never rewritten");
    assert_eq!(pending[0].id, interrupted_id);
    assert_eq!(pending[0].result, "pending");
    let undone = history
        .entries()
        .expect("entries")
        .into_iter()
        .find(|entry| entry.id == success_id)
        .expect("the undone move is still journaled");
    assert!(
        undone.undone_at.is_some(),
        "the successful move is marked undone as usual"
    );
    drop(history);

    // A second run reports the same pending row and still does not touch it.
    let second = run(&dir, &home, &["undo"]);
    let text = stdout_of(&second);
    assert!(text.contains("warning: 1 pending journal row"), "{text}");
    assert!(text.contains("nothing left to undo"), "{text}");
    let history = OperationHistory::open(&journal).expect("open journal");
    assert_eq!(history.pending_entries().expect("pending rows").len(), 1);
}

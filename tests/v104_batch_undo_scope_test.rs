//! v1.0.4 batch undo tests (M3 `batch-undo-scope`, option 1).
//!
//! One batch apply stamps every journal row with a shared `batch_id`, and
//! `undo --batch <id>` reverses exactly those rows newest-first, even when a
//! single rename happened in between (the case `undo --last N` cannot tell
//! apart). Old journals without the column keep working through the additive
//! migration. Every journal here is synthetic in a temp dir with an isolated
//! HOME; runs are bounded by a deadline.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use kaseifu::storage::history::{OperationHistory, PendingRecord};

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-batch-undo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn base_command(cwd: &Path, home: &Path) -> ProcessCommand {
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

/// Record one successful move exactly as a real execution leaves it: the file
/// ends up at its destination and the journal row is successful.
fn journaled_move(
    history: &OperationHistory,
    from_dir: &Path,
    to_dir: &Path,
    name: &str,
    batch_id: Option<&str>,
) {
    let source = from_dir.join(name);
    let destination = to_dir.join(name);
    std::fs::write(&source, b"fixture").expect("fixture file");
    let id = history
        .record_pending_in_batch(PendingRecord {
            operation: "move",
            source: &source,
            destination: &destination,
            candidate_id: None,
            confidence: None,
            margin: None,
            batch_id,
        })
        .expect("record move");
    std::fs::rename(&source, &destination).expect("simulate the move");
    history.mark_success(&id).expect("mark success");
}

/// A legacy journal: the pre-batch schema with no `batch_id` column.
fn legacy_journal_with_one_move(path: &Path, from_dir: &Path, to_dir: &Path, name: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("journal dir");
    }
    let connection = rusqlite::Connection::open(path).expect("legacy db");
    connection
        .execute_batch(
            "CREATE TABLE operations (
                id               TEXT PRIMARY KEY,
                timestamp        TEXT NOT NULL,
                operation        TEXT NOT NULL,
                source_path      TEXT NOT NULL,
                destination_path TEXT NOT NULL,
                candidate_id     TEXT,
                confidence       REAL,
                margin           REAL,
                result           TEXT NOT NULL,
                error            TEXT,
                undone_at        TEXT
            );",
        )
        .expect("legacy schema");
    let source = from_dir.join(name);
    let destination = to_dir.join(name);
    std::fs::write(&source, b"fixture").expect("fixture file");
    connection
        .execute(
            "INSERT INTO operations
                (id, timestamp, operation, source_path, destination_path, result)
             VALUES ('op-legacy-1', '2026-10-15T00:00:00Z', 'move', ?1, ?2, 'success')",
            rusqlite::params![source.to_string_lossy(), destination.to_string_lossy()],
        )
        .expect("legacy row");
    std::fs::rename(&source, &destination).expect("simulate the move");
}

#[test]
fn legacy_journal_without_batch_column_still_undoes() {
    let dir = temp_dir("legacy");
    let home = dir.join("home");
    let from_dir = dir.join("from");
    let to_dir = dir.join("to");
    std::fs::create_dir_all(&from_dir).expect("from");
    std::fs::create_dir_all(&to_dir).expect("to");
    std::fs::create_dir_all(&home).expect("home");
    legacy_journal_with_one_move(&isolated_journal(&home), &from_dir, &to_dir, "note.txt");

    let output = run(&dir, &home, &["undo", "--last", "1"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(from_dir.join("note.txt").exists(), "the move is reversed");
    let history = OperationHistory::open(isolated_journal(&home)).expect("reopen");
    let entries = history.entries().expect("entries");
    assert!(
        entries.iter().all(|entry| entry.batch_id.is_none()),
        "legacy rows stay batch-free"
    );
}

#[test]
fn batch_undo_reverses_exactly_the_batch_newest_first() {
    let dir = temp_dir("batch");
    let home = dir.join("home");
    let from_dir = dir.join("from");
    let to_dir = dir.join("to");
    for folder in [&from_dir, &to_dir, &home] {
        std::fs::create_dir_all(folder).expect("dir");
    }
    let history = OperationHistory::open(isolated_journal(&home)).expect("journal");
    for name in ["a.txt", "b.txt", "c.txt"] {
        journaled_move(&history, &from_dir, &to_dir, name, Some("batch-1"));
    }
    // A single rename after the batch: `--last` would sweep it in, `--batch`
    // must leave it alone.
    journaled_move(&history, &from_dir, &to_dir, "single.txt", None);
    drop(history);

    let output = run(&dir, &home, &["undo", "--batch", "batch-1"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(from_dir.join(name).exists(), "{name} is restored");
        assert!(!to_dir.join(name).exists(), "{name} left its destination");
    }
    assert!(
        to_dir.join("single.txt").exists(),
        "the interleaved move stays"
    );
    assert!(
        !from_dir.join("single.txt").exists(),
        "the interleaved move stays"
    );
}

#[test]
fn batch_undo_afterwards_still_offers_last_for_the_single_move() {
    let dir = temp_dir("batch-then-last");
    let home = dir.join("home");
    let from_dir = dir.join("from");
    let to_dir = dir.join("to");
    for folder in [&from_dir, &to_dir, &home] {
        std::fs::create_dir_all(folder).expect("dir");
    }
    let history = OperationHistory::open(isolated_journal(&home)).expect("journal");
    journaled_move(&history, &from_dir, &to_dir, "a.txt", Some("batch-9"));
    journaled_move(&history, &from_dir, &to_dir, "single.txt", None);
    drop(history);

    assert!(
        run(&dir, &home, &["undo", "--batch", "batch-9"])
            .status
            .success()
    );
    assert!(from_dir.join("a.txt").exists());
    let output = run(&dir, &home, &["undo", "--last", "1"]);
    assert!(output.status.success());
    assert!(
        from_dir.join("single.txt").exists(),
        "the single move undoes after"
    );
}

#[test]
fn unknown_batch_id_reports_nothing_to_undo() {
    let dir = temp_dir("unknown-batch");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let output = run(&dir, &home, &["undo", "--batch", "batch-nope"]);
    assert!(output.status.success());
    assert!(
        stdout_of(&output).contains("nothing left to undo"),
        "got: {}",
        stdout_of(&output)
    );
    let json = run(
        &dir,
        &home,
        &["--format", "json", "undo", "--batch", "batch-nope"],
    );
    assert!(json.status.success());
    let parsed: serde_json::Value =
        serde_json::from_str(stdout_of(&json).trim()).expect("one JSON document");
    assert_eq!(parsed["undo"]["status"], "empty", "got: {parsed}");
}

#[test]
fn batch_undo_json_reports_the_restored_rows() {
    let dir = temp_dir("batch-json");
    let home = dir.join("home");
    let from_dir = dir.join("from");
    let to_dir = dir.join("to");
    for folder in [&from_dir, &to_dir, &home] {
        std::fs::create_dir_all(folder).expect("dir");
    }
    let history = OperationHistory::open(isolated_journal(&home)).expect("journal");
    for name in ["a.txt", "b.txt"] {
        journaled_move(&history, &from_dir, &to_dir, name, Some("batch-7"));
    }
    drop(history);

    let output = run(
        &dir,
        &home,
        &["--format", "json", "undo", "--batch", "batch-7"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let parsed: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("one JSON document");
    assert_eq!(parsed["undo"]["status"], "restored", "got: {parsed}");
    assert_eq!(parsed["undo"]["restored"].as_array().map(Vec::len), Some(2));
}

#[test]
fn last_and_batch_conflict() {
    let dir = temp_dir("conflict");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let output = run(&dir, &home, &["undo", "--last", "2", "--batch", "batch-1"]);
    assert!(!output.status.success(), "--last and --batch conflict");
}

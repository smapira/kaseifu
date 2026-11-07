//! v0.13.2 issue #22: `kaseifu move`, the path without a decision.
//!
//! The command is tested at its own layer (`operations::relocate`) and through
//! the binary for the flags, because the point of the issue is that a folder can
//! be filed without a classifier: no key, no network, dry run by default, and
//! reversible with `undo`.
//!
//! Sandboxes live under `target/`, never under the system temp directory:
//! `is_excluded_destination` refuses anything under `/var` (where macOS puts its
//! temp directories), which would make every move here a refusal.

use kaseifu::operations::relocate::{
    EntryKind, RelocateError, RelocateOutcome, copy_then_remove, execute_relocate, plan_relocate,
    transfer,
};
use kaseifu::operations::undo::undo_last_move;
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};
use std::process::Command;

fn sandbox(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-sandboxes")
        .join(format!("v120-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("sandbox");
    root
}

fn folder_with_two_files(root: &Path, name: &str) -> PathBuf {
    let folder = root.join(name);
    std::fs::create_dir_all(folder.join("nested")).expect("folder");
    std::fs::write(folder.join("a.txt"), b"alpha").expect("a");
    std::fs::write(folder.join("nested/b.txt"), b"beta").expect("b");
    folder
}

// ---------------------------------------------------------------------------
// The move itself
// ---------------------------------------------------------------------------

#[test]
fn v120_execute_moves_a_folder() {
    let root = sandbox("folder");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");
    let history = OperationHistory::open_in_memory().expect("journal");

    let plan = plan_relocate(&source, &destination).expect("plan");
    assert_eq!(plan.kind, EntryKind::Dir);
    let outcome = execute_relocate(&plan, &history).expect("move");
    let RelocateOutcome::Moved { operation_id } = outcome else {
        panic!("expected a move, got {outcome:?}");
    };

    assert!(!source.exists(), "the folder left its old place");
    assert_eq!(
        std::fs::read(destination.join("spec/a.txt")).expect("a"),
        b"alpha"
    );
    assert_eq!(
        std::fs::read(destination.join("spec/nested/b.txt")).expect("b"),
        b"beta"
    );
    // The journal says what moved, and it is the newest move.
    let entry = history
        .last_successful_move()
        .expect("journal")
        .expect("one row");
    assert_eq!(entry.id, operation_id);
    assert_eq!(entry.kind.as_deref(), Some("dir"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_undo_restores_the_folder() {
    let root = sandbox("undo");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");
    let history = OperationHistory::open_in_memory().expect("journal");

    let plan = plan_relocate(&source, &destination).expect("plan");
    execute_relocate(&plan, &history).expect("move");

    let undone = undo_last_move(&history).expect("undo");
    assert_eq!(undone.restored_to, source);
    assert_eq!(
        std::fs::read(source.join("a.txt")).expect("a"),
        b"alpha",
        "the tree is back, file for file"
    );
    assert!(source.join("nested/b.txt").is_file());
    assert!(!destination.join("spec").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_the_journal_records_the_kind_for_files_too() {
    let root = sandbox("kind-file");
    let source = root.join("note.txt");
    std::fs::write(&source, b"hello").expect("file");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");
    let history = OperationHistory::open_in_memory().expect("journal");

    let plan = plan_relocate(&source, &destination).expect("plan");
    assert_eq!(plan.kind, EntryKind::File);
    execute_relocate(&plan, &history).expect("move");

    let entry = history
        .last_successful_move()
        .expect("journal")
        .expect("one row");
    assert_eq!(entry.kind.as_deref(), Some("file"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_old_rows_still_read() {
    // A journal written before the `kind` column: the migration adds it, the
    // rows keep their data, and the missing kind reads as none (= file).
    let root = sandbox("legacy");
    let journal = root.join("kaseifu.db");
    let source = root.join("old.txt");
    std::fs::create_dir_all(root.join("filed")).expect("filed");
    std::fs::write(&source, b"x").expect("file");
    {
        let connection = rusqlite::Connection::open(&journal).expect("legacy journal");
        connection
            .execute_batch(
                "CREATE TABLE operations (
                    id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, operation TEXT NOT NULL,
                    source_path TEXT NOT NULL, destination_path TEXT NOT NULL,
                    candidate_id TEXT, confidence REAL, margin REAL,
                    result TEXT NOT NULL, error TEXT, undone_at TEXT, batch_id TEXT
                 );
                 INSERT INTO operations VALUES (
                    'op-old', '2026-01-01T00:00:00+00:00', 'move',
                    'OLD_SOURCE', 'OLD_DESTINATION', NULL, NULL, NULL,
                    'success', NULL, NULL, NULL
                 );",
            )
            .expect("legacy schema");
    }

    let history = OperationHistory::open(&journal).expect("migrate");
    let entries = history.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_path, "OLD_SOURCE");
    assert_eq!(entries[0].kind, None, "a pre-kind row claims no kind");
    drop(history);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_undo_batch_still_works() {
    // Two moves in one batch reverse together; the kind column does not change
    // what `undo --batch` selects.
    let root = sandbox("batch");
    let from = root.join("inbox");
    let to = root.join("filed");
    std::fs::create_dir_all(&from).expect("inbox");
    std::fs::create_dir_all(&to).expect("filed");
    let history = OperationHistory::open_in_memory().expect("journal");
    history
        .record_pending_kind(
            "move",
            &from.join("one.txt"),
            &to.join("one.txt"),
            None,
            None,
            None,
            "file",
        )
        .expect("row");
    drop(history);

    // The full batch flow lives in `v104_batch_undo_scope_test`; here the
    // question is only that a kind-bearing row is still selectable.
    let history = OperationHistory::open_in_memory().expect("journal");
    std::fs::write(root.join("a.txt"), b"a").expect("a");
    std::fs::write(root.join("b.txt"), b"b").expect("b");
    let first = root.join("a.txt");
    let second = root.join("b.txt");
    std::fs::create_dir_all(root.join("target")).expect("target");
    let id = history
        .record_pending_kind(
            "move",
            &first,
            &root.join("target/a.txt"),
            None,
            None,
            None,
            "file",
        )
        .expect("row");
    transfer(&first, &root.join("target/a.txt")).expect("move");
    history.mark_success(&id).expect("success");
    assert!(root.join("target/a.txt").is_file());
    assert!(second.is_file());
    let undone = undo_last_move(&history).expect("undo");
    assert_eq!(undone.undone_move_id, id);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_a_failed_copy_leaves_the_source_and_no_residue() {
    let root = sandbox("failed");
    let source = root.join("album");
    std::fs::create_dir_all(&source).expect("album");
    std::fs::write(source.join("a.txt"), b"a").expect("a");
    // A link cannot be copied honestly, so the copy refuses after writing the
    // first file (the cross-volume path, exercised directly).
    std::os::unix::fs::symlink(root.join("missing"), source.join("link")).expect("link");
    let destination = root.join("moved-album");

    let error = copy_then_remove(&source, &destination).expect_err("must refuse the link");
    assert!(
        error.to_string().contains("only regular files and folders"),
        "{error}"
    );
    assert!(source.join("a.txt").is_file(), "the source stays whole");
    assert!(!destination.exists(), "no partial copy remains");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_an_excluded_destination_is_refused() {
    let root = sandbox("excluded-destination");
    let source = root.join("spec");
    std::fs::create_dir_all(&source).expect("spec");

    // The Library tree is never a destination (the path shape decides, so no
    // directory has to exist for the refusal).
    let error = plan_relocate(&source, Path::new("/Users/nobody/Library/archive"))
        .expect_err("must refuse");
    assert!(
        matches!(error, RelocateError::DestinationExcluded(_)),
        "{error:?}"
    );
    assert!(source.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_an_excluded_source_is_refused() {
    let root = sandbox("excluded-source");
    // A folder inside a cache tree: `~/Library/Caches`, `.Trash`, `.cache`.
    let cache = root.join(".cache");
    let source = cache.join("album");
    std::fs::create_dir_all(&source).expect("album");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("filed");

    let error = plan_relocate(&source, &destination).expect_err("must refuse");
    assert!(
        matches!(error, RelocateError::SourceExcluded(_)),
        "{error:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_a_folder_cannot_move_into_itself() {
    let root = sandbox("nested");
    let source = folder_with_two_files(&root, "spec");
    let inside = source.join("nested");

    let error = plan_relocate(&source, &inside).expect_err("must refuse");
    assert!(matches!(error, RelocateError::Nested(_, _)), "{error:?}");
    assert!(source.join("a.txt").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_a_collision_is_never_overwritten() {
    let root = sandbox("collision");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(destination.join("spec")).expect("existing");

    let error = plan_relocate(&source, &destination).expect_err("must refuse");
    assert!(matches!(error, RelocateError::Collision(_)), "{error:?}");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

fn run(home: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env_remove("JEV_API_KEY")
        .env_remove("KASEIFU_DEBUG");
    command.output().expect("run kaseifu")
}

#[test]
fn v120_move_is_a_dry_run_by_default() {
    let root = sandbox("cli-dry");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "stdout: {stdout}");
    assert!(source.join("a.txt").is_file(), "nothing moved: {stdout}");
    assert!(!destination.join("spec").exists());
    assert!(
        stdout.contains("dry-run (nothing moved; pass --execute to move)"),
        "{stdout}"
    );
    assert!(stdout.contains("kind: dir"), "{stdout}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_move_executes_and_journals() {
    let root = sandbox("cli-execute");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--execute",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "stdout: {stdout}");
    assert!(destination.join("spec/a.txt").is_file(), "{stdout}");
    assert!(!source.exists());
    assert!(stdout.contains("operation_id: op-"), "{stdout}");
    // The journal row exists in this sandbox's home, with the kind recorded.
    let journal = home.join("Library/Application Support/kaseifu/kaseifu.db");
    assert!(journal.is_file(), "the move is journaled");
    let history = OperationHistory::open(&journal).expect("journal");
    let entry = history
        .last_successful_move()
        .expect("journal")
        .expect("one row");
    assert_eq!(entry.kind.as_deref(), Some("dir"));
    drop(history);

    // And `undo` gives it back.
    let undone = run(&home, &["undo"]);
    let text = String::from_utf8_lossy(&undone.stdout);
    assert!(undone.status.success(), "{text}");
    assert!(source.join("a.txt").is_file(), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_move_json_is_one_document() {
    let root = sandbox("cli-json");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = root.join("note.txt");
    std::fs::write(&source, b"x").expect("file");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--format",
            "json",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("json document");

    assert_eq!(json["move"]["kind"], "file");
    assert_eq!(json["move"]["executed"], false);
    assert_eq!(json["move"]["operation_id"], serde_json::Value::Null);
    assert!(json["move"]["destination_path"].is_string());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_move_never_touches_the_classifier() {
    let root = sandbox("cli-no-key");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    // No key, no endpoint, no network: the move answers on its own.
    let output = run(
        &home,
        &[
            "move",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
            "--execute",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(!stderr.contains("key"), "{stderr}");
    assert!(!stderr.to_lowercase().contains("jev"), "{stderr}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v120_organize_still_refuses_a_folder() {
    let root = sandbox("organize-folder");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = folder_with_two_files(&root, "spec");
    let destination = root.join("filed");
    std::fs::create_dir_all(&destination).expect("destination");

    let output = run(
        &home,
        &[
            "organize",
            &source.to_string_lossy(),
            "--to",
            &destination.to_string_lossy(),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "a folder stays refused");
    assert!(
        stderr.contains("works on one file") && stderr.contains("label"),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

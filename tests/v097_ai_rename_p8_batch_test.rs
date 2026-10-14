//! v0.9.7 AI rename Phase 8 (batch rename over a folder's contents).
//!
//! The batch itself is exercised with `FakeFmBridge`, so nothing here needs
//! Apple Intelligence; the CLI tests use the paths that do not reach the model
//! (a foreign target, an empty folder, a missing flag) plus two runs that accept
//! either answer (a real model, or the explicit "not available" of a build
//! without one). Every fixture is a synthetic tree under the system temp
//! directory with an isolated HOME; the developer's folders, journal, Keychain
//! and completion directories are never touched.

use kaseifu::rename::batch::{self, Applied, BatchOutcome};
use kaseifu::rename::service::RenameOptions;
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::contract::SemanticInfo;
use kaseifu::semantic::fake::FakeFmBridge;
use kaseifu::semantic::provider::FoundationModelsProvider;
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

// ---------------------------------------------------------------------------
// Fixtures (synthetic trees only, temp dir only)
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v097-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    // The rename pipeline canonicalizes the target, so every expectation is
    // written against the canonical path (`/private/var/...` on macOS).
    std::fs::canonicalize(&dir).expect("canonical temp dir")
}

fn write_child(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("child parent");
    }
    std::fs::write(&path, contents).expect("fixture child");
    path
}

fn info(title: &str) -> SemanticInfo {
    SemanticInfo {
        title: Some(title.to_string()),
        category: None,
        project: None,
    }
}

/// A template with no `{variable}`: the proposal is the same words every time.
fn literal_options(name: &str) -> RenameOptions {
    RenameOptions {
        template: Some(name.to_string()),
        ..RenameOptions::default()
    }
}

fn provider(bridge: &FakeFmBridge) -> FoundationModelsProvider<FakeFmBridge> {
    FoundationModelsProvider::with_bridge(bridge.clone())
}

fn held_reason(item: &kaseifu::rename::batch::BatchItem) -> String {
    match &item.outcome {
        BatchOutcome::Held { reason } => reason.clone(),
        other => panic!("expected a held item, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Planning: conflicts are a property of the set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v097_two_items_that_would_write_one_name_are_both_held() {
    let dir = work_dir("conflict");
    write_child(&dir, "a.txt", "first note\n");
    write_child(&dir, "b.txt", "second note\n");
    write_child(&dir, "sub/c.txt", "third note\n");

    let bridge = FakeFmBridge::answering(info("title"));
    let store = RenameStore::open_in_memory().expect("store");
    let walk = batch::walk(&dir).expect("walk");
    let report = batch::plan(
        &provider(&bridge),
        &store,
        &walk,
        &dir,
        &literal_options("same"),
    )
    .await;

    assert_eq!(
        report.items.len(),
        4,
        "three children plus the folder that holds one of them"
    );
    assert_eq!(
        report.proposals(),
        2,
        "the child folder and the child in it are ready: {:?}",
        report.items
    );
    assert_eq!(report.conflicts(), 2, "the two siblings are held together");
    assert_eq!(report.held(), 0);
    assert_eq!(report.recorded(), 0, "planning renames nothing");
    assert!(dir.join("a.txt").is_file() && dir.join("b.txt").is_file());
    let conflicts: Vec<&BatchOutcome> = report
        .items
        .iter()
        .map(|item| &item.outcome)
        .filter(|outcome| matches!(outcome, BatchOutcome::Conflict { .. }))
        .collect();
    for outcome in conflicts {
        let BatchOutcome::Conflict { reason } = outcome else {
            unreachable!()
        };
        assert!(reason.contains("same name"), "{reason}");
    }
}

#[tokio::test]
async fn v097_an_unsupported_type_is_held_without_spending_a_model_call() {
    let dir = work_dir("held");
    write_child(&dir, "a.txt", "a note about the launch plan\n");
    write_child(&dir, "mystery.xyz", "who knows\n");

    let bridge = FakeFmBridge::answering(info("title"));
    let store = RenameStore::open_in_memory().expect("store");
    let walk = batch::walk(&dir).expect("walk");
    let report = batch::plan(
        &provider(&bridge),
        &store,
        &walk,
        &dir,
        &literal_options("same"),
    )
    .await;

    assert_eq!(report.held(), 1);
    assert_eq!(report.proposals(), 1);
    assert_eq!(
        bridge.requests().len(),
        1,
        "a refused target never reaches the model"
    );
    let held = report
        .items
        .iter()
        .find(|item| item.current_name == "mystery.xyz")
        .expect("the foreign type is in the report");
    let reason = held_reason(held);
    assert!(reason.contains("is not a document"), "{reason}");
}

// ---------------------------------------------------------------------------
// Applying: order, undo, and one item failing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v097_children_are_applied_before_their_parent_and_undo_reverses_every_row() {
    let dir = work_dir("order");
    let folder = dir.join("d");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "inner.txt", "inner note\n");

    let bridge = FakeFmBridge::answering(info("title"));
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");
    let walk = batch::walk(&dir).expect("walk");
    let mut report = batch::plan(
        &provider(&bridge),
        &store,
        &walk,
        &dir,
        &literal_options("renamed"),
    )
    .await;
    assert_eq!(report.proposals(), 2, "the folder and its child");

    batch::apply(&mut report, &history);
    assert_eq!(report.recorded(), 2, "{report:?}");
    assert!(
        dir.join("renamed/renamed.txt").is_file(),
        "both renamed, which only works when the child went first"
    );

    // Undo is newest-first, so the folder comes back before the row recorded
    // under its old name is reached (original spec section 10).
    kaseifu::operations::undo::undo_last_move(&history).expect("undo the folder");
    assert!(
        dir.join("d/renamed.txt").is_file(),
        "the child moved back with it"
    );
    kaseifu::operations::undo::undo_last_move(&history).expect("undo the child");
    assert!(dir.join("d/inner.txt").is_file(), "the child is back");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v097_one_failing_item_does_not_stop_the_batch() {
    let dir = work_dir("partial");
    write_child(&dir, "d1/a.txt", "first note\n");
    let doomed = write_child(&dir, "d2/b.txt", "second note\n");

    let bridge = FakeFmBridge::answering(info("title"));
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");
    let walk = batch::walk(&dir).expect("walk");
    let mut report = batch::plan(
        &provider(&bridge),
        &store,
        &walk,
        &dir,
        &literal_options("renamed"),
    )
    .await;
    assert_eq!(report.proposals(), 2);

    // The source disappears between planning and applying.
    std::fs::remove_file(&doomed).expect("remove the doomed child");
    batch::apply(&mut report, &history);

    assert_eq!(report.recorded(), 1, "the other item still went through");
    let failed = report
        .items
        .iter()
        .find_map(|item| match &item.applied {
            Some(Applied::Failed(reason)) => Some(reason.clone()),
            _ => None,
        })
        .expect("the missing source is reported as a failure");
    assert!(failed.contains("no longer exists"), "{failed}");
    assert!(dir.join("d1/renamed.txt").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The CLI: flags, an empty batch, and never waiting for input
// ---------------------------------------------------------------------------

fn isolated_home(dir: &Path) -> PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    home
}

fn cli(dir: &Path, home: &Path, args: &[&str]) -> Output {
    ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"))
        .current_dir(dir)
        .env("HOME", home)
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("KASEIFU_COMPLETION_DIR")
        .args(args)
        .output()
        .expect("kaseifu runs")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn v097_cli_recursive_needs_a_folder() {
    let dir = work_dir("cli-file");
    let home = isolated_home(&dir);
    write_child(&dir, "note.txt", "a short note\n");

    let output = cli(&dir, &home, &["rename", "note.txt", "--recursive"]);
    assert!(!output.status.success(), "a file is not a batch");
    assert!(
        stderr_of(&output).contains("--recursive needs a folder"),
        "{}",
        stderr_of(&output)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v097_cli_yes_belongs_to_recursive() {
    let dir = work_dir("cli-yes");
    let home = isolated_home(&dir);
    write_child(&dir, "note.txt", "a short note\n");

    let output = cli(&dir, &home, &["rename", "note.txt", "--yes"]);
    assert!(!output.status.success(), "--yes alone is a mistake");
    assert!(
        stderr_of(&output).contains("recursive"),
        "the error names the flag it needs: {}",
        stderr_of(&output)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v097_cli_an_empty_folder_is_an_empty_batch() {
    let dir = work_dir("cli-empty");
    let home = isolated_home(&dir);
    std::fs::create_dir_all(dir.join("empty folder")).expect("folder");

    // No target means nothing to ask the model for, so this answers on a build
    // without one too.
    let output = cli(&dir, &home, &["rename", "empty folder", "--recursive"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("proposals: 0"), "{text}");
    assert!(text.contains("conflicts: 0"), "{text}");
    assert!(text.contains("status: proposed"), "{text}");
    assert!(
        text.contains("mode: dry-run (pass --execute to rename)"),
        "{text}"
    );

    let json = cli(
        &dir,
        &home,
        &["rename", "empty folder", "--recursive", "--format", "json"],
    );
    let value: serde_json::Value =
        serde_json::from_str(stdout_of(&json).trim()).expect("one JSON document");
    let rename = &value["rename"];
    assert_eq!(rename["status"], serde_json::json!("proposed"));
    assert_eq!(rename["proposals"], serde_json::json!(0));
    assert_eq!(rename["truncated"], serde_json::json!(false));
    assert_eq!(rename["undo_last"], serde_json::json!(0));
    assert!(rename["items"].as_array().expect("items").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v097_cli_guidance_names_the_batch_form() {
    let dir = work_dir("cli-guidance");
    let home = isolated_home(&dir);
    let output = cli(&dir, &home, &["rename"]);
    let text = stdout_of(&output);
    assert!(text.contains("--recursive"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v097_cli_never_waits_for_input_without_a_terminal() {
    let dir = work_dir("cli-execute");
    let home = isolated_home(&dir);
    let folder = dir.join("tree");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "a.txt", "a note about the launch plan\n");
    write_child(&folder, "b.txt", "a second note about the budget\n");

    // `--execute` without a terminal must run to completion: no prompt, no
    // hang. The answer is either the real on-device model or the explicit
    // refusal a build without one gives.
    let output = cli(&dir, &home, &["rename", "tree", "--recursive", "--execute"]);
    let text = stdout_of(&output);
    let errors = stderr_of(&output);
    assert!(
        !text.contains("Apply changes?"),
        "a piped run must not ask: {text}"
    );
    if output.status.success() {
        assert!(text.contains("status: applied"), "{text}");
        assert!(
            text.contains("order: applied children before parents"),
            "{text}"
        );
        assert!(text.contains("undo: kaseifu undo --last 2"), "{text}");
        // Whatever the model called them, the two originals are gone.
        assert!(!folder.join("a.txt").exists() && !folder.join("b.txt").exists());
    } else {
        assert!(
            errors.contains("the on-device model is not available"),
            "a failure must name the reason: {errors}"
        );
        assert!(folder.join("a.txt").is_file(), "nothing was renamed");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

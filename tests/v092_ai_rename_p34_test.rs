//! v0.9.2 AI rename Phase 3 (Naming Rule Engine) and Phase 4 (Document Rename).
//!
//! Model-independent tests only: the provider is always a `FakeFmBridge`, so
//! nothing here needs Apple Intelligence. Real-model verification is manual
//! (see the plan's section 9). Filesystem fixtures live under the system
//! temp directory; binary runs get an isolated HOME and never touch the
//! developer's journal, Keychain or completion directories.

use kaseifu::operations::undo::undo_last_move;
use kaseifu::rename::service::{
    PlanOutcome, RenameError, RenameOptions, execute_plan, plan_rename,
};
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::contract::SemanticInfo;
use kaseifu::semantic::fake::FakeFmBridge;
use kaseifu::semantic::provider::FoundationModelsProvider;
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v092-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_doc(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("fixture file");
    path
}

fn answering(title: &str) -> FoundationModelsProvider<FakeFmBridge> {
    let info = SemanticInfo {
        title: Some(title.to_string()),
        category: Some("category".to_string()),
        project: Some("project".to_string()),
    };
    FoundationModelsProvider::with_bridge(FakeFmBridge::answering(info))
}

fn pattern_options(pattern: &str) -> RenameOptions {
    RenameOptions {
        pattern: Some(pattern.to_string()),
        ..RenameOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Planner and executor (Fake model, temp files, in-memory journals)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v092_plan_proposes_and_execute_journals_a_move_row() {
    let dir = work_dir("execute");
    let target = write_doc(&dir, "note.txt", "Meeting notes.\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("journal");
    let outcome = plan_rename(
        &answering("title"),
        &store,
        &target,
        &pattern_options("P02"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert!(
        plan.proposal.ends_with("_title.txt"),
        "proposal: {}",
        plan.proposal
    );
    assert_eq!(plan.rule_label, "P02");
    let operation_id = execute_plan(&plan, &history).expect("execute");
    assert!(!operation_id.is_empty());
    assert!(!target.exists(), "source moved away");
    assert!(plan.destination.exists(), "destination created");
    let rows = history.entries().expect("journal rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].operation, "move");
    assert_eq!(rows[0].result, "success");
    let restored = undo_last_move(&history).expect("undo");
    assert!(
        target.exists(),
        "undo restores {}",
        restored.restored_to.display()
    );
    assert!(!plan.destination.exists(), "undo removes the renamed file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_plan_holds_when_the_title_is_missing() {
    let dir = work_dir("held");
    let target = write_doc(&dir, "note.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    let info = SemanticInfo {
        title: Option::None,
        category: Option::None,
        project: Option::None,
    };
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::answering(info));
    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    assert!(
        matches!(outcome, PlanOutcome::Held { .. }),
        "got {outcome:?}"
    );
    assert!(target.exists(), "held plans move nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_plan_reports_noop_for_current_names() {
    let dir = work_dir("noop");
    let target = write_doc(&dir, "title.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(
        &answering("title"),
        &store,
        &target,
        &pattern_options("P01"),
    )
    .await
    .expect("plan");
    assert!(
        matches!(outcome, PlanOutcome::Noop { .. }),
        "got {outcome:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_plan_holds_dangerous_titles_without_sanitizing() {
    let dir = work_dir("danger");
    let target = write_doc(&dir, "note.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(&answering("a/b"), &store, &target, &pattern_options("P01"))
        .await
        .expect("plan");
    assert!(
        matches!(outcome, PlanOutcome::Held { .. }),
        "got {outcome:?}"
    );
    assert!(target.exists(), "dangerous titles move nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_execute_fails_cleanly_when_the_source_vanishes() {
    let dir = work_dir("vanish");
    let target = write_doc(&dir, "note.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("journal");
    let outcome = plan_rename(
        &answering("title"),
        &store,
        &target,
        &pattern_options("P02"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan");
    };
    std::fs::remove_file(&target).expect("remove source");
    assert!(
        execute_plan(&plan, &history).is_err(),
        "vanished source must fail"
    );
    assert!(
        history.entries().expect("rows").is_empty(),
        "no journal write before recheck"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_execute_never_overwrites() {
    let dir = work_dir("collision");
    let target = write_doc(&dir, "note.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("journal");
    let outcome = plan_rename(
        &answering("title"),
        &store,
        &target,
        &pattern_options("P02"),
    )
    .await
    .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan");
    };
    std::fs::write(&plan.destination, "squatter").expect("squatter");
    assert!(
        execute_plan(&plan, &history).is_err(),
        "collision must fail"
    );
    assert_eq!(
        std::fs::read_to_string(&plan.destination).expect("squatter kept"),
        "squatter"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_unavailable_models_fail_explicitly() {
    let dir = work_dir("unavail");
    let target = write_doc(&dir, "note.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FoundationModelsProvider::with_bridge(FakeFmBridge::unavailable(
        "appleIntelligenceNotEnabled",
    ));
    let error = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect_err("unavailable model must fail");
    assert!(
        matches!(error, RenameError::ModelUnavailable(_)),
        "got {error}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_symlinks_and_foreign_types_are_refused() {
    let dir = work_dir("refuse");
    let real = write_doc(&dir, "real.txt", "content\n");
    #[cfg(unix)]
    {
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let store = RenameStore::open_in_memory().expect("store");
        let error = plan_rename(&answering("title"), &store, &link, &pattern_options("P02"))
            .await
            .expect_err("symlink must fail");
        assert!(matches!(error, RenameError::Precondition(_)), "got {error}");
    }
    let photo = write_doc(&dir, "photo.jpg", "fake\n");
    let store = RenameStore::open_in_memory().expect("store");
    let error = plan_rename(&answering("title"), &store, &photo, &pattern_options("P02"))
        .await
        .expect_err("image must fail");
    assert!(matches!(error, RenameError::Unsupported(_)), "got {error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v092_stored_presets_and_defaults_drive_planning() {
    let dir = work_dir("preset-plan");
    let target = write_doc(&dir, "note.txt", "content\n");
    let store = RenameStore::open_in_memory().expect("store");
    store
        .save_preset("memo", "{title}_v1", "YYYY-MM-DD")
        .expect("save");
    let options = RenameOptions {
        preset: Some("memo".to_string()),
        ..RenameOptions::default()
    };
    let outcome = plan_rename(&answering("title"), &store, &target, &options)
        .await
        .expect("plan");
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    assert_eq!(plan.proposal, "title_v1.txt");
    assert_eq!(plan.rule_label, "preset:memo");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// CLI (isolated HOME, no model calls)
// ---------------------------------------------------------------------------

fn isolated_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("kaseifu-v092-home-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("home dir");
    home
}

fn cli(home: &Path, args: &[&str]) -> Output {
    ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"))
        .args(args)
        .env("HOME", home)
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .output()
        .expect("kaseifu runs")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn v092_help_lists_rename_and_stays_ascii() {
    let home = isolated_home("help");
    let output = cli(&home, &["rename", "--help"]);
    assert!(output.status.success(), "rename --help exits 0");
    let help = stdout_of(&output);
    assert!(help.contains("--pattern"), "documents --pattern:\n{help}");
    assert!(help.contains("--execute"), "documents --execute:\n{help}");
    assert!(help.is_ascii(), "rename help is ASCII:\n{help}");
    assert!(!help.contains("\u{2014}"), "no em dash:\n{help}");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn v092_patterns_lists_builtins_then_stored_presets() {
    let home = isolated_home("patterns");
    let output = cli(&home, &["rename", "--patterns"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    for id in ["P01", "P02", "P12"] {
        assert!(text.contains(id), "{id} listed:\n{text}");
    }
    let saved = cli(
        &home,
        &[
            "rename",
            "--save-preset",
            "memo",
            "--template",
            "{date}_{title}",
        ],
    );
    assert!(
        saved.status.success(),
        "save: {}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let listed = cli(&home, &["rename", "--list-presets"]);
    assert!(listed.status.success());
    assert!(stdout_of(&listed).contains("preset:memo"));
    let deleted = cli(&home, &["rename", "--delete-preset", "memo"]);
    assert!(deleted.status.success());
    let missing = cli(&home, &["rename", "--delete-preset", "memo"]);
    assert!(!missing.status.success(), "second delete must fail");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn v092_default_rule_round_trips() {
    let home = isolated_home("default");
    let shown = cli(&home, &["rename", "--show-default"]);
    assert!(shown.status.success());
    assert!(stdout_of(&shown).contains("P02"), "shipped default is P02");
    let set = cli(&home, &["rename", "--set-default", "date-title"]);
    assert!(set.status.success());
    assert!(stdout_of(&set).contains("P02"), "slug resolves to P02");
    let bad = cli(&home, &["rename", "--set-default", "P13"]);
    assert!(!bad.status.success(), "unknown rules must fail");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn v092_foreign_types_fail_before_any_model_call() {
    let home = isolated_home("foreign");
    let dir = work_dir("foreign");
    let photo = write_doc(&dir, "photo.jpg", "fake\n");
    let output = cli(&home, &["rename", photo.to_str().expect("utf8")]);
    assert!(!output.status.success(), "images must fail");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("Phase 5"), "points at Phase 5: {stderr}");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v092_execute_and_dry_run_conflict() {
    let home = isolated_home("conflict");
    let output = cli(&home, &["rename", "note.txt", "--execute", "--dry-run"]);
    assert!(!output.status.success(), "conflicting flags must fail");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn v092_bare_rename_prints_guidance_without_side_effects() {
    let home = isolated_home("guidance");
    let output = cli(&home, &["rename"]);
    assert!(output.status.success(), "bare rename exits 0");
    assert!(stdout_of(&output).contains("usage:"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn v092_completions_offer_rename_and_hide_discover() {
    let home = isolated_home("completion");
    let output = cli(&home, &["completions", "zsh"]);
    assert!(output.status.success());
    let script = stdout_of(&output);
    assert!(script.contains("(rename)"), "rename arm present");
    assert!(script.contains("'rename:"), "rename offered");
    assert!(!script.contains("discover"), "hidden command stays hidden");
    let _ = std::fs::remove_dir_all(&home);
}

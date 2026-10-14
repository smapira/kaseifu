//! v0.9.6 AI rename Phase 7 (folder rename from the folder's contents).
//!
//! Model-independent tests only: the bridge is always a `FakeFmBridge`, so
//! nothing here needs Apple Intelligence (the CLI tests use the paths that do
//! not reach the model: an empty folder, a refusal, a foreign type). Every
//! fixture is a synthetic folder tree under the system temp directory with an
//! isolated HOME for the CLI run; the developer's folders, journal, Keychain
//! and completion directories are never touched.

use kaseifu::context::directory::{
    DirectoryRefusal, MAX_DIRECTORY_ENTRIES, RefusalEnv, read_directory_facts, refuse_reason,
};
use kaseifu::models::file_context::{
    DirectoryEntry, DirectoryFacts, EntryKind, NormalizedFileContext, TargetFile,
};
use kaseifu::operations::undo::{UndoError, undo_last_move};
use kaseifu::rename::directory::{DirectoryError, classify_directory};
use kaseifu::rename::document::classify_document;
use kaseifu::rename::service::{PlanOutcome, RenameOptions, RenamePlan, execute_plan, plan_rename};
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::contract::{SCHEMA_VERSION, SemanticInfo, TargetKind};
use kaseifu::semantic::fake::FakeFmBridge;
use kaseifu::semantic::provider::{FoundationModelsProvider, build_request};
use kaseifu::storage::history::OperationHistory;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

// ---------------------------------------------------------------------------
// Fixtures (synthetic trees only, temp dir only)
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v096-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    // The rename pipeline canonicalizes the target, so every expectation must
    // be written against the canonical path (`/private/var/...` on macOS).
    std::fs::canonicalize(&dir).expect("canonical temp dir")
}

fn write_child(folder: &Path, name: &str, contents: &str) -> PathBuf {
    let path = folder.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("child parent");
    }
    std::fs::write(&path, contents).expect("fixture child");
    path
}

/// The original spec's worked example: four children, two languages, two types.
fn invoices_folder(root: &Path) -> PathBuf {
    let folder = root.join("\u{65b0}\u{3057}\u{3044}\u{30d5}\u{30a9}\u{30eb}\u{30c0}");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(
        &folder,
        "invoice_001.pdf",
        "\u{8acb}\u{6c42}\u{66f8} invoice 001 ACME GmbH 2026-10-01 total 1200 EUR\n",
    );
    write_child(
        &folder,
        "invoice_002.pdf",
        "\u{8acb}\u{6c42}\u{66f8} invoice 002 ACME GmbH 2026-10-02 total 900 EUR\n",
    );
    write_child(
        &folder,
        "contract.pdf",
        "\u{5951}\u{7d04}\u{66f8} contract ACME GmbH signed 2026-10-03\n",
    );
    write_child(&folder, "payment.xlsx", "payment schedule 2026-11-01\n");
    folder
}

fn info(title: &str) -> SemanticInfo {
    SemanticInfo {
        title: Some(title.to_string()),
        category: None,
        project: None,
    }
}

fn title_options(pattern: &str) -> RenameOptions {
    RenameOptions {
        pattern: Some(pattern.to_string()),
        ..RenameOptions::default()
    }
}

fn literal_options(name: &str) -> RenameOptions {
    RenameOptions {
        template: Some(name.to_string()),
        ..RenameOptions::default()
    }
}

fn ready(outcome: PlanOutcome) -> RenamePlan {
    match outcome {
        PlanOutcome::Ready(plan) => *plan,
        other => panic!("expected a ready plan, got {other:?}"),
    }
}

fn held_reason(outcome: PlanOutcome) -> String {
    match outcome {
        PlanOutcome::Held { reason, .. } => reason,
        other => panic!("expected a held plan, got {other:?}"),
    }
}

/// One plan request, as JSON.
fn request_of(bridge: &FakeFmBridge) -> serde_json::Value {
    let requests = bridge.requests();
    assert_eq!(requests.len(), 1, "exactly one model request");
    serde_json::from_str(&requests[0]).expect("the request is JSON")
}

fn provider(bridge: &FakeFmBridge) -> FoundationModelsProvider<FakeFmBridge> {
    FoundationModelsProvider::with_bridge(bridge.clone())
}

// ---------------------------------------------------------------------------
// DIR01 / W1 / W2: the plan comes from the children, not from the folder name
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v096_dir01_the_folder_plan_follows_the_children() {
    let dir = work_dir("dir01");
    let folder = invoices_folder(&dir);
    std::fs::create_dir_all(folder.join("archive")).expect("child folder");
    write_child(&folder, "archive/old.txt", "archived note\n");

    let bridge = FakeFmBridge::answering(info(
        "\u{8acb}\u{6c42}\u{30fb}\u{5951}\u{7d04}\u{95a2}\u{9023}\u{66f8}\u{985e}",
    ));
    let plans = provider(&bridge);
    let store = RenameStore::open_in_memory().expect("store");
    let plan = ready(
        plan_rename(&plans, &store, &folder, &title_options("P01"))
            .await
            .expect("plan"),
    );

    // The name is the model's title, with no extension appended and no echo of
    // the folder's own name.
    assert_eq!(
        plan.proposal,
        "\u{8acb}\u{6c42}\u{30fb}\u{5951}\u{7d04}\u{95a2}\u{9023}\u{66f8}\u{985e}"
    );
    assert_eq!(
        plan.destination,
        dir.join("\u{8acb}\u{6c42}\u{30fb}\u{5951}\u{7d04}\u{95a2}\u{9023}\u{66f8}\u{985e}")
    );
    assert!(
        matches!(plan.date_source, "created" | "modified" | "now"),
        "a folder date is one of the folder's own signals: {}",
        plan.date_source
    );
    let report = plan
        .directory
        .as_ref()
        .expect("the folder report")
        .summary();
    assert!(
        report.contains("5 entries (4 files, 1 folder)"),
        "only the folder's own children are counted: {report}"
    );
    assert!(report.contains("types pdf 3, xlsx 1"), "{report}");

    // The request carries the bounded sample beside the existing keys and the
    // child text as the existing T: signals. The folder's own name is not
    // evidence: no F: signal (normalize derives those from the target name)
    // and the folder name appears nowhere in the signals.
    let request = request_of(&bridge);
    assert_eq!(request["target_type"], serde_json::json!("directory"));
    let directory = &request["input"]["directory"];
    assert_eq!(directory["total_entries"], serde_json::json!(5));
    assert_eq!(directory["directory_count"], serde_json::json!(1));
    assert_eq!(directory["truncated"], serde_json::json!(false));
    assert_eq!(
        directory["folders"],
        serde_json::json!(["archive"]),
        "the child folder name is evidence"
    );
    let files = directory["files"].as_array().expect("files");
    assert_eq!(files.len(), 4);
    assert_eq!(files[0], serde_json::json!("contract.pdf"));
    assert_eq!(
        directory["extensions"],
        serde_json::json!([
            {"extension": "pdf", "count": 3},
            {"extension": "xlsx", "count": 1}
        ])
    );
    let signals = request["input"]["semantic_signals"]
        .as_array()
        .expect("signals");
    assert!(
        signals
            .iter()
            .all(|signal| signal.as_str().unwrap_or("").starts_with("T:")),
        "a folder carries only what was read inside it: {signals:?}"
    );
    assert!(
        signals
            .iter()
            .any(|signal| signal.as_str().unwrap_or("").contains("ACME")),
        "the children's text is the evidence: {signals:?}"
    );
    assert!(
        signals.len() <= 12,
        "a folder contributes at most the folder text budget"
    );
    // The existing keys keep their shape (the contract only gained a key).
    assert_eq!(
        request["input"]["filename"],
        serde_json::json!("\u{65b0}\u{3057}\u{3044}\u{30d5}\u{30a9}\u{30eb}\u{30c0}"),
        "input.filename stays on the wire; the prompt is what omits it"
    );
    assert!(request["input"]["image"].is_null());
    assert!(request["input"]["perception"].is_null());
    assert!(request["input"]["video"].is_null());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v096_the_contract_shapes_a_directory_request_without_changing_the_others() {
    let facts = DirectoryFacts {
        total_entries: 4,
        file_count: 3,
        directory_count: 1,
        truncated: false,
        files: vec![
            "a.pdf".to_string(),
            "b.pdf".to_string(),
            "c.txt".to_string(),
        ],
        folders: vec!["archive".to_string()],
        extensions: vec![
            kaseifu::models::file_context::ExtensionCount {
                extension: "pdf".to_string(),
                count: 2,
            },
            kaseifu::models::file_context::ExtensionCount {
                extension: "txt".to_string(),
                count: 1,
            },
        ],
    };
    let dir = work_dir("contract");
    let folder = dir.join("proposal");
    std::fs::create_dir_all(&folder).expect("folder");
    let context = NormalizedFileContext {
        target: TargetFile {
            path: folder,
            filename: "proposal".to_string(),
            ..TargetFile::default()
        },
        parent_directory: dir.to_string_lossy().into_owned(),
        semantic_signals: vec!["T:launch budget".to_string()],
        directory: Some(facts.clone()),
        ..NormalizedFileContext::default()
    };
    let request = build_request(&context, "rename-0001", "ja");
    let json = serde_json::to_value(&request).expect("json");
    assert_eq!(json["schema_version"], serde_json::json!(SCHEMA_VERSION));
    assert_eq!(json["target_type"], serde_json::json!("directory"));
    assert_eq!(
        json["input"]["directory"]["files"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        json["input"]["directory"]["truncated"],
        serde_json::json!(false)
    );
    assert!(
        json["input"].get("directory").is_some(),
        "the key is optional but present for a folder"
    );
    // A document context carries no directory key at all (add-only).
    let document = NormalizedFileContext {
        target: TargetFile {
            path: PathBuf::from("/tmp/a.txt"),
            filename: "a.txt".to_string(),
            extension: Some("txt".to_string()),
            ..TargetFile::default()
        },
        parent_directory: "/tmp".to_string(),
        ..NormalizedFileContext::default()
    };
    let document_json =
        serde_json::to_value(build_request(&document, "rename-0002", "ja")).expect("json");
    assert!(document_json["input"].get("directory").is_none());
    for key in [
        "filename",
        "size_bytes",
        "parent_directory",
        "semantic_signals",
    ] {
        assert!(
            json["input"].get(key).is_some() && document_json["input"].get(key).is_some(),
            "existing input key {key} survives"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v096_the_target_kind_follows_the_filesystem_not_the_name() {
    let dir = work_dir("kind");
    let folder_named_like_an_image = dir.join("IMG_0001.png");
    std::fs::create_dir_all(&folder_named_like_an_image).expect("folder");
    let real_image = write_child(&dir, "IMG_0002.png", "not really a png");
    assert_eq!(
        kaseifu::semantic::provider::target_kind(&folder_named_like_an_image),
        TargetKind::Directory,
        "a folder called IMG_0001.png is a folder"
    );
    assert_eq!(
        kaseifu::semantic::provider::target_kind(&real_image),
        TargetKind::Image
    );
    assert_eq!(
        kaseifu::semantic::provider::target_kind(Path::new("/tmp/notes.txt")),
        TargetKind::Document
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// DIR02 / DIR03 / DIR04 / DIR05: empty, mixed, large, nested
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v096_dir02_an_empty_folder_is_held_before_the_model_runs() {
    let dir = work_dir("empty");
    let store = RenameStore::open_in_memory().expect("store");
    for (tag, with_noise) in [("nothing at all", false), ("nothing but noise", true)] {
        let folder = dir.join(tag.replace(' ', "-"));
        std::fs::create_dir_all(&folder).expect("folder");
        if with_noise {
            write_child(&folder, ".DS_Store", "noise");
        }
        let bridge = FakeFmBridge::answering(info("never used"));
        let plans = provider(&bridge);
        let outcome = plan_rename(&plans, &store, &folder, &title_options("P01"))
            .await
            .expect("plan");
        let reason = held_reason(outcome);
        assert!(
            reason.contains("nothing to name"),
            "{tag}: the reason says why: {reason}"
        );
        assert!(
            bridge.requests().is_empty(),
            "{tag}: an empty folder spends no inference"
        );
        assert!(folder.exists(), "{tag}: nothing moved");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_dir03_mixed_contents_hold_instead_of_forcing_a_theme() {
    let dir = work_dir("mixed");
    let folder = dir.join("mixed");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "budget.xlsx", "numbers\n");
    write_child(&folder, "holiday.jpg", "pixels stay unread\n");
    write_child(&folder, "notes.txt", "a note\n");
    let store = RenameStore::open_in_memory().expect("store");

    // The model was told to leave the title empty rather than invent a theme;
    // an empty title is a held proposal with the folder's own reason.
    let silent = FakeFmBridge::answering(SemanticInfo {
        title: None,
        category: Some("documents".to_string()),
        project: None,
    });
    let outcome = plan_rename(&provider(&silent), &store, &folder, &title_options("P01"))
        .await
        .expect("plan");
    let reason = held_reason(outcome);
    assert!(
        reason.contains("no title could be drawn"),
        "the mixed reason is explicit: {reason}"
    );
    assert!(folder.exists());

    // A mixed folder whose contents DO share a theme is still proposed: the
    // original spec's own example mixes pdf and xlsx and must be named.
    let speaking = FakeFmBridge::answering(info("household paperwork"));
    let outcome = plan_rename(&provider(&speaking), &store, &folder, &title_options("P01"))
        .await
        .expect("plan");
    assert_eq!(ready(outcome).proposal, "household_paperwork");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_dir04_a_large_folder_is_sampled_and_says_so() {
    let dir = work_dir("large");
    let folder = dir.join("large");
    std::fs::create_dir_all(&folder).expect("folder");
    for index in 0..(MAX_DIRECTORY_ENTRIES + 60) {
        write_child(&folder, &format!("scan_{index:04}.txt"), "page\n");
    }
    let facts = read_directory_facts(&folder).expect("facts");
    assert_eq!(facts.total_entries, MAX_DIRECTORY_ENTRIES);
    assert!(facts.truncated);
    assert_eq!(facts.files.len(), DirectoryFacts::MAX_FILES);

    let bridge = FakeFmBridge::answering(info("scanned pages"));
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(&provider(&bridge), &store, &folder, &title_options("P01"))
        .await
        .expect("plan");
    let plan = ready(outcome);
    let report = plan.directory.as_ref().expect("report").summary();
    assert!(
        report.starts_with("at least 200 entries"),
        "the report does not claim a total it did not read: {report}"
    );
    let request = request_of(&bridge);
    assert_eq!(
        request["input"]["directory"]["truncated"],
        serde_json::json!(true)
    );
    assert_eq!(
        request["input"]["directory"]["files"]
            .as_array()
            .unwrap()
            .len(),
        DirectoryFacts::MAX_FILES
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_dir05_nested_folders_contribute_names_but_no_recursion() {
    let dir = work_dir("nested");
    let folder = dir.join("outer");
    let inner = folder.join("2024");
    std::fs::create_dir_all(&inner).expect("nested folder");
    write_child(
        &inner,
        "inside-the-nested-folder.txt",
        "a grandchild that must not be read\n",
    );

    let bridge = FakeFmBridge::answering(info("year 2024"));
    let store = RenameStore::open_in_memory().expect("store");
    let outcome = plan_rename(&provider(&bridge), &store, &folder, &title_options("P01"))
        .await
        .expect("plan");
    assert_eq!(ready(outcome).proposal, "year_2024");
    let request = request_of(&bridge);
    assert_eq!(
        request["input"]["directory"]["folders"],
        serde_json::json!(["2024"]),
        "the child folder name is evidence"
    );
    assert_eq!(
        request["input"]["directory"]["file_count"],
        serde_json::json!(0)
    );
    let serialized = request.to_string();
    assert!(
        !serialized.contains("inside-the-nested-folder"),
        "a grandchild is never listed: {serialized}"
    );
    let signals: Vec<&str> = request["input"]["semantic_signals"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|signal| signal.as_str())
        .collect();
    assert!(
        signals.iter().all(|signal| !signal.starts_with("T:")),
        "nothing inside a nested folder is read: {signals:?}"
    );
    assert!(
        signals.is_empty(),
        "a folder whose children carry no text contributes no signal at all: {signals:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_a_child_that_is_not_a_document_is_not_opened() {
    let dir = work_dir("children");
    let folder = dir.join("folder");
    std::fs::create_dir_all(folder.join("looks-like-a-document.txt")).expect("child folder");
    write_child(&folder, "looks-like-a-picture.png", "pixels stay unread\n");
    write_child(&folder, "readable.txt", "the launch plan and the budget\n");

    let bridge = FakeFmBridge::answering(info("launch notes"));
    let store = RenameStore::open_in_memory().expect("store");
    plan_rename(&provider(&bridge), &store, &folder, &title_options("P01"))
        .await
        .expect("plan");
    let request = request_of(&bridge);
    let signals: Vec<&str> = request["input"]["semantic_signals"]
        .as_array()
        .expect("signals")
        .iter()
        .filter_map(|signal| signal.as_str())
        .collect();
    let excerpts: Vec<&&str> = signals
        .iter()
        .filter(|signal| signal.starts_with("T:"))
        .collect();
    assert_eq!(
        excerpts.len(),
        1,
        "only the readable text child contributes an excerpt: {signals:?}"
    );
    assert!(excerpts[0].contains("budget"));
    assert_eq!(
        signals.len(),
        1,
        "the folder's own identity contributes no signal: {signals:?}"
    );
    assert!(
        signals.iter().all(|signal| !signal.starts_with("F:")
            && !signal.starts_with("M:")
            && !signal.starts_with("K:")),
        "neither the name nor the Spotlight title of the folder travels: {signals:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// W7 / W8 / DIR06 / DIR07 / DIR08: journal, undo, and child history
// ---------------------------------------------------------------------------

/// Plan and execute one rename, returning the journal id.
async fn execute_folder_rename(
    store: &RenameStore,
    history: &OperationHistory,
    target: &Path,
    options: &RenameOptions,
) -> String {
    let bridge = FakeFmBridge::answering(info("unused"));
    let plan = ready(
        plan_rename(&provider(&bridge), store, target, options)
            .await
            .expect("plan"),
    );
    execute_plan(&plan, history).expect("execute")
}

#[tokio::test]
async fn v096_dir07_a_folder_rename_is_journaled_and_undone() {
    let dir = work_dir("undo");
    let folder = dir.join("old-name");
    std::fs::create_dir_all(folder.join("nested")).expect("folder");
    write_child(&folder, "nested/child.txt", "a child\n");
    write_child(&folder, "top.txt", "a top child\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");

    let operation_id =
        execute_folder_rename(&store, &history, &folder, &literal_options("new-name")).await;
    let renamed = dir.join("new-name");
    assert!(renamed.is_dir(), "the folder moved with its children");
    assert!(renamed.join("nested/child.txt").is_file());
    assert!(!folder.exists(), "the old name is gone");
    let rows = history.entries().expect("rows");
    assert_eq!(rows.len(), 1, "one move row, like every other rename");
    assert_eq!(rows[0].operation, "move");
    assert_eq!(rows[0].result, "success");
    assert_eq!(rows[0].id, operation_id);
    assert_eq!(rows[0].source_path, folder.to_string_lossy());
    assert_eq!(rows[0].destination_path, renamed.to_string_lossy());

    let outcome = undo_last_move(&history).expect("undo");
    assert_eq!(outcome.restored_to, folder);
    assert!(folder.is_dir(), "a folder undo works like a file undo");
    assert!(
        folder.join("nested/child.txt").is_file(),
        "the children came back"
    );
    assert!(!renamed.exists());
    assert!(
        history.last_successful_move().expect("query").is_none(),
        "double undo prevention: the move is marked undone"
    );
    assert!(matches!(
        undo_last_move(&history),
        Err(UndoError::NothingToUndo)
    ));
    assert!(folder.is_dir(), "a refused undo changes nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_dir08_child_history_resolves_after_the_parent_was_renamed() {
    let dir = work_dir("history");
    let parent = dir.join("parent");
    std::fs::create_dir_all(&parent).expect("parent");
    let child = write_child(&parent, "child.txt", "a child\n");
    write_child(&parent, "other.txt", "another child\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");

    // Child first, then the parent: the child's row now points under a name
    // that no longer exists while the parent row is in effect.
    execute_folder_rename(
        &store,
        &history,
        &child,
        &literal_options("child-renamed.txt"),
    )
    .await;
    execute_folder_rename(
        &store,
        &history,
        &parent,
        &literal_options("parent-renamed"),
    )
    .await;
    let rows = history.entries().expect("rows");
    assert_eq!(rows.len(), 2);
    let child_row = rows[0].clone();
    let parent_row = rows[1].clone();
    assert_eq!(child_row.operation, "move");
    assert_eq!(parent_row.operation, "move");
    assert!(parent_row.undone_at.is_none());
    assert!(
        !Path::new(&child_row.destination_path).exists(),
        "while the parent row is in effect the child's recorded path is stale"
    );

    // Undo is newest first, so the ancestor row is always reversed first: after
    // it, the child's recorded paths exist again exactly as recorded.
    let undone_parent = undo_last_move(&history).expect("undo the parent");
    assert_eq!(undone_parent.undone_move_id, parent_row.id);
    assert!(parent.is_dir(), "the parent's old name is restored");
    assert!(
        Path::new(&child_row.destination_path).exists(),
        "the child's history resolves again: {}",
        child_row.destination_path
    );
    assert_eq!(
        std::fs::read_to_string(parent.join("child-renamed.txt")).expect("child"),
        "a child\n"
    );

    let undone_child = undo_last_move(&history).expect("undo the child");
    assert_eq!(undone_child.undone_move_id, child_row.id);
    assert!(
        parent.join("child.txt").is_file(),
        "the child's name is back"
    );
    assert!(!parent.join("child-renamed.txt").exists());

    // The journal was never rewritten: the child's row still records the paths
    // it was executed with.
    let rows = history.entries().expect("rows");
    assert_eq!(rows[0].source_path, child.to_string_lossy());
    assert_eq!(
        rows[0].destination_path,
        parent.join("child-renamed.txt").to_string_lossy()
    );
    assert!(rows[0].undone_at.is_some() && rows[1].undone_at.is_some());
    assert!(matches!(
        undo_last_move(&history),
        Err(UndoError::NothingToUndo)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_dir08_the_parent_first_order_also_resolves() {
    let dir = work_dir("history2");
    let parent = dir.join("first");
    std::fs::create_dir_all(&parent).expect("parent");
    write_child(&parent, "note.txt", "a note\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");

    execute_folder_rename(&store, &history, &parent, &literal_options("second")).await;
    let moved_child = dir.join("second/note.txt");
    execute_folder_rename(
        &store,
        &history,
        &moved_child,
        &literal_options("note-renamed.txt"),
    )
    .await;
    assert!(dir.join("second/note-renamed.txt").is_file());

    // Newest first: the child comes back, then the parent, then nothing.
    let child_undo = undo_last_move(&history).expect("undo the child");
    assert_eq!(child_undo.restored_to, moved_child);
    let parent_undo = undo_last_move(&history).expect("undo the parent");
    assert_eq!(parent_undo.restored_to, parent);
    assert!(parent.join("note.txt").is_file());
    assert!(!dir.join("second").exists());
    assert!(matches!(
        undo_last_move(&history),
        Err(UndoError::NothingToUndo)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_a_folder_undo_never_overwrites_and_reports_a_stale_path() {
    let dir = work_dir("undo-safety");
    let folder = dir.join("before");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "child.txt", "a child\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");
    execute_folder_rename(&store, &history, &folder, &literal_options("after")).await;

    // Something else took the old name: the undo refuses and moves nothing.
    let occupied = dir.join("before");
    std::fs::create_dir_all(&occupied).expect("occupied");
    write_child(&occupied, "not-ours.txt", "someone else\n");
    let refusal = undo_last_move(&history).expect_err("a collision refuses");
    assert!(matches!(refusal, UndoError::Collision(_)), "{refusal}");
    assert!(
        occupied.join("not-ours.txt").is_file(),
        "nothing was overwritten"
    );
    assert!(dir.join("after").is_dir(), "the renamed folder stays put");

    // Remove the squatter: the undo now works again for the same row.
    std::fs::remove_dir_all(&occupied).expect("clean up");
    undo_last_move(&history).expect("undo");
    assert!(folder.join("child.txt").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_a_path_stale_for_reasons_the_journal_cannot_explain_is_refused() {
    let dir = work_dir("stale");
    let folder = dir.join("tracked");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "child.txt", "a child\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");
    execute_folder_rename(&store, &history, &folder, &literal_options("moved")).await;

    // An outside rename (Finder, another tool) leaves no journal row: the undo
    // must refuse with the recorded path and change nothing.
    std::fs::rename(dir.join("moved"), dir.join("renamed-by-hand")).expect("outside rename");
    let refusal = undo_last_move(&history).expect_err("a stale path refuses");
    let text = refusal.to_string();
    assert!(matches!(refusal, UndoError::SourceMissing(_)), "{refusal}");
    assert!(
        text.contains("moved") && text.contains("nothing was changed"),
        "the message names the recorded path and says nothing changed: {text}"
    );
    assert!(dir.join("renamed-by-hand/child.txt").is_file());
    assert!(!folder.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v096_the_journal_records_a_path_that_resolves_from_anywhere() {
    // `std::env::temp_dir()` is a symlinked route on macOS (`/var/folders/...`
    // resolves to `/private/var/folders/...`), which makes it a deterministic
    // stand-in for "the user typed a path that is not the canonical one".
    let canonical = work_dir("absolute");
    let unresolved =
        std::env::temp_dir().join(format!("kaseifu-v096-absolute-{}", std::process::id()));
    let folder = unresolved.join("tracked");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "child.txt", "a child\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");

    execute_folder_rename(&store, &history, &folder, &literal_options("renamed")).await;
    let rows = history.entries().expect("rows");
    assert_eq!(rows.len(), 1);
    let recorded = Path::new(&rows[0].destination_path);
    assert!(
        recorded.is_absolute(),
        "the journal must not depend on the working directory: {}",
        rows[0].destination_path
    );
    assert_eq!(recorded, canonical.join("renamed"));
    let undo = undo_last_move(&history).expect("undo");
    assert_eq!(undo.restored_to, canonical.join("tracked"));
    let _ = std::fs::remove_dir_all(&canonical);
}

// ---------------------------------------------------------------------------
// W10: TOCTOU recheck for a folder
// ---------------------------------------------------------------------------
#[tokio::test]
async fn v096_dir_w10_the_recheck_follows_the_folder_identity() {
    let dir = work_dir("toctou");
    let folder = dir.join("target");
    std::fs::create_dir_all(&folder).expect("folder");
    write_child(&folder, "child.txt", "a child\n");
    let store = RenameStore::open_in_memory().expect("store");
    let history = OperationHistory::open_in_memory().expect("history");
    let bridge = FakeFmBridge::answering(info("unused"));
    let plan = ready(
        plan_rename(
            &provider(&bridge),
            &store,
            &folder,
            &literal_options("proposal"),
        )
        .await
        .expect("plan"),
    );

    // A child added between proposal and execution is legitimate: the same
    // folder, a new name. The rename must not be refused for it.
    write_child(&folder, "added-later.txt", "more content\n");
    let operation_id = execute_plan(&plan, &history).expect("execute");
    assert!(!operation_id.is_empty());
    assert!(dir.join("proposal/added-later.txt").is_file());

    // A different folder at the same path is a different object.
    let replaced = dir.join("proposal");
    let bridge = FakeFmBridge::answering(info("unused"));
    let plan = ready(
        plan_rename(
            &provider(&bridge),
            &store,
            &replaced,
            &literal_options("second"),
        )
        .await
        .expect("plan"),
    );
    std::fs::remove_dir_all(&replaced).expect("remove");
    std::fs::create_dir_all(&replaced).expect("recreate");
    write_child(&replaced, "other.txt", "not ours\n");
    let refusal = execute_plan(&plan, &history).expect_err("a swapped folder refuses");
    assert!(refusal.to_string().contains("source changed"), "{refusal}");
    assert!(
        replaced.join("other.txt").is_file(),
        "the swap is untouched"
    );
    assert!(!dir.join("second").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// D8 / W11: refusals, the messages, and the CLI surfaces
// ---------------------------------------------------------------------------

#[test]
fn v096_the_folder_refusals_cover_the_shapes_kaseifu_must_not_touch() {
    let env = RefusalEnv {
        home: Some(PathBuf::from("/Users/test")),
        cwd: Some(PathBuf::from("/Users/test/dev/repo")),
    };
    for (path, expected) in [
        ("/", DirectoryRefusal::VolumeRoot),
        ("/System", DirectoryRefusal::SystemRoot),
        ("/tmp", DirectoryRefusal::SystemRoot),
        ("/Users/test", DirectoryRefusal::Home),
        ("/Users/test/dev", DirectoryRefusal::WorkingDirectory),
        (
            "/Users/test/Downloads/Installer.pkg",
            DirectoryRefusal::Bundle,
        ),
        (
            "/Users/test/Library/Application Support/Tool",
            DirectoryRefusal::Library,
        ),
        (
            "/Volumes/Archive/.Spotlight-V100",
            DirectoryRefusal::SystemArea,
        ),
        (
            "/Volumes/Work/TemporaryItems",
            DirectoryRefusal::CacheOrTemp,
        ),
        ("/Users/test/Downloads/.hidden", DirectoryRefusal::Hidden),
    ] {
        assert_eq!(
            refuse_reason(Path::new(path), &env),
            Some(expected),
            "must be refused: {path}"
        );
    }
    for path in [
        "/Users/test/Documents/\u{65b0}\u{3057}\u{3044}\u{30d5}\u{30a9}\u{30eb}\u{30c0}",
        "/tmp/scratch/new-folder",
        "/var/folders/7t/xt8rcl791xzb8qnjnrhbx1tr0000gn/T/kaseifu/work",
        "/Users/test/Documents/Library/images",
    ] {
        assert_eq!(
            refuse_reason(Path::new(path), &env),
            None,
            "must stay: {path}"
        );
    }
}

#[tokio::test]
async fn v096_a_refused_folder_fails_fast_with_its_reason() {
    let dir = work_dir("refused");
    let store = RenameStore::open_in_memory().expect("store");
    let bridge = FakeFmBridge::answering(info("never used"));
    let plans = provider(&bridge);

    // An app bundle is refused with the bundle reason.
    let bundle = dir.join("Something.app");
    std::fs::create_dir_all(&bundle).expect("bundle");
    let refusal = plan_rename(&plans, &store, &bundle, &title_options("P01"))
        .await
        .expect_err("a bundle is refused");
    assert!(refusal.to_string().contains("app-managed"), "{refusal}");
    assert!(bundle.is_dir(), "nothing moved");

    // The working directory itself is refused (the test runs in the crate
    // root), because renaming it would strand the process.
    let cwd = std::env::current_dir().expect("cwd");
    let refusal = plan_rename(&plans, &store, &cwd, &title_options("P01"))
        .await
        .expect_err("the working directory is refused");
    assert!(
        refusal.to_string().contains("working directory"),
        "{refusal}"
    );

    // A file is never a folder target.
    let file = write_child(&dir, "note.txt", "a note\n");
    let refusal = classify_directory(&file).expect_err("a file is not a folder");
    assert_eq!(
        refusal,
        DirectoryError::NotADirectory(format!("{} is not a folder", file.display()))
    );
    assert!(bridge.requests().is_empty(), "refusals spend no inference");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v096_the_stale_extension_less_message_now_names_folders_and_media() {
    let error = classify_document(Path::new("/tmp/untitled file"))
        .expect_err("an extension-less path is not a document");
    let text = error.to_string();
    assert!(
        text.contains("is not a folder"),
        "the message must not pretend it looked for a folder: {text}"
    );
    assert!(text.contains("an image"), "{text}");
    assert!(text.contains("a video"), "{text}");
    assert!(
        !text.contains("rename needs txt, md, pdf or html"),
        "the stale wording is gone: {text}"
    );
    assert!(text.is_ascii(), "{text}");
}

// ---------------------------------------------------------------------------
// W11: the CLI (only the paths that never need the model)
// ---------------------------------------------------------------------------

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
fn v096_cli_an_empty_folder_is_reported_as_held_without_a_model() {
    let dir = work_dir("cli-empty");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let folder = dir.join("untitled folder");
    std::fs::create_dir_all(&folder).expect("folder");

    let output = cli(&dir, &home, &["rename", "untitled folder"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("status: held"), "{text}");
    assert!(
        text.contains("reason: the folder has nothing to name"),
        "{text}"
    );
    assert!(folder.is_dir(), "nothing moved");

    // The JSON shape keeps every existing key and adds `directory` (null here:
    // a held plan carries no report).
    let output = cli(
        &dir,
        &home,
        &["rename", "untitled folder", "--format", "json"],
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout_of(&output)).expect("one JSON object");
    let rename = json["rename"].as_object().expect("the rename object");
    let mut keys: Vec<&str> = rename.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "current_name",
            "date_source",
            "directory",
            "operation_id",
            "pattern",
            "perception",
            "proposal",
            "reason",
            "status",
            "target",
            "video",
        ],
        "the existing ten keys survive and directory is added"
    );
    assert!(rename["directory"].is_null());
    assert_eq!(rename["status"], serde_json::json!("held"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v096_cli_refuses_a_bundle_and_names_a_folder_in_the_guidance() {
    let dir = work_dir("cli-refused");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let bundle = dir.join("Example.app");
    std::fs::create_dir_all(&bundle).expect("bundle");

    let output = cli(&dir, &home, &["rename", "Example.app"]);
    assert!(!output.status.success(), "a refused folder exits non-zero");
    let text = stderr_of(&output);
    assert!(text.contains("app-managed"), "{text}");
    assert!(bundle.is_dir(), "nothing moved");

    // An extension-less non-folder is still refused, with the fixed message.
    write_child(&dir, "untitled file", "no extension\n");
    let output = cli(&dir, &home, &["rename", "untitled file"]);
    assert!(!output.status.success());
    let text = stderr_of(&output);
    assert!(text.contains("is not a folder"), "{text}");
    assert!(text.contains("an image or a video"), "{text}");

    // The guidance names folders as a target and stays ASCII.
    let output = cli(&dir, &home, &["rename"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(
        text.contains("folders: named from what is inside them"),
        "{text}"
    );
    assert!(text.is_ascii(), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v096_cli_help_names_folders_without_internal_wording() {
    let dir = work_dir("cli-help");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    for args in [
        vec!["rename", "--help"],
        vec!["rename", "-h"],
        vec!["--help"],
    ] {
        let output = cli(&dir, &home, &args);
        assert!(output.status.success(), "{args:?}");
        let text = stdout_of(&output);
        assert!(text.is_ascii(), "{args:?}: {text}");
        assert!(
            text.contains("folder"),
            "{args:?} must mention folders: {text}"
        );
        for forbidden in [
            "Phase",
            "NormalizedFileContext",
            "DirectoryFacts",
            "TargetKind",
        ] {
            assert!(!text.contains(forbidden), "{args:?} leaks {forbidden}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// S1 / S2 / C3: the Swift prompt, the schema, and prompt byte stability
// ---------------------------------------------------------------------------

fn swift_bridge_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("swift/kaseifu_fm_bridge.swift");
    std::fs::read_to_string(&path).expect("swift bridge source is readable")
}

#[test]
fn v096_the_prompt_names_the_collection_and_never_the_folder_name() {
    let source = swift_bridge_source();
    // The folder branch, its instruction lines, and the bounded data lines.
    for required in [
        "if targetType == \"directory\" {",
        "The target is a folder. Name the collection from what is inside it",
        "Write the title as one short natural \\(language) phrase that names the collection",
        "If the files inside have no common theme, leave the title empty instead of inventing one.",
        "lines.append(\"folder_size: ",
        "lines.append(\"folder_files: ",
        "lines.append(\"folder_folders: ",
        "lines.append(\"folder_types: ",
        "let directoryInstructions = baseInstructions + \" The target is a folder",
    ] {
        assert!(
            source.contains(required),
            "the prompt must keep: {required}"
        );
    }
    // The folder's own name is rendered for every other kind of target only:
    // exactly one filename line, inside the `else` branch of the folder gate.
    assert_eq!(
        source.matches("lines.append(\"filename: ").count(),
        1,
        "one filename line, not two"
    );
    let gate = source
        .find("if targetType == \"directory\" {")
        .expect("gate");
    let filename = source
        .find("lines.append(\"filename: ")
        .expect("filename line");
    assert!(
        filename > gate && source[gate..filename].contains("} else {"),
        "the filename line lives in the non-folder branch"
    );
    // The folder instructions and data stay before the signals line so the
    // 2000 character budget cannot cut them away.
    let size = source.find("lines.append(\"folder_size: ").expect("size");
    let signals = source.find("lines.append(\"signals:").expect("signals");
    assert!(size < signals, "folder facts stay before signals");
    assert!(
        source.contains("read from files inside the folder, not from the folder's name"),
        "the signals line says where the excerpts came from"
    );
    // Nothing in the folder path writes or follows anything new.
    assert_eq!(
        source
            .matches("@_cdecl(\"kaseifu_fm_free_result\")")
            .count(),
        1,
        "the folder path reuses the existing ABI"
    );
}

#[test]
fn v096_the_schema_documents_directory_without_requiring_it() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas/fm-rename-v1.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("schema file"))
            .expect("valid JSON");
    assert_eq!(schema["$id"], serde_json::json!("fm-rename-v1"));
    let request = schema["oneOf"]
        .as_array()
        .expect("oneOf")
        .iter()
        .find(|branch| branch["title"] == serde_json::json!("Request"))
        .expect("the request branch");
    let directory = &request["properties"]["input"]["properties"]["directory"];
    assert!(!directory.is_null(), "directory is documented");
    let required = request["properties"]["input"]
        .get("required")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    assert!(
        !required
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("directory")),
        "directory stays optional"
    );
    assert_eq!(
        directory["properties"]["files"]["maxItems"],
        serde_json::json!(12)
    );
    assert_eq!(
        directory["properties"]["folders"]["maxItems"],
        serde_json::json!(8)
    );
    assert_eq!(
        directory["properties"]["extensions"]["maxItems"],
        serde_json::json!(8)
    );
}

#[test]
fn v096_documents_images_and_videos_still_never_carry_folder_facts() {
    // The prompt gates are `target_type`, and the request only carries the
    // folder key when the observation has folder facts: this pins the halves of
    // that contract a source scan can reach (the byte-level prompt test for
    // documents lives in v093/v095 and still passes unchanged).
    let source = swift_bridge_source();
    assert!(
        !source.contains("if targetType == \"document\" {\n        lines.append(\"folder"),
        "folder lines are gated on the folder target only"
    );
    let context = NormalizedFileContext {
        target: TargetFile {
            path: PathBuf::from("/tmp/clip.mp4"),
            filename: "clip.mp4".to_string(),
            extension: Some("mp4".to_string()),
            ..TargetFile::default()
        },
        parent_directory: "/tmp".to_string(),
        ..NormalizedFileContext::default()
    };
    let json = serde_json::to_value(build_request(&context, "rename-0003", "ja")).expect("json");
    assert!(json["input"].get("directory").is_none());
    assert_eq!(json["target_type"], serde_json::json!("video"));
}

/// The reader keeps `DirectoryEntry` values in the shape the fixture contract
/// expects; pinned here so a model change is a deliberate one.
#[test]
fn v096_the_entry_shape_the_reader_consumes_is_unchanged() {
    let entry = DirectoryEntry {
        name: "child.txt".to_string(),
        kind: EntryKind::File,
    };
    let json = serde_json::to_value(&entry).expect("json");
    assert_eq!(json["name"], serde_json::json!("child.txt"));
    assert_eq!(json["kind"], serde_json::json!("file"));
}

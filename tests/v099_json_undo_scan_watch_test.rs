//! v0.9.9 `undo` / `scan` / `watch` `--format json` tests (M1).
//!
//! `undo` and `scan` print one parseable document; `watch` prints JSON Lines
//! (one object per line). Text output is unchanged. Every child runs with an
//! isolated HOME in a temporary directory; the developer's real journal is
//! never opened. Runs are bounded by a deadline so a hang fails the test.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use kaseifu::presentation::renderers::generic;
use kaseifu::storage::history::OperationHistory;
use kaseifu::watch::runner::{ProcessOutcome, outcome_json};

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-json-usw-{}-{name}", std::process::id()));
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

/// Run to completion with stdin closed (non-TTY) and a deadline.
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

/// Journal path the binary resolves for an isolated HOME.
fn isolated_journal(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("kaseifu.db")
}

/// Record one successful move of `name` from `from_dir` to `to_dir`: the file
/// ends up at its destination exactly as a real move would leave it, so
/// `undo` can restore it.
fn journaled_move(home: &Path, from_dir: &Path, to_dir: &Path, name: &str) {
    let source = from_dir.join(name);
    let destination = to_dir.join(name);
    std::fs::create_dir_all(from_dir).expect("from dir");
    std::fs::create_dir_all(to_dir).expect("to dir");
    std::fs::write(&source, b"fixture").expect("fixture file");
    let history = OperationHistory::open(isolated_journal(home)).expect("journal");
    let id = history
        .record_pending("move", &source, &destination, None, None, None)
        .expect("record move");
    std::fs::rename(&source, &destination).expect("simulate the move");
    history.mark_success(&id).expect("mark success");
}

// ---------------------------------------------------------------------------
// `undo --format json`
// ---------------------------------------------------------------------------

#[test]
fn undo_json_on_an_empty_journal_is_one_parseable_empty_document() {
    let dir = temp_dir("undo-empty");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let output = run(&dir, &home, &["--format", "json", "undo"]);
    assert!(
        output.status.success(),
        "undo must exit 0: {}",
        stderr_of(&output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout is one JSON document");
    assert_eq!(json["undo"]["status"], "empty", "got: {json}");
    assert_eq!(json["undo"]["restored"], serde_json::json!([]));
    assert_eq!(json["undo"]["pending_warning"], false);
}

#[test]
fn undo_text_on_an_empty_journal_is_unchanged() {
    let dir = temp_dir("undo-empty-text");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let output = run(&dir, &home, &["undo"]);
    assert!(output.status.success());
    assert!(
        stdout_of(&output).contains("nothing left to undo"),
        "got: {}",
        stdout_of(&output)
    );
}

#[test]
fn undo_json_restores_one_move_and_reports_its_ids() {
    let dir = temp_dir("undo-one");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let from_dir = dir.join("from");
    let to_dir = dir.join("to");
    journaled_move(&home, &from_dir, &to_dir, "note.txt");

    let output = run(&dir, &home, &["--format", "json", "undo"]);
    assert!(
        output.status.success(),
        "undo must exit 0: {}",
        stderr_of(&output)
    );
    assert!(from_dir.join("note.txt").exists(), "the file is restored");
    assert!(!to_dir.join("note.txt").exists(), "the move is reversed");
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout is one JSON document");
    assert_eq!(json["undo"]["status"], "restored", "got: {json}");
    let restored = json["undo"]["restored"].as_array().expect("restored array");
    assert_eq!(restored.len(), 1);
    assert!(
        restored[0]["to"]
            .as_str()
            .is_some_and(|to| to.ends_with("note.txt"))
    );
    assert!(
        !restored[0]["undone_move_id"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );
    assert!(
        !restored[0]["operation_id"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );
}

#[test]
fn undo_json_last_two_with_one_move_is_partial() {
    let dir = temp_dir("undo-partial");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let from_dir = dir.join("from");
    let to_dir = dir.join("to");
    journaled_move(&home, &from_dir, &to_dir, "note.txt");

    let output = run(&dir, &home, &["--format", "json", "undo", "--last", "2"]);
    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout is one JSON document");
    assert_eq!(json["undo"]["status"], "partial", "got: {json}");
    assert_eq!(json["undo"]["restored"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["undo"]["failures"].as_array().map(Vec::len), Some(1));
}

#[test]
fn undo_json_keeps_stdout_parseable_when_a_pending_row_exists() {
    let dir = temp_dir("undo-pending");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let history = OperationHistory::open(isolated_journal(&home)).expect("journal");
    history
        .record_pending(
            "move",
            Path::new("/nowhere/a.txt"),
            Path::new("/nowhere/b.txt"),
            None,
            None,
            None,
        )
        .expect("pending row");

    let output = run(&dir, &home, &["--format", "json", "undo"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_str(stdout_of(&output).trim())
        .expect("stdout stays one JSON document with a pending row");
    assert_eq!(json["undo"]["pending_warning"], true, "got: {json}");
    assert!(
        stderr_of(&output).contains("pending journal"),
        "the warning moves to stderr: {}",
        stderr_of(&output)
    );
}

// ---------------------------------------------------------------------------
// `scan --format json`
// ---------------------------------------------------------------------------

#[test]
fn scan_json_is_one_parseable_honest_document() {
    let dir = temp_dir("scan");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let target = dir.join("some-folder");
    std::fs::create_dir_all(&target).expect("target");

    let output = run(
        &dir,
        &home,
        &["--format", "json", "scan", target.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "scan must exit 0: {}",
        stderr_of(&output)
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("stdout is one JSON document");
    assert_eq!(json["scan"]["status"], "not-implemented", "got: {json}");
    assert!(
        json["scan"]["target"]
            .as_str()
            .is_some_and(|target| target.ends_with("some-folder"))
    );
}

#[test]
fn scan_text_is_unchanged() {
    let dir = temp_dir("scan-text");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let output = run(&dir, &home, &["scan", "/tmp"]);
    assert!(output.status.success());
    assert!(
        stdout_of(&output).contains("Scanning: /tmp"),
        "got: {}",
        stdout_of(&output)
    );
}

// ---------------------------------------------------------------------------
// `watch --format json` (JSON Lines helpers + the real start line)
// ---------------------------------------------------------------------------

#[test]
fn watch_start_json_is_one_single_line_object() {
    let line = generic::watch_start_json(&["/tmp/a".to_string(), "/tmp/b".to_string()], true);
    assert!(!line.contains('\n'), "JSON Lines: exactly one line");
    let json: serde_json::Value = serde_json::from_str(&line).expect("parses");
    assert_eq!(json["watch"]["event"], "start");
    assert_eq!(json["watch"]["mode"], "dry-run");
    assert_eq!(
        json["watch"]["roots"],
        serde_json::json!(["/tmp/a", "/tmp/b"])
    );
    let execute = generic::watch_start_json(&["/tmp/a".to_string()], false);
    let json: serde_json::Value = serde_json::from_str(&execute).expect("parses");
    assert_eq!(json["watch"]["mode"], "execute");
}

#[test]
fn watch_event_and_outcome_json_are_single_line_objects() {
    for event in ["detected", "ignored", "moved", "would-move", "refused"] {
        let line = generic::watch_event_json(event, "/tmp/f.txt", Some("detail text"));
        assert!(!line.contains('\n'), "{event} stays one line");
        let json: serde_json::Value = serde_json::from_str(&line).expect("parses");
        assert_eq!(json["watch"]["event"], event);
        assert_eq!(json["watch"]["path"], "/tmp/f.txt");
    }
    let moved = outcome_json(
        Path::new("/tmp/f.txt"),
        &ProcessOutcome::Moved {
            operation_id: "op-1".to_string(),
            destination: PathBuf::from("/tmp/d/f.txt"),
        },
    );
    assert!(!moved.contains('\n'));
    let json: serde_json::Value = serde_json::from_str(&moved).expect("parses");
    assert_eq!(json["watch"]["event"], "moved");
    assert_eq!(json["watch"]["path"], "/tmp/f.txt");
    assert!(
        json["watch"]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("op-1"))
    );
}

/// The real `watch` start banner with `--format json` is one parseable line.
/// The loop never ends, so the first stdout line is read by a helper thread
/// and the child is killed afterwards.
#[test]
fn watch_json_start_banner_is_parseable_from_the_real_binary() {
    let dir = temp_dir("watch-banner");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let watched = dir.join("watched");
    std::fs::create_dir_all(&watched).expect("watched dir");

    let mut command = base_command(&dir, &home);
    command
        .args([
            "--format",
            "json",
            "watch",
            "--path",
            watched.to_str().unwrap(),
        ])
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("JEV_API_KEY", "test-key-not-a-secret-0123456789")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("watch spawns");
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = sender.send(line);
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("the start banner arrives without input");
    let _ = child.kill();
    let _ = child.wait();
    assert!(!line.is_empty(), "watch printed its start banner");
    let json: serde_json::Value = serde_json::from_str(line.trim()).expect("start line parses");
    assert_eq!(json["watch"]["event"], "start", "got: {json}");
    assert_eq!(json["watch"]["mode"], "dry-run", "got: {json}");
}

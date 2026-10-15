//! v1.0.2 `organize --to <dir>` tests (M3 ZC-14 `rename-to-directory`).
//!
//! The move is explicit and non-interactive: no terminal is needed, no
//! confirmation is asked, and the journal row lets `undo` reverse it.
//! Every child runs with an isolated HOME in a temporary directory; the
//! developer's real files, journal and Keychain are never touched. Runs are
//! bounded by a deadline so a hang fails the test.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-to-{name}-{}", std::process::id()));
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

/// Run with stdin closed (non-TTY) and a deadline.
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

fn combined_of(output: &Output) -> String {
    format!(
        "{}{}",
        stdout_of(output),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn isolated_journal(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("kaseifu.db")
}

struct Fixture {
    dir: PathBuf,
    home: PathBuf,
    inbox: PathBuf,
    target: PathBuf,
    dest: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = temp_dir(name);
        let home = dir.join("home");
        let inbox = dir.join("inbox");
        let dest = dir.join("archive");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&inbox).expect("inbox");
        std::fs::create_dir_all(&dest).expect("dest");
        let target = inbox.join("memo.txt");
        std::fs::write(&target, b"fixture").expect("fixture file");
        Self {
            dir,
            home,
            inbox,
            target,
            dest,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        run(&self.dir, &self.home, args)
    }

    fn to_args(&self, extra: &[&str]) -> Vec<String> {
        // Relative destinations: the temp tree lives under `/var/folders`,
        // which the destination rules refuse as a system root tree (correct
        // for real use). A relative path never matches that rule, and the
        // binary resolves it against the fixture cwd, like real use from
        // any working directory.
        let mut args = vec![
            "organize".to_string(),
            self.target.to_str().unwrap().to_string(),
            "--to".to_string(),
            "archive".to_string(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_string()));
        args
    }

    fn run_to(&self, extra: &[&str]) -> Output {
        let args = self.to_args(extra);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&refs)
    }
}

#[test]
fn to_dry_run_reports_the_plan_and_moves_nothing() {
    let fixture = Fixture::new("dry-run");
    let output = fixture.run_to(&[]);
    assert!(output.status.success(), "stderr: {}", combined_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("memo.txt"), "{text}");
    assert!(text.contains("archive"), "the destination is named: {text}");
    assert!(fixture.target.exists(), "nothing moved");
    assert!(!fixture.dest.join("memo.txt").exists(), "nothing arrived");
    assert!(
        !isolated_journal(&fixture.home).exists(),
        "no journal was written"
    );
}

#[test]
fn to_execute_moves_and_undo_restores() {
    let fixture = Fixture::new("execute");
    let output = fixture.run_to(&["--execute"]);
    assert!(output.status.success(), "stderr: {}", combined_of(&output));
    assert!(!fixture.target.exists(), "the source is gone");
    assert!(fixture.dest.join("memo.txt").exists(), "the file arrived");
    assert!(
        isolated_journal(&fixture.home).exists(),
        "the move was journaled"
    );
    let undo = fixture.run(&["undo"]);
    assert!(undo.status.success(), "stderr: {}", combined_of(&undo));
    assert!(fixture.target.exists(), "undo restores the source");
    assert!(
        !fixture.dest.join("memo.txt").exists(),
        "undo reverses the move"
    );
}

#[test]
fn to_json_is_parseable_for_dry_run_and_execute() {
    let fixture = Fixture::new("json");
    let output = fixture.run_to(&["--format", "json"]);
    assert!(output.status.success(), "stderr: {}", combined_of(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("one JSON document");
    assert_eq!(json["organize_to"]["executed"], false, "got: {json}");
    assert!(json["organize_to"]["operation_id"].is_null(), "got: {json}");
    assert!(fixture.target.exists(), "dry run moves nothing");

    let moved = fixture.run_to(&["--format", "json", "--execute"]);
    assert!(moved.status.success(), "stderr: {}", combined_of(&moved));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&moved).trim()).expect("one JSON document");
    assert_eq!(json["organize_to"]["executed"], true, "got: {json}");
    assert!(
        json["organize_to"]["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "got: {json}"
    );
    assert!(fixture.dest.join("memo.txt").exists(), "the file arrived");
}

#[test]
fn to_excluded_destination_is_refused_with_a_reason() {
    let fixture = Fixture::new("excluded");
    let banned = Path::new("Something.app");
    std::fs::create_dir_all(fixture.dir.join(banned)).expect("excluded dir");
    let output = fixture.run(&[
        "organize",
        fixture.target.to_str().unwrap(),
        "--to",
        banned.to_str().unwrap(),
    ]);
    assert!(
        !output.status.success(),
        "an excluded destination must fail"
    );
    assert!(
        combined_of(&output).contains("not a usable folder"),
        "the reason is named: {}",
        combined_of(&output)
    );
    assert!(fixture.target.exists(), "nothing moved");
}

#[test]
fn to_missing_or_non_directory_is_refused() {
    let fixture = Fixture::new("missing");
    let missing = "no-such-dir";
    let output = fixture.run(&[
        "organize",
        fixture.target.to_str().unwrap(),
        "--to",
        missing,
    ]);
    assert!(!output.status.success());
    assert!(
        combined_of(&output).contains("not a directory"),
        "got: {}",
        combined_of(&output)
    );
    let output = fixture.run(&[
        "organize",
        fixture.target.to_str().unwrap(),
        "--to",
        fixture.target.to_str().unwrap(),
    ]);
    assert!(!output.status.success(), "a file is not a directory");
    assert!(fixture.target.exists(), "nothing moved");
}

#[test]
fn to_own_parent_is_a_noop_refusal() {
    let fixture = Fixture::new("noop");
    let output = fixture.run(&[
        "organize",
        fixture.target.to_str().unwrap(),
        "--to",
        fixture.inbox.file_name().unwrap().to_str().unwrap(),
    ]);
    assert!(!output.status.success(), "a no-op move must fail");
    assert!(
        combined_of(&output).contains("nothing to move"),
        "got: {}",
        combined_of(&output)
    );
    assert!(fixture.target.exists(), "nothing moved");
    assert!(
        !isolated_journal(&fixture.home).exists(),
        "no journal was written"
    );
}

#[test]
fn to_conflicts_with_select() {
    let fixture = Fixture::new("conflict");
    let output = fixture.run(&[
        "organize",
        fixture.target.to_str().unwrap(),
        "--select",
        "--to",
        fixture.dest.to_str().unwrap(),
    ]);
    assert!(!output.status.success(), "--to and --select conflict");
    assert!(fixture.target.exists(), "nothing moved");
}

//! v113 file-level targets: `organize` and `classify` refuse a folder.
//!
//! Issue #14 (real use of 0.13.0, 2026-10-21): a folder given to `organize` ran
//! the file pipeline and printed a plausible verdict (exit 0, "no move"), and
//! only the plan layer refused to move anything. The type check now runs at the
//! boundary - before the repository policy, the API key, the journal and the
//! network - and the refusal names the command that takes folders (`label`).
//! `watch` still takes a directory (it watches a tree rather than naming one
//! target) and `inspect` still observes a folder on purpose.
//!
//! Every test runs in a sandbox below `target/` (the pattern the repository
//! policy tests use: a destination inside the macOS temp tree is refused as a
//! never-a-destination folder, so a plan can only be built here) with an
//! isolated HOME and stdin closed: no user file, journal or completion
//! directory is involved. No run may need an API key - a run that got past the
//! boundary check would report the missing key instead of the folder, which is
//! what the assertions look for.

use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Every sandbox gets its own directory: the test binary runs its tests in
/// parallel, and the sandboxes are removed when their test ends.
static RUN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// A rule whose pattern matches a folder path as well as a file path, so the
/// tests can tell "refused at the boundary" from "decided by the rule".
const RULE_POLICY: &str = r#"[[rule]]
name = "site-owned-content"
match_glob = "content/{project}/**"
match_contains = "sites/thch-vape"
destination = "sites/<project>/content/"
reason = "content kept under a site folder belongs to that site"
"#;

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout_of(output)).expect("the report is one JSON document")
}

/// Spawn one command with stdin closed and collect it inside a deadline.
fn wait_for(mut command: ProcessCommand) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kaseifu spawns");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                panic!("kaseifu exceeded the deadline");
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

/// One sandbox: a repository to work in plus an isolated HOME, deleted when the
/// test ends (the completion update is disabled, as every test does).
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let unique = RUN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "v113-folder-target-{name}-{}-{unique}",
                std::process::id()
            ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).expect("sandbox");
        std::fs::create_dir_all(dir.join("repo")).expect("repository");
        Self { dir }
    }

    /// The repository the tests build their trees in.
    fn repo(&self) -> PathBuf {
        self.dir.join("repo")
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    /// Create a directory below the repository and return it.
    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.repo().join(relative);
        std::fs::create_dir_all(&path).expect("directory");
        path
    }

    /// Create a file below the repository and return it.
    fn file(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.repo().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(&path, body).expect("file");
        path
    }

    /// The journal a run that opened the history would create here.
    fn journal(&self) -> PathBuf {
        self.home()
            .join("Library/Application Support/kaseifu/kaseifu.db")
    }

    /// Run kaseifu in the sandbox's repository with an isolated HOME.
    fn run(&self, args: &[&str]) -> Output {
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
        command
            .current_dir(self.repo())
            .env("HOME", self.home())
            .env("KASEIFU_KEY_SOURCE", "env")
            .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
            .env_remove("JEV_API_KEY")
            .args(args);
        wait_for(command)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn v113_organize_refuses_a_folder_before_the_policy_and_the_key() {
    let sandbox = Sandbox::new("organize-folder");
    // The folder's own path matches the rule: without the boundary check this
    // run reports a rule hit with exit 0 and only the plan layer refuses.
    let folder = sandbox.dir("content/proj/sites/thch-vape");
    sandbox.dir("sites/proj/content");
    sandbox.file(".kaseifu.toml", RULE_POLICY);
    let folder_arg = folder.to_str().expect("path");

    let output = sandbox.run(&["organize", folder_arg]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}",
        stdout_of(&output)
    );
    let stderr = stderr_of(&output);
    assert!(stderr.contains("organize works on one file"), "{stderr}");
    assert!(stderr.contains("`kaseifu label <folder>`"), "{stderr}");
    // Refused before the policy is read and before the key is resolved: a run
    // that got past the check would report the missing key here.
    assert!(!stderr.contains("API key"), "{stderr}");
    assert!(!sandbox.journal().exists(), "the journal stays closed");
    assert!(folder.is_dir(), "the folder is untouched");
}

#[test]
fn v113_every_organize_shape_refuses_a_folder() {
    let sandbox = Sandbox::new("organize-shapes");
    let folder = sandbox.dir("plain");
    let destination = sandbox.dir("sites");
    let folder_arg = folder.to_str().expect("path");
    let destination_arg = destination.to_str().expect("path");

    let shapes: [&[&str]; 6] = [
        &["organize", folder_arg],
        &["organize", folder_arg, "--execute"],
        &["organize", folder_arg, "--to", destination_arg, "--execute"],
        &["organize", folder_arg, "--select"],
        &["organize", folder_arg, "--select", "--format", "json"],
        &["organize", folder_arg, "--keywords", "cassette"],
    ];
    for args in shapes {
        let output = sandbox.run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            stderr_of(&output).contains("organize works on one file"),
            "{args:?}: {}",
            stderr_of(&output)
        );
        assert!(folder.is_dir(), "{args:?} moved the folder");
    }
    assert!(
        std::fs::read_dir(&destination)
            .expect("destination")
            .next()
            .is_none(),
        "nothing may arrive in the destination"
    );
}

#[test]
fn v113_classify_refuses_a_folder_while_inspect_still_reads_one() {
    let sandbox = Sandbox::new("classify-folder");
    let folder = sandbox.dir("plain");
    let folder_arg = folder.to_str().expect("path");

    let output = sandbox.run(&["classify", folder_arg]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}",
        stdout_of(&output)
    );
    let stderr = stderr_of(&output);
    assert!(stderr.contains("classify works on one file"), "{stderr}");
    assert!(stderr.contains("`kaseifu label <folder>`"), "{stderr}");
    assert!(!stderr.contains("API key"), "{stderr}");

    // `inspect` observes a folder on purpose: directory entries, metadata and
    // the parent are exactly what it reports about one.
    let inspected = sandbox.run(&["inspect", folder_arg]);
    assert!(inspected.status.success(), "{}", stderr_of(&inspected));
    assert!(
        stdout_of(&inspected).contains("plain"),
        "{}",
        stdout_of(&inspected)
    );
}

#[test]
fn v113_a_file_is_still_decided_by_its_rule() {
    let sandbox = Sandbox::new("file-still-works");
    let file = sandbox.file("content/proj/sites/thch-vape/brief.md", "brief\n");
    sandbox.dir("sites/proj/content");
    sandbox.file(".kaseifu.toml", RULE_POLICY);
    let file_arg = file.to_str().expect("path");

    let output = sandbox.run(&["organize", file_arg, "--format", "json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let value = json_of(&output);
    assert_eq!(
        value["rule"]["name"],
        serde_json::json!("site-owned-content")
    );
    assert_eq!(value["outcome"]["type"], serde_json::json!("would_move"));
    assert_eq!(value["mode"], serde_json::json!("dry_run"));
    assert!(file.exists(), "a dry run moves nothing");
    // The journal the rule hit opens lives in the sandbox HOME: the run touched
    // nothing of the machine that started it.
    assert!(sandbox.journal().exists(), "the sandbox journal");
}

#[test]
fn v113_help_states_that_the_target_is_one_file() {
    let sandbox = Sandbox::new("help");
    for command in ["organize", "classify"] {
        let output = sandbox.run(&[command, "--help"]);
        assert!(output.status.success(), "{command} --help");
        let text = stdout_of(&output);
        assert!(
            text.contains("one regular file"),
            "{command} --help does not state the target: {text}"
        );
    }
}

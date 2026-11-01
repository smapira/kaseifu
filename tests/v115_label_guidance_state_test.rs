//! v115 `label` guidance touches no state.
//!
//! Issue `20261016-open-label-guidance-touches-state` (Homebrew Formula test,
//! 2026-10-16, reproduced again 2026-11-01): `kaseifu label` without an argument
//! only prints how to use the command, yet it created
//! `~/Library/Application Support/kaseifu/kaseifu.db`, because `handle_rename`
//! opened the store before it knew the request. The guidance path now returns
//! before the store is opened, and a management flag combined with a target is
//! refused before it too; the paths that read or write the store are unchanged.
//!
//! Every test runs in a sandbox below `target/` with an isolated HOME and stdin
//! closed, so nothing of the machine that runs it is touched.

use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static RUN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
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

/// One sandbox: a working directory plus an isolated HOME, removed at the end.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let unique = RUN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "v115-label-state-{name}-{}-{unique}",
                std::process::id()
            ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).expect("sandbox");
        std::fs::create_dir_all(dir.join("work")).expect("working directory");
        Self { dir }
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    /// The journal the run would create, and the directory that holds it.
    fn journal(&self) -> PathBuf {
        self.home()
            .join("Library/Application Support/kaseifu/kaseifu.db")
    }

    fn file(&self, relative: &str) -> PathBuf {
        let path = self.work().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(&path, b"fixture").expect("file");
        path
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
        command
            .current_dir(self.work())
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
fn v115_label_guidance_creates_no_state() {
    let sandbox = Sandbox::new("guidance");

    let output = sandbox.run(&["label"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        stdout_of(&output).contains("kaseifu label <file-or-folder>"),
        "got:\n{}",
        stdout_of(&output)
    );
    assert!(
        !sandbox.journal().exists(),
        "the guidance path must not create the journal"
    );
    assert!(
        !sandbox.journal().parent().expect("parent").exists(),
        "the guidance path must not create the journal directory"
    );
}

#[test]
fn v115_management_flags_still_answer() {
    let sandbox = Sandbox::new("management");
    for flag in ["--patterns", "--show-default"] {
        let output = sandbox.run(&["label", flag]);
        assert!(output.status.success(), "{flag}: {}", stderr_of(&output));
        assert!(
            !stdout_of(&output).trim().is_empty(),
            "{flag} printed nothing"
        );
    }
}

#[test]
fn v115_a_management_flag_with_a_target_is_refused_before_state() {
    let sandbox = Sandbox::new("mixed");
    let file = sandbox.file("note.md");
    let file_arg = file.to_str().expect("path");

    let output = sandbox.run(&["label", file_arg, "--patterns"]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout_of(&output));
    assert!(
        stderr_of(&output).contains("management flags cannot be combined with a target file"),
        "{}",
        stderr_of(&output)
    );
    assert!(
        !sandbox.journal().exists(),
        "a refused request must not create the journal"
    );
}

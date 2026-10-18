//! v109 `kaseifu skill`: the agent skill file this binary carries.
//!
//! The command places the embedded skill at
//! `<repo>/.github/skills/kaseifu/SKILL.md` and compares existing copies with
//! it. Everything runs in a temp sandbox with an isolated HOME and stdin
//! closed: no user file, journal or completion directory is involved.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Every sandbox gets its own directory: the test binary runs its tests in
/// parallel, and the sandboxes are removed when their test ends.
static RUN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// The skill file this repository ships to other repositories, and the bytes
/// the binary built from this checkout carries (both are the same file).
fn canonical_skill() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/skill/kaseifu/SKILL.md"))
        .expect("the canonical skill file")
}

/// The repository's distributed copy of the skill.
fn distributed_skill() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/skills/kaseifu/SKILL.md")
}

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

/// One temp sandbox: a working directory plus an isolated HOME, deleted when
/// the test ends (the completion update is disabled, as every test does).
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let unique = RUN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "kaseifu-skill-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).expect("sandbox");
        Self { dir }
    }

    fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    /// Run kaseifu in this sandbox's working directory.
    fn run(&self, args: &[&str]) -> Output {
        std::fs::create_dir_all(self.work()).expect("working directory");
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

    /// Run kaseifu with the completion upkeep *enabled*, pointed at a directory
    /// inside the sandbox, to observe what a command writes on its own.
    fn run_with_completion_dir(&self, args: &[&str]) -> Output {
        let completions = self.dir.join("completions");
        std::fs::create_dir_all(&completions).expect("completion directory");
        std::fs::create_dir_all(self.work()).expect("working directory");
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
        command
            .current_dir(self.work())
            .env("HOME", self.home())
            .env("KASEIFU_KEY_SOURCE", "env")
            .env("KASEIFU_COMPLETION_DIR", &completions)
            .env_remove("KASEIFU_NO_COMPLETION_UPDATE")
            .env_remove("JEV_API_KEY")
            .args(args);
        wait_for(command)
    }

    /// The zsh script the completion upkeep writes (its absence is the point of
    /// one test).
    fn completion_script(&self) -> PathBuf {
        self.dir.join("completions/_kaseifu")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Create `<work>/<repo>/.github/skills/kaseifu/SKILL.md` with `content`.
fn write_copy(sandbox: &Sandbox, repo: &str, content: &[u8]) -> PathBuf {
    let path = sandbox
        .work()
        .join(repo)
        .join(".github/skills/kaseifu/SKILL.md");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("repository");
    std::fs::write(&path, content).expect("copy");
    path
}

#[test]
fn v109_repository_copy_matches_the_embedded_skill() {
    let sandbox = Sandbox::new("drift");
    let distributed = distributed_skill();
    assert_eq!(
        std::fs::read(&distributed).expect("the distributed copy"),
        canonical_skill(),
        "the repository copy must be the canonical skill file, byte for byte"
    );
    let output = sandbox.run(&[
        "skill",
        "check",
        "--path",
        distributed.to_str().expect("path"),
    ]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        stdout_of(&output).contains("status: match"),
        "{}",
        stdout_of(&output)
    );
}

#[test]
fn v109_preview_writes_nothing_and_reports_a_preview() {
    let sandbox = Sandbox::new("preview");
    let repo = sandbox.work().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");

    let output = sandbox.run(&["skill", "install", "--to", "repo", "--format", "json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let value = json_of(&output);
    assert_eq!(value["skill"]["action"], serde_json::json!("install"));
    assert_eq!(value["skill"]["status"], serde_json::json!("preview"));
    assert!(
        value["skill"]["target"]
            .as_str()
            .expect("target")
            .ends_with(".github/skills/kaseifu/SKILL.md")
    );
    assert_eq!(
        std::fs::read_dir(&repo).expect("repo").count(),
        0,
        "a preview creates nothing, not even the directory chain"
    );
}

#[test]
fn v109_write_installs_the_embedded_skill_and_is_idempotent() {
    let sandbox = Sandbox::new("write");
    std::fs::create_dir_all(sandbox.work()).expect("work");

    let first = sandbox.run(&[
        "skill", "install", "--to", ".", "--write", "--format", "json",
    ]);
    assert!(first.status.success(), "{}", stderr_of(&first));
    assert_eq!(
        json_of(&first)["skill"]["status"],
        serde_json::json!("installed")
    );
    assert_eq!(
        std::fs::read(sandbox.work().join(".github/skills/kaseifu/SKILL.md"))
            .expect("the installed copy"),
        canonical_skill()
    );

    let second = sandbox.run(&["skill", "install", "--to", ".", "--write"]);
    assert!(second.status.success(), "{}", stderr_of(&second));
    assert!(
        stdout_of(&second).contains("status: up-to-date"),
        "{}",
        stdout_of(&second)
    );
}

#[test]
fn v109_write_refuses_an_edited_copy_until_force() {
    let sandbox = Sandbox::new("refuse");
    let installed = write_copy(&sandbox, ".", &canonical_skill());
    std::fs::write(&installed, b"edited by hand\n").expect("edited copy");

    let refused = sandbox.run(&[
        "skill", "install", "--to", ".", "--write", "--format", "json",
    ]);
    assert_eq!(refused.status.code(), Some(1), "{}", stdout_of(&refused));
    assert!(
        stderr_of(&refused).contains("--force"),
        "{}",
        stderr_of(&refused)
    );
    assert_eq!(
        json_of(&refused)["skill"]["status"],
        serde_json::json!("refused")
    );
    assert_eq!(
        std::fs::read(&installed).expect("copy"),
        b"edited by hand\n".to_vec(),
        "a refusal must leave the file exactly as it was"
    );

    let forced = sandbox.run(&["skill", "install", "--to", ".", "--write", "--force"]);
    assert!(forced.status.success(), "{}", stderr_of(&forced));
    assert_eq!(std::fs::read(&installed).expect("copy"), canonical_skill());
}

#[test]
fn v109_check_reports_match_stale_and_a_missing_copy() {
    let sandbox = Sandbox::new("check");
    let installed = write_copy(&sandbox, "repo", &canonical_skill());
    let path = installed.to_str().expect("path");

    let matched = sandbox.run(&["skill", "check", "--path", path]);
    assert!(matched.status.success(), "{}", stderr_of(&matched));
    assert!(stdout_of(&matched).contains("status: match"));

    std::fs::write(&installed, b"edited by hand\n").expect("edited copy");
    let stale = sandbox.run(&["skill", "check", "--path", path, "--format", "json"]);
    assert_eq!(stale.status.code(), Some(1));
    assert!(
        stderr_of(&stale).contains("skill install --to"),
        "{}",
        stderr_of(&stale)
    );
    let value = json_of(&stale);
    assert_eq!(value["skill"]["status"], serde_json::json!("stale"));
    assert_ne!(
        value["skill"]["embedded_sha256"], value["skill"]["file_sha256"],
        "a stale copy must show both digests"
    );

    let missing = sandbox.run(&["skill", "check", "--path", "repo/nope/SKILL.md"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        stderr_of(&missing).contains("`kaseifu skill install --to"),
        "{}",
        stderr_of(&missing)
    );
}

#[test]
fn v109_scan_visits_repository_copies_in_lexical_order() {
    let sandbox = Sandbox::new("scan");
    let work = sandbox.work();
    // a and b hold a copy, c does not (a scan never introduces one).
    std::fs::create_dir_all(work.join("c/.github/skills/kaseifu")).expect("c");
    let a = write_copy(&sandbox, "a", &canonical_skill());
    write_copy(&sandbox, "b", b"edited by hand\n");
    // A copy below a .git directory is not a repository copy.
    write_copy(&sandbox, "g/.git/hidden", b"hidden\n");
    // A symbolic link is never followed, even at the right path.
    let linked = work.join("s/.github/skills/kaseifu");
    std::fs::create_dir_all(&linked).expect("s");
    std::os::unix::fs::symlink(&a, linked.join("SKILL.md")).expect("symlink");
    // Depth 6 is reached; depth 7 is past the limit.
    write_copy(&sandbox, "d1/d2/d3/d4/d5/d6", b"deep\n");
    write_copy(&sandbox, "d1/d2/d3/d4/d5/d6/d7", b"too deep\n");

    let preview = sandbox.run(&["skill", "install", "--scan", ".", "--format", "json"]);
    assert!(preview.status.success(), "{}", stderr_of(&preview));
    let value = json_of(&preview);
    let targets: Vec<&str> = value["skill"]["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .map(|target| target["target"].as_str().expect("target"))
        .collect();
    assert_eq!(targets.len(), 3, "only repository copies: {targets:?}");
    for (found, wanted) in targets.iter().zip([
        "a/.github/skills/kaseifu/SKILL.md",
        "b/.github/skills/kaseifu/SKILL.md",
        "d1/d2/d3/d4/d5/d6/.github/skills/kaseifu/SKILL.md",
    ]) {
        assert!(found.ends_with(wanted), "{found} must end with {wanted}");
    }
    assert_eq!(
        value["skill"]["targets"][0]["status"],
        serde_json::json!("up_to_date")
    );
    assert_eq!(
        value["skill"]["targets"][1]["status"],
        serde_json::json!("preview")
    );

    let refused = sandbox.run(&["skill", "install", "--scan", ".", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{}", stdout_of(&refused));
    assert_eq!(
        std::fs::read(work.join("b/.github/skills/kaseifu/SKILL.md")).expect("b"),
        b"edited by hand\n".to_vec(),
        "a scan without --force changes no copy"
    );

    let forced = sandbox.run(&["skill", "install", "--scan", ".", "--write", "--force"]);
    assert!(forced.status.success(), "{}", stderr_of(&forced));
    assert_eq!(
        std::fs::read(work.join("b/.github/skills/kaseifu/SKILL.md")).expect("b"),
        canonical_skill()
    );
    assert_eq!(
        std::fs::read(work.join("d1/d2/d3/d4/d5/d6/.github/skills/kaseifu/SKILL.md"))
            .expect("deep"),
        canonical_skill()
    );
}

#[test]
fn v109_scan_of_a_tree_without_copies_is_not_an_error() {
    let sandbox = Sandbox::new("empty");
    std::fs::create_dir_all(sandbox.work().join("plain/repo")).expect("tree");

    let output = sandbox.run(&["skill", "install", "--scan", ".", "--format", "json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(
        json_of(&output)["skill"]["targets"]
            .as_array()
            .expect("targets")
            .len(),
        0
    );

    let text = sandbox.run(&["skill", "install", "--scan", "."]);
    assert!(text.status.success(), "{}", stderr_of(&text));
    assert!(
        stdout_of(&text).contains("no repository at or below"),
        "{}",
        stdout_of(&text)
    );
}

#[test]
fn v109_missing_scan_directory_is_reported() {
    let sandbox = Sandbox::new("missing-scan");
    let output = sandbox.run(&["skill", "install", "--scan", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr_of(&output).contains("is not a directory"),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn v109_usage_errors_exit_two() {
    let sandbox = Sandbox::new("usage");
    for args in [
        vec!["skill", "install"],
        vec!["skill", "install", "--to", ".", "--scan", "."],
        vec!["skill", "install", "--to", ".", "--force"],
        vec!["skill", "check"],
    ] {
        let output = sandbox.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} must be a command line error: {}",
            stderr_of(&output)
        );
        assert!(
            stdout_of(&output).is_empty(),
            "{args:?} must not print a report"
        );
    }
}

#[test]
fn v109_help_names_the_two_verbs_and_stays_ascii() {
    let sandbox = Sandbox::new("help");
    let output = sandbox.run(&["skill", "--help"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.is_ascii(), "{text}");
    assert!(text.contains("install") && text.contains("check"), "{text}");
    assert!(text.contains(".github/skills/kaseifu/SKILL.md"), "{text}");
}

#[test]
fn v109_skill_writes_nothing_outside_the_file_it_names() {
    let sandbox = Sandbox::new("quiet");
    let installed = write_copy(&sandbox, "repo", &canonical_skill());

    // `skill check` is the CI verb: the completion upkeep stays off, so the run
    // writes nothing (the check itself proves the harness below is live).
    let checked = sandbox.run_with_completion_dir(&[
        "skill",
        "check",
        "--path",
        installed.to_str().expect("path"),
    ]);
    assert!(checked.status.success(), "{}", stderr_of(&checked));
    assert!(
        !sandbox.completion_script().exists(),
        "skill check must not write the completion script"
    );

    // Any other command does refresh it, which is what makes the line above a
    // statement about `skill` rather than about the sandbox.
    let ordinary = sandbox.run_with_completion_dir(&["config", "show"]);
    assert!(ordinary.status.success(), "{}", stderr_of(&ordinary));
    assert!(
        sandbox.completion_script().is_file(),
        "an ordinary command refreshes the completion script"
    );
    std::fs::remove_file(sandbox.completion_script()).expect("remove the script");

    // An install writes the named copy and nothing else.
    let installed_now =
        sandbox.run_with_completion_dir(&["skill", "install", "--to", "repo", "--write"]);
    assert!(
        installed_now.status.success(),
        "{}",
        stderr_of(&installed_now)
    );
    assert!(
        !sandbox.completion_script().exists(),
        "skill install must not write the completion script"
    );
    assert!(
        !sandbox
            .home()
            .join("Library/Application Support/kaseifu/kaseifu.db")
            .exists(),
        "the skill command must not create the journal"
    );
}

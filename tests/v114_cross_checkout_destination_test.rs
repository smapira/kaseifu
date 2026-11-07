//! v114 - a destination in another working copy of the same repository
//! (issue #17).
//!
//! Discovery is machine-wide, so a candidate destination can belong to a
//! different checkout of the repository the source lives in (measured
//! 2026-10-19: organizing from `.delta/worktrees/<id>/...` selected the primary
//! checkout, because Spotlight never returns the hidden worktree path itself).
//! An automatic move there is now REVIEW; `--to` and `--select` stay allowed
//! because a human named the path, and they carry one warning line.
//!
//! Every fixture writes the `.git` entry by hand (a directory, or the
//! `gitdir: <path>` file form plus a `commondir` file): git itself is never
//! spawned, so the contract is deterministic and independent of a git binary.
//! Sandboxes live below `target/` (a macOS temporary directory is excluded as a
//! destination shape, so a plan could not be built there) and every command run
//! gets an isolated HOME, no API key and stdin closed. The classifier path uses
//! a fake `mdfind` plus a local mock Jev server, the same way the zero-config
//! CLI tests do; nothing on the machine that runs the tests is touched.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::JevDecision;
use kaseifu::operations::checkout;
use kaseifu::operations::mover;
use kaseifu::operations::plan::{
    PlanError, build_manual_move_plan, build_move_plan_with, build_rule_move_plan,
};
use kaseifu::policy::decision::Thresholds;
use kaseifu::presentation::renderers::organize as organize_view;
use kaseifu::presentation::renderers::select as select_view;
use kaseifu::storage::history::OperationHistory;

/// The source file name shared by the pipeline fixtures.
const TARGET_NAME: &str = "vacation-note-2026.txt";

/// A key that never leaves this process: the endpoint is the local mock Jev
/// server, so no real credential and no outside network is involved.
const DUMMY_KEY: &str = "v114-local-mock-not-a-secret";

/// Every sandbox is its own directory: the test binary runs its tests in
/// parallel and each sandbox is removed when its test ends.
static RUN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let unique = RUN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "v114-cross-checkout-{name}-{}-{unique}",
                std::process::id()
            ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).expect("sandbox");
        std::fs::create_dir_all(dir.join("bin")).expect("fake bin");
        Self { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        std::fs::create_dir_all(&path).expect("sandbox directory");
        path
    }

    fn file(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sandbox parent");
        }
        std::fs::write(&path, body).expect("sandbox file");
        path
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    /// The journal a run that opened the history would create here.
    fn journal(&self) -> PathBuf {
        self.home()
            .join("Library/Application Support/kaseifu/kaseifu.db")
    }

    /// One executable `mdfind` stand-in that prints the given hits.
    fn write_mdfind(&self, hits: &[PathBuf]) {
        use std::os::unix::fs::PermissionsExt;
        let mut script = String::from("#!/bin/sh\n");
        for hit in hits {
            script.push_str(&format!(
                "printf '%s\\n' '{}'\n",
                hit.display().to_string().replace('\'', "'\\''")
            ));
        }
        let path = self.path("bin/mdfind");
        std::fs::write(&path, script).expect("fake mdfind");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fake mdfind executable");
    }

    /// Run kaseifu with an isolated HOME, no API key and stdin closed.
    ///
    /// `endpoint` is the local mock Jev server; only then does the run get a
    /// placeholder key (the classifier path resolves a key before it talks to
    /// the endpoint). Every other run stays key-less.
    fn run(&self, current_dir: &Path, args: &[&str], endpoint: Option<&str>) -> Output {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
        command
            .args(args)
            .current_dir(current_dir)
            .env("HOME", self.home())
            .env("PATH", path)
            .env("KASEIFU_KEY_SOURCE", "env")
            .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
            .env("NO_COLOR", "1")
            .env_remove("JEV_API_KEY")
            .env_remove("JEV_API_BASE_URL")
            .env_remove("JEV_MODEL")
            .env_remove("KASEIFU_DEBUG")
            .env_remove("KASEIFU_COMPLETION_DIR");
        match endpoint {
            Some(endpoint) => command
                .env("JEV_API_URL", endpoint)
                .env("JEV_API_KEY", DUMMY_KEY),
            None => command.env_remove("JEV_API_URL"),
        };
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("kaseifu spawns");
        let deadline = Instant::now() + Duration::from_secs(30);
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
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_str(stdout_of(output).trim()).unwrap_or_else(|error| {
        panic!(
            "stdout must be one JSON document ({error}): {}\nstderr: {}",
            stdout_of(output),
            stderr_of(output)
        )
    })
}

// ---------------------------------------------------------------------------
// Fixtures: a primary checkout plus a linked worktree of the same repository
// ---------------------------------------------------------------------------

/// One repository seen twice: `primary/` (a plain `.git` directory) and
/// `worktree/` (a `.git` file pointing at `primary/.git/worktrees/wt`, which
/// carries `commondir` = `../..`).
struct Worktree {
    sandbox: Sandbox,
    primary: PathBuf,
    worktree: PathBuf,
}

impl Worktree {
    fn new(name: &str) -> Self {
        let sandbox = Sandbox::new(name);
        let primary = sandbox.dir("primary");
        sandbox.dir("primary/.git/worktrees/wt");
        sandbox.file("primary/.git/worktrees/wt/commondir", "../..\n");
        let worktree = sandbox.dir("worktree");
        sandbox.file(
            "worktree/.git",
            &format!("gitdir: {}\n", primary.join(".git/worktrees/wt").display()),
        );
        sandbox.dir("primary/notes");
        sandbox.dir("worktree/inbox");
        Self {
            sandbox,
            primary,
            worktree,
        }
    }

    fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// The file being organized, inside the worktree.
    fn source(&self) -> PathBuf {
        self.sandbox.file(
            "worktree/inbox/vacation-note-2026.txt",
            "v114 sandbox file\n",
        )
    }

    /// Evidence that makes `primary/notes` the one discovered candidate.
    fn evidence(&self) -> Vec<PathBuf> {
        vec![
            self.sandbox
                .file("primary/notes/vacation-note-copy.txt", "a"),
            self.sandbox
                .file("primary/notes/vacation-note-draft.txt", "b"),
        ]
    }

    /// `primary/notes`: the other checkout's folder, the destination in the
    /// reported scenario.
    fn primary_notes(&self) -> PathBuf {
        self.primary.join("notes")
    }
}

fn candidate(id: &str, directory: &Path) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: directory.to_path_buf(),
        score: 1.0,
        evidence_count: 1,
        reasons: Vec::new(),
    }
}

/// A decisive auto-move selection for `D1`.
fn auto_move_to_d1() -> JevDecision {
    JevDecision {
        destination_id: Some("D1".to_string()),
        confidence: 0.950,
        margin: 0.900,
        ..JevDecision::default()
    }
}

// ---------------------------------------------------------------------------
// (a) the same working copy keeps moving
// ---------------------------------------------------------------------------

#[test]
fn v114_a_destination_in_the_same_working_copy_still_moves() {
    let fixture = Worktree::new("same-working-copy");
    let source = fixture.source();
    let destination = fixture.sandbox.dir("worktree/notes");

    assert_eq!(
        checkout::other_working_copy(&source, &destination),
        None,
        "one working copy is not another one"
    );
    let plan = build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &destination)],
        Thresholds::default(),
    )
    .expect("a move inside the worktree is planned");
    assert_eq!(plan.destination_path, destination.join(TARGET_NAME));
}

// ---------------------------------------------------------------------------
// (b) the primary checkout of a worktree is another working copy
// ---------------------------------------------------------------------------

#[test]
fn v114_the_primary_checkout_of_a_worktree_is_another_working_copy() {
    let fixture = Worktree::new("primary-is-other");
    let source = fixture.source();
    let notes = fixture.primary_notes();

    let roots = checkout::other_working_copy(&source, &notes)
        .expect("the primary checkout is another working copy");
    assert_eq!(
        roots.root,
        std::fs::canonicalize(&fixture.primary).expect("canonical"),
        "the warning names the checkout the file would land in"
    );
    assert_eq!(
        roots.common_dir,
        std::fs::canonicalize(fixture.primary.join(".git")).expect("canonical"),
        "both checkouts share one common directory"
    );

    let error = build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &notes)],
        Thresholds::default(),
    )
    .expect_err("an automatic move into another checkout is reviewed");
    assert_eq!(
        error,
        PlanError::DestinationInAnotherCheckout(notes.clone())
    );
    assert!(
        organize_view::plan_error_message(&error, false, false)
            .contains("another working copy of the same repository"),
        "the sentence names the reason"
    );
    assert!(source.exists(), "a refused plan moves nothing");

    // The other direction is the same finding: a file from the primary moved
    // into the worktree also crosses a working-copy boundary.
    let primary_source = fixture
        .sandbox
        .file("primary/inbox/vacation-note-2026.txt", "primary copy\n");
    let worktree_notes = fixture.sandbox.dir("worktree/notes");
    let error = build_move_plan_with(
        &primary_source,
        &auto_move_to_d1(),
        &[candidate("D1", &worktree_notes)],
        Thresholds::default(),
    )
    .expect_err("the worktree is another working copy too");
    assert!(matches!(error, PlanError::DestinationInAnotherCheckout(_)));
}

// ---------------------------------------------------------------------------
// (c) another clone and (d) a submodule are different repositories
// ---------------------------------------------------------------------------

#[test]
fn v114_another_clone_is_a_different_repository() {
    let fixture = Worktree::new("another-clone");
    let source = fixture.source();
    fixture.sandbox.dir("clone/notes");
    fixture.sandbox.dir("clone/.git");
    let destination = fixture.sandbox.dir("clone/notes");

    assert_eq!(checkout::other_working_copy(&source, &destination), None);
    let plan = build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &destination)],
        Thresholds::default(),
    )
    .expect("a separate clone is a different repository, so the move is planned");
    assert_eq!(plan.destination_path, destination.join(TARGET_NAME));
}

#[test]
fn v114_a_submodule_is_a_different_repository() {
    let fixture = Worktree::new("submodule");
    let source = fixture.source();
    let sandbox = fixture.sandbox();
    // The submodule form: a `.git` file with a `gitdir:` line and no
    // `commondir`, so the git directory is the module's own repository.
    let git_dir = sandbox.dir("super/.git/modules/lib");
    sandbox.file(
        "super/lib/.git",
        &format!("gitdir: {}\n", git_dir.display()),
    );
    let destination = sandbox.dir("super/lib/notes");

    assert_eq!(checkout::other_working_copy(&source, &destination), None);
    build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &destination)],
        Thresholds::default(),
    )
    .expect("a submodule is its own repository, so the move is planned");
}

// ---------------------------------------------------------------------------
// (e) a destination without a working copy of its own changes nothing
// ---------------------------------------------------------------------------

#[test]
fn v114_a_destination_without_its_own_checkout_is_allowed() {
    let fixture = Worktree::new("plain-destination");
    let source = fixture.source();
    // No `.git` in this tree: the nearest entry above is the repository that
    // holds the test's own worktree (`target/` lives inside it), which is not
    // the source's repository - the same answer as a path outside git.
    let destination = fixture.sandbox.dir("plain/notes");

    assert_eq!(checkout::other_working_copy(&source, &destination), None);
    build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &destination)],
        Thresholds::default(),
    )
    .expect("a destination outside the source's repository is planned");

    // The module's own unit tests cover the literal "outside git" answer
    // (`git_roots` returns `None`); here the two paths simply never share a
    // common directory.
    assert_ne!(
        checkout::git_common_dir(&source),
        checkout::git_common_dir(&destination)
    );
}

// ---------------------------------------------------------------------------
// (f) the cross-checkout reason wins over the collision
// ---------------------------------------------------------------------------

#[test]
fn v114_a_collision_does_not_hide_the_cross_checkout_reason() {
    let fixture = Worktree::new("collision-priority");
    let source = fixture.source();
    let notes = fixture.primary_notes();
    // A file of the same name already sits in the other checkout.
    let blocker = fixture
        .sandbox
        .file("primary/notes/vacation-note-2026.txt", "x");

    let error = build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &notes)],
        Thresholds::default(),
    )
    .expect_err("the deeper reason is reported");
    assert_eq!(
        error,
        PlanError::DestinationInAnotherCheckout(notes.clone()),
        "the checkout is named, not the colliding file"
    );
    assert!(blocker.exists());

    // Inside one working copy a collision is still a collision.
    let same_checkout = fixture.sandbox.dir("worktree/notes");
    let blocker = fixture
        .sandbox
        .file("worktree/notes/vacation-note-2026.txt", "y");
    let error = build_move_plan_with(
        &source,
        &auto_move_to_d1(),
        &[candidate("D1", &same_checkout)],
        Thresholds::default(),
    )
    .expect_err("a collision inside the checkout");
    assert_eq!(error, PlanError::Collision(blocker));
}

// ---------------------------------------------------------------------------
// (g) --to / --select: allowed, with one warning line
// ---------------------------------------------------------------------------

#[test]
fn v114_a_manual_plan_allows_another_checkout() {
    let fixture = Worktree::new("manual-allowed");
    let source = fixture.source();
    let notes = fixture.primary_notes();

    let plan = build_manual_move_plan(&source, &candidate("D1", &notes))
        .expect("a human named the path, so the plan is allowed");
    assert_eq!(plan.destination_path, notes.join(TARGET_NAME));
}

#[test]
fn v114_to_warns_and_still_moves() {
    let fixture = Worktree::new("to-warning");
    let source = fixture.source();
    let notes = fixture.primary_notes();
    let sandbox = fixture.sandbox();
    let source_arg = source.to_str().expect("utf-8");
    let notes_arg = notes.to_str().expect("utf-8");

    // Text: exit 0, one warning line, the file stays (a dry run).
    let output = sandbox.run(
        &fixture.worktree,
        &["organize", source_arg, "--to", notes_arg],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("another working copy"), "got:\n{text}");
    assert!(text.contains("repository"), "got:\n{text}");
    assert!(text.contains("undo reverses"), "got:\n{text}");
    assert!(text.is_ascii(), "output stays ASCII: {text}");
    assert!(source.exists(), "a dry run moves nothing");

    // JSON: the additive `checkout` object, with every historical key intact.
    let output = sandbox.run(
        &fixture.worktree,
        &[
            "organize", source_arg, "--to", notes_arg, "--format", "json",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let value = json_of(&output);
    let document = &value["organize_to"];
    assert_eq!(document["executed"], serde_json::json!(false));
    assert_eq!(document["file"], serde_json::json!(source_arg));
    assert_eq!(document["destination"], serde_json::json!(notes_arg));
    assert_eq!(
        document["checkout"]["another_working_copy"],
        serde_json::json!(true)
    );
    assert_eq!(
        document["checkout"]["root"],
        serde_json::json!(
            std::fs::canonicalize(&fixture.primary)
                .expect("canonical")
                .display()
                .to_string()
        )
    );
    assert!(
        document["checkout"]["notice"]
            .as_str()
            .expect("notice text")
            .contains("another working copy"),
        "{value}"
    );

    // The same move inside one working copy carries no warning at all.
    fixture.sandbox.dir("worktree/notes");
    let inside_arg = fixture
        .worktree
        .join("notes")
        .to_str()
        .expect("utf-8")
        .to_string();
    let output = sandbox.run(
        &fixture.worktree,
        &[
            "organize",
            source_arg,
            "--to",
            &inside_arg,
            "--format",
            "json",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let value = json_of(&output);
    assert!(
        value["organize_to"].get("checkout").is_none(),
        "a run without the case keeps its exact key set: {value}"
    );
}

#[test]
fn v114_the_select_plan_shows_the_same_notice() {
    let fixture = Worktree::new("select-notice");
    let source = fixture.source();
    let notes = fixture.primary_notes();
    let plan = build_manual_move_plan(&source, &candidate("D1", &notes)).expect("manual plan");
    let roots = checkout::other_working_copy(&source, &notes).expect("another working copy");

    let warned = select_view::render_plan(
        &plan,
        &candidate("D1", &notes),
        false,
        false,
        false,
        Some(&roots.root),
    );
    assert!(warned.contains("another working copy"), "got:\n{warned}");
    assert!(warned.is_ascii(), "got:\n{warned}");

    let plain =
        select_view::render_plan(&plan, &candidate("D1", &notes), false, false, false, None);
    assert!(
        !plain.contains("another working copy"),
        "a plan without the case is unchanged:\n{plain}"
    );
}

// ---------------------------------------------------------------------------
// (b) end to end: REVIEW, exit 0; (h) --execute moves nothing
// ---------------------------------------------------------------------------

/// The mock Jev endpoint: answers every request with a decisive `D1`.
struct MockJev {
    endpoint: String,
    _handle: JoinHandle<()>,
}

impl MockJev {
    fn start() -> Self {
        let body = concat!(
            r#"{"model":"jev-latest","answers":{"destination":{"type":"choice","#,
            r#""choice":"D1","probabilities":{"D1":0.95,"NONE":0.05}}}}"#
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let _ = read_http_request(&mut stream);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self {
            endpoint: format!("http://{address}"),
            _handle: handle,
        }
    }
}

/// Read one request off the connection (the body is not needed).
fn read_http_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        if let Some(header_end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..header_end]).into_owned();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if data.len() >= header_end + 4 + content_length {
                break;
            }
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

#[test]
fn v114_organize_reviews_a_cross_checkout_destination() {
    let fixture = Worktree::new("organize-review");
    let source = fixture.source();
    let hits = fixture.evidence();
    let sandbox = fixture.sandbox();
    sandbox.write_mdfind(&hits);
    let server = MockJev::start();
    let source_arg = source.to_str().expect("utf-8");

    let output = sandbox.run(
        &fixture.worktree,
        &["organize", source_arg, "--format", "json"],
        Some(&server.endpoint),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "a REVIEW keeps the existing exit code 0: {}\n{}",
        stdout_of(&output),
        stderr_of(&output)
    );
    let value = json_of(&output);
    assert_eq!(value["mode"], serde_json::json!("dry_run"));
    assert_eq!(value["decision"]["destination_id"], serde_json::json!("D1"));
    assert_eq!(value["outcome"]["type"], serde_json::json!("unplanable"));
    assert_eq!(
        value["outcome"]["kind"],
        serde_json::json!("destination_in_another_checkout")
    );
    assert!(
        value["outcome"]["reason"]
            .as_str()
            .expect("reason")
            .contains("another working copy of the same repository"),
        "{value}"
    );

    // The text view says REVIEW and prints the same sentence.
    let output = sandbox.run(
        &fixture.worktree,
        &["organize", source_arg],
        Some(&server.endpoint),
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("== REVIEW =="), "got:\n{text}");
    assert!(
        text.contains("another working copy of the same repository"),
        "got:\n{text}"
    );
    assert!(text.is_ascii(), "output stays ASCII: {text}");
    assert!(source.exists(), "a dry run moves nothing");
}

#[test]
fn v114_execute_moves_nothing_when_the_destination_is_another_checkout() {
    let fixture = Worktree::new("execute-review");
    let source = fixture.source();
    let hits = fixture.evidence();
    fixture.sandbox().write_mdfind(&hits);
    let server = MockJev::start();
    let source_arg = source.to_str().expect("utf-8");

    let output = fixture.sandbox().run(
        &fixture.worktree,
        &["organize", source_arg, "--execute", "--format", "json"],
        Some(&server.endpoint),
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let value = json_of(&output);
    assert_eq!(value["mode"], serde_json::json!("execute"));
    assert_eq!(
        value["outcome"]["kind"],
        serde_json::json!("destination_in_another_checkout")
    );
    assert!(
        source.exists(),
        "a REVIEW moves nothing, even with --execute"
    );
    assert!(
        !fixture.primary_notes().join(TARGET_NAME).exists(),
        "the other checkout is untouched"
    );
    assert!(
        std::fs::read_dir(fixture.primary_notes())
            .expect("notes")
            .all(|entry| entry.expect("entry").file_name() != TARGET_NAME),
        "nothing arrived in the other checkout"
    );
}

// ---------------------------------------------------------------------------
// (i) a rule move inside the worktree still moves
// ---------------------------------------------------------------------------

#[test]
fn v114_a_rule_move_inside_the_worktree_still_moves() {
    let fixture = Worktree::new("rule-move");
    let source = fixture.source();
    fixture.sandbox.dir("worktree/filed");
    fixture.sandbox.file(
        "worktree/.kaseifu.toml",
        "[[rule]]\nname = \"file-notes\"\nmatch_glob = \"inbox/**\"\ndestination = \"filed\"\n",
    );
    let source_arg = source.to_str().expect("utf-8");

    let output = fixture.sandbox().run(
        &fixture.worktree,
        &["organize", source_arg, "--format", "json"],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let value = json_of(&output);
    assert_eq!(
        value["rule"]["name"],
        serde_json::json!("file-notes"),
        "the rule decides the file: {value}"
    );
    assert_eq!(
        value["outcome"]["type"],
        serde_json::json!("would_move"),
        "a rule destination inside the workspace is not a cross-checkout case: {value}"
    );

    // The plan layer agrees: the rule builder never runs the checkout check.
    let destination = fixture.sandbox.path("worktree/filed");
    let plan = build_rule_move_plan(&source, &destination, &fixture.worktree, "rule:file-notes")
        .expect("a destination inside the workspace is planned");
    assert_eq!(plan.destination_path, destination.join(TARGET_NAME));

    // And the move really happens with `--execute`, journaled for `undo`.
    let output = fixture.sandbox().run(
        &fixture.worktree,
        &["organize", source_arg, "--execute", "--format", "json"],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let value = json_of(&output);
    assert_eq!(value["outcome"]["type"], serde_json::json!("moved"));
    assert!(!source.exists(), "the rule move happened");
    assert!(destination.join(TARGET_NAME).exists());
    assert!(
        fixture.sandbox().journal().exists(),
        "the journal lives in the sandbox HOME"
    );
}

// ---------------------------------------------------------------------------
// The report surface: the REVIEW heading and the machine word
// ---------------------------------------------------------------------------

#[test]
fn v114_the_checkout_review_renders_as_review() {
    let fixture = Worktree::new("render-review");
    let source = fixture.source();
    let notes = fixture.primary_notes();
    let error = PlanError::DestinationInAnotherCheckout(notes.clone());
    let report = kaseifu::operations::organize::OrganizeReport {
        decision: auto_move_to_d1(),
        policy: kaseifu::models::decision::DecisionAction::AutoMove("D1".to_string()),
        candidate_path: Some(notes.clone()),
        // An AUTO MOVE verdict never renders the LEAVE reason rows, so the
        // list stays empty in this hand-built report.
        candidates: Vec::new(),
        target_indexed: None,
        outcome: kaseifu::operations::organize::OrganizeOutcome::Unplanable(error),
        rule: None,
        keywords: kaseifu::classifier::keywords::KeywordDelivery::default(),
    };

    let text = organize_view::render_text(&report, &source, false);
    assert!(text.contains("== REVIEW =="), "got:\n{text}");
    assert!(
        text.contains("another working copy of the same repository"),
        "got:\n{text}"
    );
    let json = organize_view::json_report(&report, &source, false, None);
    assert_eq!(
        json["outcome"]["kind"],
        serde_json::json!("destination_in_another_checkout")
    );

    // The manual move is journaled for `undo`, which is why the warning can
    // say the move is reversible.
    let plan = build_manual_move_plan(&source, &candidate("D1", &notes)).expect("manual plan");
    let history = OperationHistory::open_in_memory().expect("journal");
    mover::execute_manual_move(&plan, &history).expect("manual move");
    let undone = kaseifu::operations::undo::undo_last_move(&history).expect("undo");
    assert_eq!(undone.restored_to, source);
    assert!(source.exists(), "undo brings the file back");
}

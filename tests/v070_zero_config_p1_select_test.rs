//! v0.7.0 P1 ZC-13 tests: interactive destination selection.
//!
//! The TTY path runs the real binary on a real PTY (`libc::openpty`), with
//! input injected as terminal keystrokes; the non-TTY paths run it with
//! stdin closed. Discovery's `mdfind` is injected through PATH and candidate
//! folders live under `CARGO_TARGET_TMPDIR` (a temp-dir path is a blocked
//! destination by design). HOME is isolated, no API key is set, and
//! `JEV_API_URL` points at a closed port: the whole flow must work without
//! Jev, without the Keychain and without the network.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use kaseifu::models::candidate::CandidateDestination;
use kaseifu::operations::mover::{MoveOutcome, execute_manual_move};
use kaseifu::operations::plan::{PlanError, build_manual_move_plan};
use kaseifu::operations::undo::undo_last_move;
use kaseifu::storage::history::OperationHistory;

const TARGET_NAME: &str = "vacation-note-2026.txt";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-p1-select-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn fixture_root(name: &str) -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    dir.push(format!("p1-select-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture root");
    dir
}

struct Fixture {
    home: PathBuf,
    inbox: PathBuf,
    target: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
    fake_bin: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = fixture_root(name);
        let home = temp_dir(&format!("home-{name}"));
        let inbox = dir.join("inbox");
        let alpha = dir.join("dest-alpha");
        let beta = dir.join("dest-beta");
        let fake_bin = temp_dir(&format!("bin-{name}"));
        for folder in [&inbox, &alpha, &beta] {
            std::fs::create_dir_all(folder).expect("fixture folder");
        }
        let target = inbox.join(TARGET_NAME);
        std::fs::write(&target, "hello from the p1 select test").expect("target file");
        std::fs::write(alpha.join("vacation-note-copy.txt"), "a").expect("alpha evidence");
        std::fs::write(beta.join("vacation-note-draft.txt"), "b").expect("beta evidence");
        let fixture = Self {
            home,
            inbox,
            target,
            alpha,
            beta,
            fake_bin,
        };
        fixture.write_fake_mdfind(&[
            // Blocked destinations are filtered before the filesystem is
            // touched: neither path needs to exist.
            "/Users/nobody/Library/Application Support/SeedStore/vacation-note.txt".to_string(),
            "/var/folders/xx/yy/T/vacation-note.txt".to_string(),
            fixture
                .alpha
                .join("vacation-note-copy.txt")
                .display()
                .to_string(),
            fixture
                .beta
                .join("vacation-note-draft.txt")
                .display()
                .to_string(),
        ]);
        fixture
    }

    fn write_fake_mdfind(&self, hits: &[String]) {
        let mut script = String::from("#!/bin/sh\n");
        for hit in hits {
            script.push_str(&format!(
                "printf '%s\\n' '{}'\n",
                hit.replace('\'', "'\\''")
            ));
        }
        let path = self.fake_bin.join("mdfind");
        std::fs::write(&path, script).expect("fake mdfind");
        std::fs::set_permissions(
            &path,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
        )
        .expect("fake mdfind executable");
    }

    fn journal_path(&self) -> PathBuf {
        self.home
            .join("Library/Application Support/kaseifu/kaseifu.db")
    }

    fn candidate(&self, id: &str, path: &Path) -> CandidateDestination {
        CandidateDestination {
            id: id.to_string(),
            path: path.to_path_buf(),
            score: 12.5,
            evidence_count: 1,
            reasons: vec!["found 1 items".to_string()],
        }
    }
}

// ---------------------------------------------------------------------------
// Running the real binary (non-PTY)
// ---------------------------------------------------------------------------

fn run(fixture: &Fixture, args: &[&str], key: Option<&str>, timeout: Duration) -> Output {
    let mut command = base_command(fixture, args);
    match key {
        Some(key) => command.env("JEV_API_KEY", key),
        None => command.env_remove("JEV_API_KEY"),
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kaseifu spawns");
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect output");
                panic!(
                    "kaseifu exceeded {timeout:?}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

/// Child environment: isolated HOME, fake `mdfind` first on PATH, no key,
/// and an unreachable Jev endpoint (a network attempt would fail the run).
fn base_command(fixture: &Fixture, args: &[&str]) -> ProcessCommand {
    let path = format!(
        "{}:{}",
        fixture.fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .args(args)
        .current_dir(&fixture.inbox)
        .env("HOME", &fixture.home)
        .env("PATH", path)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env("JEV_API_URL", "http://127.0.0.1:9")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR");
    command
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// Non-TTY paths
// ---------------------------------------------------------------------------

#[test]
fn select_non_tty_fails_with_guidance_and_touches_nothing() {
    let fixture = Fixture::new("non-tty");
    let output = run(
        &fixture,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--select",
        ],
        None,
        Duration::from_secs(20),
    );

    assert!(!output.status.success(), "must not start interacting");
    let text = format!("{}{}", stdout_of(&output), stderr_of(&output));
    assert!(
        text.contains("interactive selection needs a terminal (stdin is not a TTY)"),
        "{text}"
    );
    assert!(text.contains("--select --format json"), "{text}");
    assert!(text.contains("--to <dir>"), "{text}");
    assert!(!text.contains("discover"), "{text}");
    assert!(fixture.target.exists(), "nothing moved");
    assert!(!fixture.journal_path().exists(), "no journal was written");
    assert!(!fixture.inbox.join(".env").exists(), "no .env was written");
}

#[test]
fn select_json_lists_candidates_without_interaction_or_key() {
    let fixture = Fixture::new("json");
    let output = run(
        &fixture,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--select",
            "--format",
            "json",
        ],
        None,
        Duration::from_secs(30),
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let candidates: Vec<serde_json::Value> =
        serde_json::from_str(stdout_of(&output).trim()).expect("candidate list JSON");
    assert!(!candidates.is_empty(), "fixture candidates: {candidates:?}");
    let paths: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate["path"].as_str().expect("path").to_string())
        .collect();
    for expected in [&fixture.alpha, &fixture.beta] {
        assert!(
            paths.iter().any(|path| Path::new(path) == expected),
            "missing {expected:?}: {paths:?}"
        );
    }
    for candidate in &candidates {
        assert!(candidate["id"].is_string(), "{candidate}");
        assert!(candidate["score"].is_f64(), "{candidate}");
        assert!(candidate["evidence_count"].is_u64(), "{candidate}");
    }
    assert!(
        paths.iter().all(|path| Path::new(path) != fixture.inbox),
        "the file's own parent is never a choice (no-op): {paths:?}"
    );
    let text = format!("{}{}", stdout_of(&output), stderr_of(&output));
    assert!(!text.contains("/Library/"), "blocked dirs stay out: {text}");
    assert!(
        !text.contains("/var/folders"),
        "blocked dirs stay out: {text}"
    );
    assert!(!text.contains("Jev API key"), "no key prompt: {text}");
    assert!(fixture.target.exists(), "listing moves nothing");
    assert!(!fixture.journal_path().exists(), "no journal was written");
}

#[test]
fn select_json_with_execute_is_rejected_instead_of_silently_ignored() {
    let fixture = Fixture::new("json-execute");
    let output = run(
        &fixture,
        &[
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--select",
            "--format",
            "json",
            "--execute",
        ],
        None,
        Duration::from_secs(20),
    );

    assert!(!output.status.success());
    let text = stderr_of(&output);
    assert!(
        text.contains("does not select") && text.contains("run without --format json"),
        "{text}"
    );
    assert!(fixture.target.exists());
}

// ---------------------------------------------------------------------------
// Plan layer (library): exclusions, collisions, journal distinction
// ---------------------------------------------------------------------------

#[test]
fn manual_plan_refuses_blocked_destinations() {
    let fixture = Fixture::new("plan-refuse");
    for blocked in [
        "/Users/someone/Library/Application Support/SeedStore",
        "/Users/someone/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info",
        "/var/folders/xx/yy/T",
        "/Users/someone/Pictures/20260925T175534",
    ] {
        let candidate = fixture.candidate("D1", Path::new(blocked));
        let error = build_manual_move_plan(&fixture.target, &candidate).expect_err(blocked);
        assert!(
            matches!(error, PlanError::DestinationExcluded(_)),
            "{blocked}: {error:?}"
        );
    }
    assert!(fixture.target.exists(), "a refusal moves nothing");
}

#[test]
fn manual_plan_refuses_collisions_and_missing_sources() {
    let fixture = Fixture::new("plan-collision");
    std::fs::write(fixture.alpha.join(TARGET_NAME), "already here").expect("collision fixture");
    let candidate = fixture.candidate("D1", &fixture.alpha);
    let error = build_manual_move_plan(&fixture.target, &candidate).expect_err("collision");
    assert!(matches!(error, PlanError::Collision(_)), "{error:?}");

    let missing = fixture.inbox.join("not-there.txt");
    let candidate = fixture.candidate("D1", &fixture.alpha);
    let error = build_manual_move_plan(&missing, &candidate).expect_err("missing source");
    assert!(matches!(error, PlanError::SourceMissing(_)), "{error:?}");
}

#[test]
fn manual_move_journals_null_selection_and_undo_restores() {
    let fixture = Fixture::new("manual-journal");
    let candidate = fixture.candidate("D2", &fixture.alpha);
    let plan = build_manual_move_plan(&fixture.target, &candidate).expect("manual plan");
    assert_eq!(plan.confidence, 0.0);
    assert_eq!(plan.margin, 0.0);

    let history = OperationHistory::open_in_memory().expect("journal");
    let MoveOutcome::Moved { operation_id } =
        execute_manual_move(&plan, &history).expect("manual move")
    else {
        panic!("execute_manual_move must move");
    };

    let entries = history.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    let row = &entries[0];
    assert_eq!(row.id, operation_id);
    assert_eq!(row.operation, "move");
    assert_eq!(row.result, "success");
    assert_eq!(row.candidate_id, None, "a human move has no Jev selection");
    assert_eq!(row.confidence, None);
    assert_eq!(row.margin, None);
    assert!(fixture.alpha.join(TARGET_NAME).exists());
    assert!(!fixture.target.exists());

    let outcome = undo_last_move(&history).expect("undo the manual move");
    assert_eq!(outcome.restored_to, fixture.target);
    assert!(fixture.target.exists(), "the file is back");
    assert_eq!(history.entries().expect("entries").len(), 2);
}

// ---------------------------------------------------------------------------
// PTY paths (real terminal, injected keystrokes)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod pty {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// One step of a scripted terminal session.
    pub enum Step<'a> {
        /// Wait until `marker` appears in the collected terminal output.
        WaitFor(&'a [u8]),
        /// Write `input` to the terminal (as if typed).
        Send(&'a [u8]),
        /// Deliver `signal` to the child process.
        Signal(libc::c_int),
    }

    pub struct PtyRun {
        pub status: std::process::ExitStatus,
        pub output: Vec<u8>,
    }

    /// Run `command` on a fresh PTY and execute the scripted steps.
    ///
    /// `libc::openpty` keeps this self-contained (no `script`/`expect`), and
    /// the deadline kills a regression that would hang waiting for input.
    pub fn run_in_pty(
        mut command: ProcessCommand,
        steps: &[Step<'_>],
        timeout: Duration,
    ) -> PtyRun {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let mut size = libc::winsize {
            ws_row: 30,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let opened = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        assert_eq!(opened, 0, "openpty");

        let master_read = unsafe { std::fs::File::from_raw_fd(master) };
        let mut master_write = unsafe { std::fs::File::from_raw_fd(libc::dup(master)) };
        let stdin = unsafe { std::fs::File::from_raw_fd(libc::dup(slave)) };
        let stdout = unsafe { std::fs::File::from_raw_fd(libc::dup(slave)) };
        let stderr = unsafe { std::fs::File::from_raw_fd(libc::dup(slave)) };
        unsafe { libc::close(slave) };

        command
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        let mut child = command.spawn().expect("spawn in pty");
        let pid = child.id();

        let collected = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let sink = Arc::clone(&collected);
        let finished = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut master_read = master_read;
            let mut buffer = [0u8; 4096];
            loop {
                match master_read.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => sink
                        .lock()
                        .expect("lock")
                        .extend_from_slice(&buffer[..read]),
                }
            }
            finished.store(true, Ordering::SeqCst);
        });

        let deadline = Instant::now() + timeout;
        for step in steps {
            match step {
                Step::WaitFor(marker) => loop {
                    let seen = collected
                        .lock()
                        .expect("lock")
                        .windows(marker.len())
                        .any(|window| window == *marker);
                    if seen {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "marker {:?} never appeared: {}",
                        String::from_utf8_lossy(marker),
                        String::from_utf8_lossy(&collected.lock().expect("lock"))
                    );
                    std::thread::sleep(Duration::from_millis(20));
                },
                Step::Send(input) => {
                    master_write.write_all(input).expect("write pty input");
                    master_write.flush().expect("flush pty input");
                }
                Step::Signal(signal) => unsafe {
                    libc::kill(pid as libc::pid_t, *signal);
                },
            }
        }

        let status = loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "pty run exceeded {timeout:?}: {}",
                    String::from_utf8_lossy(&collected.lock().expect("lock"))
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        };

        let settle = Instant::now() + Duration::from_millis(1500);
        while !done.load(Ordering::SeqCst) && Instant::now() < settle {
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = collected.lock().expect("lock").clone();
        PtyRun { status, output }
    }

    fn pty_output(run: &PtyRun) -> String {
        String::from_utf8_lossy(&run.output).into_owned()
    }

    fn select_command(fixture: &Fixture, extra: &[&str]) -> ProcessCommand {
        let mut args = vec![
            "organize",
            fixture.target.to_str().expect("utf-8"),
            "--select",
        ];
        args.extend_from_slice(extra);
        base_command(fixture, &args)
    }

    #[test]
    fn pty_select_execute_moves_journals_and_undo_restores() {
        let fixture = Fixture::new("pty-execute");
        let pty_run = run_in_pty(
            select_command(&fixture, &["--execute", "--yes"]),
            &[Step::WaitFor(b"q) cancel"), Step::Send(b"1\n")],
            Duration::from_secs(30),
        );
        let text = pty_output(&pty_run);

        assert!(pty_run.status.success(), "select flow failed: {text}");
        assert!(
            text.contains("Candidates are discovered destinations"),
            "menu: {text}"
        );
        assert!(text.contains("1) D1"), "numbered list: {text}");
        assert!(text.contains("[MANUAL]"), "manual marker: {text}");
        assert!(text.contains("MOVED"), "result: {text}");
        assert!(text.contains("op-"), "operation id: {text}");
        assert!(
            text.contains("kaseifu undo reverses this move"),
            "undo hint: {text}"
        );
        assert!(
            !text.contains("Jev API key"),
            "no key prompt without a key: {text}"
        );

        // The file moved into the first (highest ranked) candidate folder.
        assert!(!fixture.target.exists(), "the source moved: {text}");
        let landed: Vec<&PathBuf> = [&fixture.alpha, &fixture.beta]
            .into_iter()
            .filter(|dir| dir.join(TARGET_NAME).exists())
            .collect();
        assert_eq!(landed.len(), 1, "moved exactly once: {landed:?}");

        // The journal row marks the move as human: NULL Jev columns.
        let inspection = kaseifu::storage::history::inspect_journal(&fixture.journal_path())
            .expect("read journal")
            .expect("journal exists");
        assert_eq!(inspection.total_operations, 1);
        assert!(inspection.pending.is_empty());
        let entries = OperationHistory::open(fixture.journal_path())
            .expect("open journal")
            .entries()
            .expect("entries");
        assert_eq!(entries[0].candidate_id, None, "{entries:?}");
        assert_eq!(entries[0].confidence, None);
        assert_eq!(entries[0].margin, None);

        // `undo` reverses it and the journal shows both rows.
        let undo = run(&fixture, &["undo"], None, Duration::from_secs(20));
        assert!(undo.status.success(), "stderr: {}", stderr_of(&undo));
        assert!(
            stdout_of(&undo).contains("restored:"),
            "{}",
            stdout_of(&undo)
        );
        assert!(fixture.target.exists(), "undo restored the file");
        let inspection = kaseifu::storage::history::inspect_journal(&fixture.journal_path())
            .expect("read journal")
            .expect("journal exists");
        assert_eq!(inspection.total_operations, 2, "move + undo");

        // `doctor` agrees with the journal: the Journal row is ok and the
        // count matches; nothing is pending.
        let doctor = run(&fixture, &["doctor"], None, Duration::from_secs(20));
        assert!(doctor.status.success(), "stderr: {}", stderr_of(&doctor));
        let doctor_text = stdout_of(&doctor);
        let journal_line = doctor_text
            .lines()
            .find(|line| line.contains("Journal"))
            .expect("journal row");
        assert!(journal_line.starts_with("ok Journal"), "{journal_line}");
        assert!(journal_line.contains("2 operation(s)"), "{journal_line}");
        assert!(journal_line.contains("no pending"), "{journal_line}");
    }

    #[test]
    fn pty_select_execute_confirmation_decline_changes_nothing() {
        let fixture = Fixture::new("pty-decline");
        let pty_run = run_in_pty(
            select_command(&fixture, &["--execute"]),
            &[
                Step::WaitFor(b"q) cancel"),
                Step::Send(b"1\n"),
                Step::WaitFor(b"Move this file?"),
                Step::Send(b"n\n"),
            ],
            Duration::from_secs(30),
        );
        let text = pty_output(&pty_run);

        assert!(pty_run.status.success(), "{text}");
        assert!(text.contains("Move this file? [y/N]"), "{text}");
        assert!(text.contains("cancelled; nothing was changed"), "{text}");
        assert!(fixture.target.exists(), "{text}");
        assert!(!fixture.journal_path().exists(), "no journal was written");
    }

    #[test]
    fn pty_select_quit_and_invalid_input_change_nothing() {
        let fixture = Fixture::new("pty-quit");
        let pty_run = run_in_pty(
            select_command(&fixture, &[]),
            &[
                Step::WaitFor(b"q) cancel"),
                Step::Send(b"9\n"),
                Step::WaitFor(b"expected a number from 1 to"),
                Step::Send(b"q\n"),
            ],
            Duration::from_secs(30),
        );
        let text = pty_output(&pty_run);

        assert!(pty_run.status.success(), "{text}");
        assert!(
            text.contains("expected a number from 1 to"),
            "invalid input is explained: {text}"
        );
        assert!(text.contains("cancelled; nothing was changed"), "{text}");
        assert!(fixture.target.exists(), "{text}");
        assert!(!fixture.journal_path().exists(), "no journal was written");
    }

    #[test]
    fn pty_select_sigint_changes_nothing() {
        let fixture = Fixture::new("pty-sigint");
        let pty_run = run_in_pty(
            select_command(&fixture, &[]),
            &[Step::WaitFor(b"q) cancel"), Step::Signal(libc::SIGINT)],
            Duration::from_secs(30),
        );

        assert!(!pty_run.status.success(), "SIGINT terminates the run");
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            pty_run.status.signal(),
            Some(libc::SIGINT),
            "terminated by SIGINT: {}",
            pty_output(&pty_run)
        );
        assert!(fixture.target.exists(), "nothing moved");
        assert!(!fixture.journal_path().exists(), "no journal was written");
    }
}

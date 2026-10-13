//! v0.5.2 zero-config CLI tests (ZC-1 non-TTY path / ZC-2 / ZC-3 display /
//! ZC-4 hints).
//!
//! Every child runs inside a temporary directory with an isolated HOME and all
//! key-related variables removed, so the developer's configuration, journal,
//! `.env` and Keychain are never involved. Runs are bounded by a deadline: a
//! regression that reintroduces a blocking prompt fails the test instead of
//! freezing the suite. The interactive TTY path is exercised through a real
//! PTY (`libc::openpty`) with the key fed on stdin.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

const DUMMY_KEY: &str = "test-key-not-a-secret-0123456789";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-zero-config-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// Child environment: isolated cwd/HOME, no key variables from the parent.
fn base_command(cwd: &Path, home: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR")
        .env_remove("KASEIFU_NO_COMPLETION_UPDATE");
    command
}

/// Run to completion within `timeout`; a hang is killed and fails the test.
fn run_with_deadline(mut command: ProcessCommand, timeout: Duration) -> Output {
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

fn run(dir: &Path, home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = base_command(dir, home);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    run_with_deadline(command, Duration::from_secs(20))
}

fn home_of(dir: &Path) -> PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    home
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn combined(output: &Output) -> String {
    format!("{}{}", stdout_of(output), stderr_of(output))
}

fn fail_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("FAIL "))
        .collect()
}

// ---------------------------------------------------------------------------
// ZC-2: bare `kaseifu` prints guidance
// ---------------------------------------------------------------------------

#[test]
fn v052_no_args_prints_guidance_without_side_effects() {
    let dir = temp_dir("guidance");
    let home = home_of(&dir);
    let completion = dir.join("completion");
    std::fs::create_dir_all(&completion).expect("completion dir");

    // `KASEIFU_COMPLETION_DIR` is set without the disable switch: the automatic
    // completion upkeep would write there, so an empty directory afterwards
    // proves the no-argument path stays side-effect free.
    let output = run(
        &dir,
        &home,
        &[],
        &[(
            "KASEIFU_COMPLETION_DIR",
            completion.to_str().expect("utf-8"),
        )],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("kaseifu classify <file>"), "{text}");
    assert!(text.contains("kaseifu organize <file>"), "{text}");
    assert!(text.contains("kaseifu doctor"), "{text}");
    assert!(
        text.contains("never moved unless you pass --execute"),
        "{text}"
    );
    assert!(text.contains("run kaseifu setup"), "{text}");
    assert!(text.contains("Run kaseifu --help"), "{text}");
    assert!(
        !text.contains("\u{1b}["),
        "ASCII/plain without a TTY: {text}"
    );

    assert!(!dir.join("data").exists(), "no data/ directory");
    assert!(!home.join("Library/Application Support/kaseifu").exists());
    let completion_files: Vec<_> = std::fs::read_dir(&completion)
        .expect("read completion dir")
        .collect();
    assert!(
        completion_files.is_empty(),
        "no completion update reached the disk"
    );
    assert!(stderr_of(&output).is_empty(), "{}", stderr_of(&output));
}

#[test]
fn v052_no_args_json_is_structured_and_keeps_the_key_hidden() {
    let dir = temp_dir("guidance-json");
    let home = home_of(&dir);
    let output = run(&dir, &home, &["--format", "json"], &[]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let json: serde_json::Value = serde_json::from_str(stdout_of(&output).trim()).expect("JSON");
    assert_eq!(json["execute_required"], true);
    assert_eq!(json["api_key_configured"], false);
    assert_eq!(json["commands"].as_array().expect("commands").len(), 3);
    assert!(!stdout_of(&output).contains("JEV_API_KEY"));
}

#[test]
fn v052_help_still_lists_every_command() {
    let dir = temp_dir("help");
    let home = home_of(&dir);
    for flag in ["--help", "-h"] {
        let output = run(&dir, &home, &[flag], &[]);
        assert!(output.status.success(), "{flag}: {}", stderr_of(&output));
        let text = stdout_of(&output);
        for command in [
            "classify",
            "organize",
            "doctor",
            "inspect",
            "setup",
            "auth",
            "config",
            "completions",
            "watch",
            "undo",
        ] {
            assert!(text.contains(command), "{flag} must list {command}: {text}");
        }
        assert!(
            !text.contains("discover"),
            "hidden command must stay hidden"
        );
    }
}

// ---------------------------------------------------------------------------
// ZC-1: non-TTY failures carry the guidance
// ---------------------------------------------------------------------------

#[test]
fn v052_non_tty_jev_command_fails_with_guidance_and_writes_nothing() {
    let dir = temp_dir("non-tty");
    let home = home_of(&dir);
    let file = dir.join("note.txt");
    std::fs::write(&file, "hello").expect("write file");

    let output = run(
        &dir,
        &home,
        &["classify", file.to_str().expect("utf-8")],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );

    assert!(
        !output.status.success(),
        "a missing key must fail: {}",
        stdout_of(&output)
    );
    let text = combined(&output);
    assert!(text.contains("no API key is configured"), "{text}");
    assert!(text.contains("KASEIFU_KEY_SOURCE=env"), "{text}");
    assert!(text.contains("kaseifu setup"), "{text}");
    assert!(text.contains("kaseifu inspect"), "{text}");
    assert!(text.contains("kaseifu doctor"), "{text}");
    assert!(!text.contains("discover"), "{text}");
    assert!(
        !dir.join(".env").exists(),
        "no .env is written without a terminal"
    );
    assert!(!dir.join("data").exists(), "no data/ directory");
}

#[test]
fn v052_watch_without_a_key_fails_before_watching() {
    let dir = temp_dir("watch-non-tty");
    let home = home_of(&dir);
    let watched = dir.join("watched");
    std::fs::create_dir_all(&watched).expect("watched dir");

    let output = run(
        &dir,
        &home,
        &["watch", "--path", watched.to_str().expect("utf-8")],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );

    assert!(!output.status.success());
    let text = combined(&output);
    assert!(text.contains("no API key is configured"), "{text}");
    assert!(text.contains("kaseifu setup"), "{text}");
}

// ---------------------------------------------------------------------------
// ZC-3 / ZC-4: doctor display
// ---------------------------------------------------------------------------

/// A real (empty) journal, created through the library so `doctor` reads a
/// valid file with 0 operations.
fn write_empty_journal(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("journal dir");
    drop(kaseifu::storage::history::OperationHistory::open(path).expect("create journal"));
    assert!(path.exists(), "journal fixture exists");
}

#[test]
fn v052_doctor_fail_rows_carry_next_commands() {
    let dir = temp_dir("doctor-fail");
    let home = home_of(&dir);
    let completion = dir.join("completion");
    std::fs::create_dir_all(&completion).expect("completion dir");
    let legacy = dir.join("data").join("kaseifu.db");
    write_empty_journal(&legacy);
    let before = std::fs::read(&legacy).expect("read legacy journal");

    let output = run(
        &dir,
        &home,
        &["doctor"],
        &[
            ("KASEIFU_KEY_SOURCE", "env"),
            (
                "KASEIFU_COMPLETION_DIR",
                completion.to_str().expect("utf-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "doctor exits 0: {}",
        stderr_of(&output)
    );
    let text = stdout_of(&output);
    assert!(text.contains("Ready: no ("), "{text}");
    let fails = fail_lines(&text);
    assert!(fails.len() >= 3, "expected several FAIL rows: {text}");
    for line in &fails {
        assert!(
            line.contains("(next: "),
            "every FAIL row names the next action: {line}"
        );
    }
    assert!(
        text.contains("FAIL API Key:") && text.contains("kaseifu setup"),
        "{text}"
    );
    assert!(
        fails
            .iter()
            .any(|line| line.contains("Legacy Journal") && line.contains("0 operation(s)")),
        "the legacy journal row count is shown: {text}"
    );
    assert!(
        fails
            .iter()
            .any(|line| line.contains("Legacy Journal") && line.contains("safe to delete")),
        "zero rows are called out as safe to delete: {text}"
    );
    assert!(
        fails.iter().any(|line| line.contains("Completion")
            && line.contains("kaseifu completions zsh --install")),
        "{text}"
    );

    assert_eq!(
        std::fs::read(&legacy).expect("legacy journal stays readable"),
        before,
        "doctor must not modify the legacy journal"
    );
}

#[test]
fn v052_doctor_reports_an_unreadable_legacy_journal_without_touching_it() {
    let dir = temp_dir("doctor-legacy-unreadable");
    let home = home_of(&dir);
    let legacy = dir.join("data").join("kaseifu.db");
    std::fs::create_dir_all(legacy.parent().expect("parent")).expect("legacy dir");
    std::fs::write(&legacy, b"not a journal at all").expect("write fixture");

    let output = run(&dir, &home, &["doctor"], &[("KASEIFU_KEY_SOURCE", "env")]);

    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    let legacy_line = fail_lines(&text)
        .into_iter()
        .find(|line| line.contains("Legacy Journal"))
        .expect("legacy journal row")
        .to_string();
    assert!(legacy_line.contains("could not read it"), "{legacy_line}");
    assert!(
        legacy_line.contains("not migrated, not deleted"),
        "{legacy_line}"
    );
    assert!(legacy_line.contains("(next: "), "{legacy_line}");
    assert_eq!(
        std::fs::read(&legacy).expect("read"),
        b"not a journal at all",
        "the file must not be modified"
    );
}

#[test]
fn v052_doctor_is_all_ok_in_a_ready_environment() {
    let dir = temp_dir("doctor-ready");
    let home = home_of(&dir);
    let completion = dir.join("completion");
    std::fs::create_dir_all(&completion).expect("completion dir");
    let completion_arg = completion.to_str().expect("utf-8");

    let install = run(
        &dir,
        &home,
        &["completions", "zsh", "--install", "--dir", completion_arg],
        &[],
    );
    assert!(install.status.success(), "{}", stderr_of(&install));

    let output = run(
        &dir,
        &home,
        &["doctor"],
        &[
            ("KASEIFU_KEY_SOURCE", "env"),
            ("JEV_API_KEY", DUMMY_KEY),
            ("KASEIFU_COMPLETION_DIR", completion_arg),
        ],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("Ready: yes"), "{text}");
    assert!(!text.contains("FAIL "), "no failing check: {text}");
    assert!(
        !text.contains(DUMMY_KEY),
        "the key value must never be printed"
    );
    assert!(
        !dir.join("data").exists(),
        "doctor never creates the journal"
    );
}

#[test]
fn v052_doctor_json_keeps_existing_keys_and_adds_the_hint() {
    let dir = temp_dir("doctor-json");
    let home = home_of(&dir);
    let output = run(
        &dir,
        &home,
        &["--format", "json", "doctor"],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );

    assert!(output.status.success(), "{}", stderr_of(&output));
    let json: serde_json::Value = serde_json::from_str(stdout_of(&output).trim()).expect("JSON");
    assert_eq!(json["ready"], false);
    assert!(json["endpoint"].is_string());
    assert!(json["connectivity"].is_string());
    let checks = json["checks"].as_array().expect("checks");
    assert_eq!(checks.len(), 9, "the nine existing checks stay");
    for check in checks {
        assert!(check["name"].is_string());
        assert!(check["ok"].is_boolean());
        assert!(check["detail"].is_string());
        if check["ok"] == false {
            assert!(check["next"].is_string(), "FAIL rows carry a hint: {check}");
        } else {
            assert!(check["next"].is_null());
        }
    }
    let failing = json["failing"].as_array().expect("failing");
    assert!(failing.iter().any(|name| name == "API Key"), "{failing:?}");
}

// ---------------------------------------------------------------------------
// ZC-1: the interactive TTY path (real PTY)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod pty {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// Run `command` on a fresh PTY and answer `prompt_marker` with `input`.
    ///
    /// `libc::openpty` keeps this self-contained (no `script`/`expect`/python),
    /// and the deadlines kill a regression that would hang waiting for input.
    /// The input is written only after the prompt has been observed and the
    /// program has had a moment to disable terminal echo, so the collected
    /// output is what the user would really see.
    ///
    /// stdout stays on the terminal unless `stdout_file` redirects it. The
    /// redirect is how a test proves the prompt reaches the terminal without
    /// polluting a stdout stream the caller wants to parse.
    fn run_in_pty(
        mut command: ProcessCommand,
        stdout_file: Option<&Path>,
        prompt_marker: &[u8],
        input: &[u8],
        timeout: Duration,
    ) -> (std::process::ExitStatus, Vec<u8>) {
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
        let master_write = unsafe { std::fs::File::from_raw_fd(libc::dup(master)) };
        let stdin = unsafe { std::fs::File::from_raw_fd(libc::dup(slave)) };
        let stdout = match stdout_file {
            Some(path) => std::fs::File::create(path).expect("create redirected stdout"),
            None => unsafe { std::fs::File::from_raw_fd(libc::dup(slave)) },
        };
        let stderr = unsafe { std::fs::File::from_raw_fd(libc::dup(slave)) };
        unsafe { libc::close(slave) };

        command
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        let mut child = command.spawn().expect("spawn in pty");

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

        let prompt_deadline = Instant::now() + timeout;
        loop {
            let seen = collected
                .lock()
                .expect("lock")
                .windows(prompt_marker.len())
                .any(|window| window == prompt_marker);
            if seen {
                break;
            }
            if Instant::now() > prompt_deadline {
                let _ = child.kill();
                let _ = child.wait();
                let seen = collected.lock().expect("lock").clone();
                panic!(
                    "the prompt never appeared: {}",
                    String::from_utf8_lossy(&seen)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // The prompt is flushed just before echo is disabled; give the terminal
        // attribute change a moment so the key is never echoed back.
        std::thread::sleep(Duration::from_millis(100));
        let mut writer = master_write;
        writer.write_all(input).expect("write pty input");
        writer.flush().expect("flush pty input");

        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("pty run exceeded {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        };

        let settle = Instant::now() + Duration::from_millis(1500);
        while !done.load(Ordering::SeqCst) && Instant::now() < settle {
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = collected.lock().expect("lock").clone();
        (status, output)
    }

    /// ZC-1 acceptance: with a TTY and no key, `classify` prompts, stores the
    /// key where the mode keeps it, and reaches the decision display.
    #[test]
    fn v052_tty_classify_prompts_stores_and_reaches_the_decision() {
        let dir = temp_dir("pty-classify");
        let home = home_of(&dir);
        let file = dir.join("v052-pty-note-7f31c2.txt");
        std::fs::write(&file, "hello from the pty test").expect("write file");

        let mut command = base_command(&dir, &home);
        command
            .args(["classify", file.to_str().expect("utf-8")])
            .env("KASEIFU_KEY_SOURCE", "env")
            .env("NO_COLOR", "1");
        let (status, output) = run_in_pty(
            command,
            None,
            b"Jev API key",
            format!("{DUMMY_KEY}\n").as_bytes(),
            Duration::from_secs(30),
        );
        let text = String::from_utf8_lossy(&output).into_owned();

        assert!(
            status.success(),
            "classify reached the end of the pipeline: {text}"
        );
        assert!(text.contains("Jev API key"), "the prompt is shown: {text}");
        assert!(
            text.contains("POLICY") && text.contains("LEAVE"),
            "the decision display is reached (no candidates -> LEAVE): {text}"
        );
        assert!(
            !text.contains(DUMMY_KEY),
            "the key must never be echoed or printed: {text}"
        );

        let dotenv = dir.join(".env");
        let saved = std::fs::read_to_string(&dotenv).expect("the key was saved to .env");
        assert!(
            saved.contains(DUMMY_KEY),
            "the .env file holds the key: {saved}"
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&dotenv)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
            ".env is private"
        );
        assert!(
            !home.join("Library/Application Support/kaseifu").exists(),
            "env mode never touches the Keychain-backed configuration"
        );
    }

    /// The prompt belongs on the terminal, never on stdout: with stdout
    /// redirected, `--format json` has to produce a pure JSON file while the
    /// prompt stays visible. Regression guard for the prompt being written to
    /// stdout, which corrupted the JSON and, with stdout redirected, looked
    /// like a silent hang.
    #[test]
    fn v052_tty_json_stdout_stays_parseable_while_prompting() {
        let dir = temp_dir("pty-json");
        let home = home_of(&dir);
        let file = dir.join("v052-pty-json-note-3b9e.txt");
        std::fs::write(&file, "hello from the pty json test").expect("write file");
        let stdout_path = dir.join("stdout.json");

        let mut command = base_command(&dir, &home);
        command
            .args([
                "classify",
                file.to_str().expect("utf-8"),
                "--format",
                "json",
            ])
            .env("KASEIFU_KEY_SOURCE", "env")
            .env("NO_COLOR", "1");
        let (status, terminal) = run_in_pty(
            command,
            Some(&stdout_path),
            b"Jev API key",
            format!("{DUMMY_KEY}\n").as_bytes(),
            Duration::from_secs(30),
        );
        let terminal = String::from_utf8_lossy(&terminal).into_owned();

        assert!(status.success(), "classify reached the end: {terminal}");
        assert!(
            terminal.contains("Jev API key"),
            "the prompt is still shown on the terminal: {terminal}"
        );

        let stdout = std::fs::read(&stdout_path).expect("stdout was redirected to the file");
        let parsed: serde_json::Value =
            serde_json::from_slice(&stdout).expect("the redirected stdout is pure JSON");
        assert!(
            parsed.get("action").is_some(),
            "the decision shape is emitted: {parsed}"
        );
        let text = String::from_utf8_lossy(&stdout).into_owned();
        assert!(
            !text.contains("Jev API key") && !text.contains(DUMMY_KEY),
            "neither the prompt nor the key reaches stdout: {text}"
        );

        let saved = std::fs::read_to_string(dir.join(".env")).expect("the key was saved to .env");
        assert!(
            saved.contains(DUMMY_KEY),
            "the prompted key is stored where env mode keeps it"
        );
    }

    /// A blank `JEV_API_KEY=` — the shape a copied example file leaves behind —
    /// counts as "no key", so the command asks instead of running with an empty
    /// one. The answer replaces that assignment and every other `.env` line is
    /// kept as it was.
    #[test]
    fn v052_tty_prompts_over_a_blank_dotenv_key_and_keeps_other_lines() {
        let dir = temp_dir("pty-blank-dotenv");
        let home = home_of(&dir);
        let file = dir.join("v052-pty-blank-note-51c7.txt");
        std::fs::write(&file, "hello from the blank dotenv test").expect("write file");
        std::fs::write(
            dir.join(".env"),
            "JEV_API_KEY=\nJEV_API_URL=https://example.test\n",
        )
        .expect("seed .env");

        let mut command = base_command(&dir, &home);
        command
            .args(["classify", file.to_str().expect("utf-8")])
            .env("KASEIFU_KEY_SOURCE", "env")
            .env("NO_COLOR", "1");
        let (status, output) = run_in_pty(
            command,
            None,
            b"Jev API key",
            format!("{DUMMY_KEY}\n").as_bytes(),
            Duration::from_secs(30),
        );
        let text = String::from_utf8_lossy(&output).into_owned();

        assert!(status.success(), "classify reached the end: {text}");
        assert!(
            text.contains("Jev API key"),
            "a blank key is treated as missing, so the command asks: {text}"
        );
        assert!(!text.contains(DUMMY_KEY), "the key is never echoed: {text}");

        let saved = std::fs::read_to_string(dir.join(".env")).expect("read .env back");
        assert!(
            saved.contains(&format!("JEV_API_KEY={DUMMY_KEY}")),
            "the blank assignment is replaced in place: {saved}"
        );
        assert!(
            !saved.contains("JEV_API_KEY=\n"),
            "no blank assignment survives: {saved}"
        );
        assert!(
            saved.contains("JEV_API_URL=https://example.test"),
            "every other line is kept: {saved}"
        );
    }
}

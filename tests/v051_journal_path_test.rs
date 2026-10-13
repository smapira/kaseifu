//! v0.5.1 journal location tests (R5).
//!
//! The journal default moved from the CWD-relative `data/kaseifu.db` to the
//! absolute `~/Library/Application Support/kaseifu/kaseifu.db`. These tests
//! run the real binary inside a temporary working directory with an isolated
//! HOME, so the developer's journal, configuration and Keychain are never
//! read or written; the old CWD-relative journal is only ever *detected*.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-journal-path-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
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
                    "kaseifu exceeded {timeout:?}: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn run(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        // Never let the automatic completion refresh touch real directories.
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env_remove("KASEIFU_KEY_SOURCE")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR");
    command.args(args);
    run_with_deadline(command, Duration::from_secs(10))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The journal path the binary resolves for an isolated HOME.
fn resolved_journal(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("kaseifu.db")
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[test]
fn v051_journal_path_is_absolute_and_under_the_app_dir() {
    let path = kaseifu::storage::history::journal_path();
    assert!(
        path.is_absolute(),
        "the journal path must be absolute: {path:?}"
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("kaseifu.db")
    );
    assert_eq!(
        path.parent(),
        Some(kaseifu::config::app_dir().as_path()),
        "the journal lives next to the user configuration"
    );
    let legacy = kaseifu::storage::history::LEGACY_DATABASE_PATH;
    assert_eq!(
        legacy, "data/kaseifu.db",
        "the old path stays detection-only"
    );
}

// ---------------------------------------------------------------------------
// `doctor` reports the resolved path
// ---------------------------------------------------------------------------

#[test]
fn v051_doctor_reports_the_resolved_journal_path_and_creates_nothing() {
    let dir = temp_dir("doctor-path");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");

    let output = run(&dir, &home, &["doctor"]);
    assert!(output.status.success(), "{}", output.stderr.is_empty());
    let text = stdout_of(&output);

    let journal = resolved_journal(&home);
    assert!(
        text.contains(&format!(
            "ok Journal: {} (no journal yet)",
            journal.display()
        )),
        "doctor must print the resolved journal path: {text}"
    );
    assert!(
        text.contains("ok Legacy Journal: no data/kaseifu.db in the current directory"),
        "{text}"
    );
    assert!(
        !journal.exists(),
        "doctor must not create the journal: {journal:?}"
    );
}

#[test]
fn v051_journal_path_is_identical_from_different_working_directories() {
    let dir = temp_dir("cwd-independence");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("working directory");

    let from_root = run(&dir, &home, &["doctor"]);
    let from_elsewhere = run(&elsewhere, &home, &["doctor"]);

    let expected = format!("Journal: {} ", resolved_journal(&home).display());
    assert!(
        stdout_of(&from_root).contains(&expected),
        "{}",
        stdout_of(&from_root)
    );
    assert!(
        stdout_of(&from_elsewhere).contains(&expected),
        "running from another directory must resolve the same journal: {}",
        stdout_of(&from_elsewhere)
    );
}

// ---------------------------------------------------------------------------
// The old CWD-relative journal is detected, never migrated
// ---------------------------------------------------------------------------

#[test]
fn v051_doctor_detects_a_legacy_journal_without_touching_it() {
    let dir = temp_dir("doctor-legacy");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");

    let legacy = dir.join("data").join("kaseifu.db");
    std::fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy dir");
    std::fs::write(&legacy, b"legacy-journal-bytes").expect("legacy journal");

    let output = run(&dir, &home, &["doctor"]);
    assert!(output.status.success());
    let text = stdout_of(&output);

    assert!(
        text.contains("FAIL Legacy Journal: found data/kaseifu.db"),
        "the leftover journal must be visible: {text}"
    );
    assert!(
        text.contains("could not read it") && text.contains("not migrated, not deleted"),
        "an unreadable leftover is reported as such and never touched: {text}"
    );
    assert!(
        text.contains("(next: "),
        "the FAIL row names the next action: {text}"
    );
    assert!(
        text.contains(&format!(
            "current journal: {}",
            resolved_journal(&home).display()
        )),
        "{text}"
    );

    assert_eq!(
        std::fs::read(&legacy).expect("legacy journal stays readable"),
        b"legacy-journal-bytes",
        "the legacy journal must not be modified"
    );
    let entries: Vec<_> = std::fs::read_dir(legacy.parent().expect("legacy parent"))
        .expect("legacy dir")
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "no files are added next to the legacy journal"
    );
    assert!(
        !resolved_journal(&home).exists(),
        "doctor must not create the new journal either"
    );
}

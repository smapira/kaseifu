//! CLI-level checks for `KASEIFU_KEY_SOURCE` (real binary, child processes).
//!
//! Every child runs inside a temporary directory with an isolated HOME and all
//! key-related variables removed, so the developer's `.env`, configuration and
//! Keychain are never involved. Temporary `.env` files contain obvious dummy
//! values only. Runs are bounded by a deadline: a regression that reintroduces
//! a blocking Keychain prompt fails the test instead of freezing the suite.
//! (The `keychain` mode is deliberately not exercised here: it must read the
//! real Keychain, which only the injected-store tests may simulate.)

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

const DUMMY_KEY: &str = "test-key-not-a-secret-0123456789";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-key-source-{}-{}",
        std::process::id(),
        name
    ));
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
        // Never let the automatic completion refresh touch real directories.
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
                    "kaseifu exceeded {timeout:?} (a blocking Keychain prompt?): {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    let mut command = base_command(dir, &home);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    run_with_deadline(command, Duration::from_secs(10))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn write_env(dir: &Path, contents: &str) {
    std::fs::write(dir.join(".env"), contents).expect("write .env");
}

// ---------------------------------------------------------------------------
// K0/K1. Debug default and explicit `env` mode
// ---------------------------------------------------------------------------

#[test]
fn cli_debug_default_is_env_and_never_blocks_on_the_keychain() {
    let dir = temp_dir("debug-default");
    let output = run(&dir, &["auth", "status"], &[]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("KASEIFU_KEY_SOURCE: env (default)"), "{text}");
    assert!(text.contains("API Key: Not configured"), "{text}");
    assert!(text.contains("Source: not configured"), "{text}");
}

#[test]
fn cli_explicit_env_mode_reports_missing_fast() {
    let dir = temp_dir("env-missing");
    let output = run(&dir, &["auth", "status"], &[("KASEIFU_KEY_SOURCE", "env")]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("KASEIFU_KEY_SOURCE: env"), "{text}");
    assert!(!text.contains("(default)"), "{text}");
    assert!(text.contains("Source: not configured"), "{text}");
}

#[test]
fn cli_env_mode_doctor_reports_missing_without_hanging() {
    let dir = temp_dir("env-doctor");
    let output = run(&dir, &["doctor"], &[("KASEIFU_KEY_SOURCE", "env")]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(
        text.contains("FAIL API Key: not configured (KASEIFU_KEY_SOURCE=env)"),
        "{text}"
    );
}

#[test]
fn cli_doctor_json_names_the_api_key_check() {
    let dir = temp_dir("doctor-json");
    let output = run(
        &dir,
        &["--format", "json", "doctor"],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("doctor JSON");
    let checks = json["checks"].as_array().expect("checks array");
    let api_key = checks
        .iter()
        .find(|check| check["name"] == "API Key")
        .expect("API Key check is present");
    assert_eq!(api_key["ok"], false);
}

// ---------------------------------------------------------------------------
// K1. `.env` as the development source
// ---------------------------------------------------------------------------

#[test]
fn cli_env_mode_reads_the_project_dotenv() {
    let dir = temp_dir("dotenv");
    write_env(&dir, &format!("JEV_API_KEY={DUMMY_KEY}\n"));

    let status = run(&dir, &["auth", "status"], &[("KASEIFU_KEY_SOURCE", "env")]);
    assert!(status.status.success(), "{}", stderr_of(&status));
    let text = stdout_of(&status);
    assert!(text.contains("API Key: Configured"), "{text}");
    assert!(text.contains("Source: .env"), "{text}");
    assert!(
        !text.contains(DUMMY_KEY),
        "the key value must never be printed"
    );

    let doctor = run(&dir, &["doctor"], &[("KASEIFU_KEY_SOURCE", "env")]);
    assert!(doctor.status.success());
    assert!(
        stdout_of(&doctor).contains("ok API Key: provided via .env"),
        "{}",
        stdout_of(&doctor)
    );
}

#[test]
fn cli_auth_status_json_keeps_existing_keys_and_adds_the_source() {
    let dir = temp_dir("status-json");
    write_env(&dir, &format!("JEV_API_KEY={DUMMY_KEY}\n"));
    let output = run(
        &dir,
        &["--format", "json", "auth", "status"],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("status JSON");
    assert_eq!(json["provider"], "jev");
    assert_eq!(json["api_key_configured"], true);
    assert_eq!(json["storage"], "Environment");
    assert_eq!(json["api_key_source"], ".env");
    assert_eq!(json["key_source_mode"], "env");
    assert_eq!(json["key_source_default"], false);
    assert!(json["endpoint"].is_string());
    assert!(!stdout_of(&output).contains(DUMMY_KEY));
}

#[test]
fn cli_process_environment_beats_dotenv_for_mode_and_key() {
    let dir = temp_dir("precedence");
    write_env(
        &dir,
        &format!("KASEIFU_KEY_SOURCE=env\nJEV_API_KEY={DUMMY_KEY}\n"),
    );
    // `auto` still resolves the key from the process environment first, so the
    // real Keychain is never read even though `.env` asks for `env`.
    let output = run(
        &dir,
        &["auth", "status"],
        &[
            ("KASEIFU_KEY_SOURCE", "auto"),
            ("JEV_API_KEY", "env-key-not-a-secret-9876543210"),
        ],
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("KASEIFU_KEY_SOURCE: auto"), "{text}");
    assert!(!text.contains("(default)"), "{text}");
    assert!(text.contains("Source: env"), "{text}");
}

#[test]
fn cli_dotenv_can_pin_the_mode() {
    let dir = temp_dir("dotenv-mode");
    write_env(
        &dir,
        &format!("KASEIFU_KEY_SOURCE=env\nJEV_API_KEY={DUMMY_KEY}\n"),
    );
    let output = run(&dir, &["auth", "status"], &[]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("KASEIFU_KEY_SOURCE: env"), "{text}");
    assert!(!text.contains("(default)"), "{text}");
    assert!(text.contains("Source: .env"), "{text}");
}

// ---------------------------------------------------------------------------
// K1. Jev-calling commands and write commands
// ---------------------------------------------------------------------------

#[test]
fn cli_env_mode_jev_command_fails_fast_without_a_key() {
    let dir = temp_dir("classify-missing");
    let file = dir.join("note.txt");
    std::fs::write(&file, "hello").expect("write file");
    let output = run(
        &dir,
        &["classify", file.to_str().expect("utf-8 path")],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );
    assert!(!output.status.success());
    let text = format!("{}{}", stdout_of(&output), stderr_of(&output));
    assert!(text.contains("no API key is configured"), "{text}");
    assert!(text.contains("KASEIFU_KEY_SOURCE=env"), "{text}");
}

#[test]
fn cli_env_mode_write_commands_never_touch_the_keychain() {
    let dir = temp_dir("write-gate");
    // stdin is not a TTY: a broken gate would prompt (or read the Keychain)
    // and therefore fail or hang instead of printing the guidance.
    let set = run(&dir, &["auth", "set-key"], &[("KASEIFU_KEY_SOURCE", "env")]);
    assert!(set.status.success(), "{}", stderr_of(&set));
    assert!(
        stdout_of(&set).contains("the macOS Keychain is not used"),
        "{}",
        stdout_of(&set)
    );

    let remove = run(
        &dir,
        &["--format", "json", "auth", "remove-key", "--yes"],
        &[("KASEIFU_KEY_SOURCE", "env")],
    );
    assert!(remove.status.success(), "{}", stderr_of(&remove));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&remove).trim()).expect("remove-key JSON");
    assert_eq!(json["removed"], false);
    assert_eq!(json["reason"], "env-mode");
    assert_eq!(json["key_source_mode"], "env");
}

#[test]
fn cli_env_mode_setup_shows_guidance_and_skips_the_key_prompt() {
    let dir = temp_dir("setup-env");
    let output = run(&dir, &["setup"], &[("KASEIFU_KEY_SOURCE", "env")]);
    let text = stdout_of(&output);
    assert!(
        text.contains("env mode (KASEIFU_KEY_SOURCE=env): the macOS Keychain is not used"),
        "{text}"
    );
    assert!(
        !text.contains("Jev API key"),
        "the key prompt must be skipped: {text}"
    );
    // Without a TTY the endpoint prompt stops the wizard (exit 1), which
    // proves the run never reached a Keychain call before failing.
    assert!(!output.status.success());
    assert!(
        stderr_of(&output).contains("interactive input needs a terminal"),
        "{}",
        stderr_of(&output)
    );
}

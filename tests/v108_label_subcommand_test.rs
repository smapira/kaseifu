//! v108 `label` is the canonical rename subcommand (`rename` stays hidden).
//!
//! `kaseifu label` behaves exactly as `rename` did: same guidance, same
//! management flags, same JSON keys (additive-only, so the `"rename"`
//! document keys are unchanged). `rename` remains a hidden transitional
//! alias. CLI children run with an isolated HOME and stdin closed.

use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-label-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn run(args: &[&str]) -> (Output, PathBuf) {
    let dir = temp_dir("run");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(&dir)
        .env("HOME", &home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env_remove("JEV_API_KEY")
        .args(args);
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
    let output = child.wait_with_output().expect("collect output");
    (output, dir)
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn v108_label_guidance_names_the_canonical_command() {
    let (output, dir) = run(&["label"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(text.contains("kaseifu label <file-or-folder>"), "{text}");
    assert!(text.contains("kaseifu label --patterns"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v108_rename_stays_a_hidden_alias_with_the_same_help() {
    let (label_help, dir) = run(&["label", "--help"]);
    assert!(label_help.status.success());
    let label_text = stdout_of(&label_help);
    let (rename_help, _) = run(&["rename", "--help"]);
    assert!(rename_help.status.success());
    assert_eq!(
        stdout_of(&rename_help),
        label_text,
        "the alias resolves to the same command"
    );
    // The alias works for real work too, not just --help.
    let (output, _) = run(&["rename"]);
    assert!(output.status.success());
    assert!(
        stdout_of(&output).contains("kaseifu label <file-or-folder>"),
        "alias guidance names the canonical command"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v108_top_level_help_lists_label() {
    let (output, dir) = run(&["--help"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(text.contains("label"), "{text}");
    assert!(text.is_ascii() && !text.contains('\u{2014}'), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v108_label_json_guidance_keeps_the_stable_key() {
    let (output, dir) = run(&["label", "--format", "json"]);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_str(&stdout_of(&output)).expect("JSON parses");
    assert_eq!(value["rename"]["status"], serde_json::json!("guidance"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v108_label_patterns_lists_the_rules() {
    let (output, dir) = run(&["label", "--patterns"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(text.contains("P01"), "{text}");
    assert!(text.contains("date-title"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

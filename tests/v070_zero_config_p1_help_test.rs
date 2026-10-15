//! v0.7.0 P1 ZC-6 / ZC-8 / ZC-12 tests: help output contract and scope.
//!
//! Runs the real binary: the help text users see is the artifact under test.
//! ZC-6 (policies and --execute in the top-level help) and ZC-8 (image scope,
//! video out of scope) are pinned here; ZC-12 extends this file with the
//! whole-help forbidden-word scan and the flag support notes.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-p1-help-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

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
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1");
    command
}

fn run(dir: &Path, home: &Path, args: &[&str]) -> Output {
    base_command(dir, home)
        .args(args)
        .output()
        .expect("kaseifu runs")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn help_output(dir: &Path, home: &Path, args: &[&str]) -> String {
    let output = run(dir, home, args);
    assert!(
        output.status.success(),
        "{args:?} must exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout_of(&output);
    assert!(!text.is_empty(), "{args:?} printed no help");
    text
}

// ---------------------------------------------------------------------------
// ZC-8: scope of classification (images only; video is out)
// ---------------------------------------------------------------------------

/// The nine image extensions the predicate accepts, in README order.
const IMAGE_EXTENSIONS: [&str; 9] = [
    "jpg", "jpeg", "png", "heic", "heif", "hif", "tif", "tiff", "webp",
];

#[test]
fn readme_scope_list_matches_the_image_predicate() {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md readable");
    let marker = "Today's supported target is image files:";
    let start = readme.find(marker).expect("README scope marker");
    let paragraph = readme[start..].split("\n\n").next().expect("paragraph");
    let listed: Vec<&str> = paragraph
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|token| token.strip_prefix('.').unwrap_or(token))
        .collect();

    assert_eq!(listed, IMAGE_EXTENSIONS, "README list: {listed:?}");
    for extension in IMAGE_EXTENSIONS {
        let probe = format!("probe.{extension}");
        assert!(
            kaseifu::context::image_metadata::is_image_file(Path::new(&probe)),
            "README claims {extension} is classified"
        );
    }
    for video in ["mp4", "mov"] {
        let probe = format!("probe.{video}");
        assert!(
            !kaseifu::context::image_metadata::is_image_file(Path::new(&probe)),
            "{video} must stay out of scope"
        );
    }
    assert!(
        readme
            .contains("Video files (MP4, MOV, and other video containers) are not classified yet"),
        "the video exclusion must be explicit"
    );
    // R6: Spotlight indexing is asynchronous, so a fresh write may be
    // missing from Discovery's `mdfind` search for a few seconds.
    assert!(
        readme.contains("Spotlight indexing is asynchronous"),
        "R6 must be stated"
    );
    assert!(
        readme.contains("may not appear in `mdfind`"),
        "R6 must name mdfind"
    );
    assert!(
        readme.contains("4 s measured on an idle machine"),
        "R6 keeps the measured value"
    );
}

#[test]
fn help_states_the_image_scope_and_video_exclusion() {
    let dir = temp_dir("scope");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    for flag in ["-h", "--help"] {
        let text = help_output(&dir, &home, &[flag]);
        assert!(
            text.contains("Scope: image files are classified today"),
            "{flag}: missing scope:\n{text}"
        );
        assert!(
            text.contains("tif, tiff, webp)"),
            "{flag}: extension list:\n{text}"
        );
        assert!(
            text.contains("Video (mp4, mov) is not classified yet and its capture"),
            "{flag}: missing video exclusion:\n{text}"
        );
    }
}
// ---------------------------------------------------------------------------
// ZC-12: every help output stays free of internal wording
// ---------------------------------------------------------------------------

/// Visible command names listed under clap's `Commands:` heading.
fn visible_commands(help: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line.trim_end() == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.starts_with("  ") && !line.starts_with("    ") {
            let name = line.split_whitespace().next().unwrap_or("");
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                commands.push(name.to_string());
            }
        } else if !line.trim().is_empty() {
            break;
        }
    }
    commands
}

#[test]
fn every_help_output_avoids_internal_phases_types_and_pipeline_words() {
    let dir = temp_dir("forbidden");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let top = help_output(&dir, &home, &["--help"]);
    assert!(
        !top.contains("discover"),
        "the hidden command must not appear in the top-level help:\n{top}"
    );

    let mut targets: Vec<Vec<String>> = vec![Vec::new()];
    for command in visible_commands(&top) {
        if command == "help" {
            continue;
        }
        targets.push(vec![command.clone()]);
        let nested_help = help_output(&dir, &home, &[command.as_str(), "--help"]);
        for nested in visible_commands(&nested_help) {
            if nested == "help" {
                continue;
            }
            targets.push(vec![command.clone(), nested]);
        }
    }
    // The hidden command stays reachable: its own help is scanned too.
    targets.push(vec!["discover".to_string()]);

    let forbidden = [
        "Phase",
        "FileContext",
        "NormalizedFileContext",
        "JevDecision",
        "DecisionContext",
        "CandidateDestination",
        "OrganizeReport",
        "MovePlan",
        "PlanError",
        "Observe ->",
        "decision -> policy",
        "policy -> plan",
        "DETAILS layer",
    ];

    for target in &targets {
        for flag in ["-h", "--help"] {
            let mut args: Vec<&str> = target.iter().map(String::as_str).collect();
            args.push(flag);
            let text = help_output(&dir, &home, &args);
            for word in forbidden {
                assert!(!text.contains(word), "{args:?} leaks {word}:\n{text}");
            }
            assert!(text.is_ascii(), "{args:?} must stay ASCII-only:\n{text}");
        }
    }
    assert!(
        targets.len() > 12,
        "the walk must reach the visible commands and nested subcommands: {targets:?}"
    );
}

// ---------------------------------------------------------------------------
// ZC-12: displayed flags state what they do (including what they do not)
// ---------------------------------------------------------------------------

#[test]
fn format_help_states_which_commands_support_json() {
    let dir = temp_dir("format-scope");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let text = help_output(&dir, &home, &["--help"]);
    assert!(
        text.contains("`json` applies to inspect, classify, organize, doctor, setup, auth"),
        "got:\n{text}"
    );
    assert!(
        text.contains("undo and scan. `watch` emits one JSON object per line"),
        "got:\n{text}"
    );
}

#[test]
fn help_keeps_the_safety_and_key_path_information() {
    let dir = temp_dir("safety");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let top = help_output(&dir, &home, &["--help"]);
    assert!(
        top.contains("Files are never moved unless you pass --execute"),
        "got:\n{top}"
    );

    let organize = help_output(&dir, &home, &["organize", "--help"]);
    assert!(
        organize.contains("default is a dry run; cannot be combined with --dry-run"),
        "got:\n{organize}"
    );
    assert!(
        organize.contains("cannot be combined with --execute"),
        "got:\n{organize}"
    );
    let watch = help_output(&dir, &home, &["watch", "--help"]);
    assert!(watch.contains("default is a dry run"), "got:\n{watch}");

    let setup = help_output(&dir, &home, &["setup", "--help"]);
    assert!(
        setup.contains("macOS Keychain unless") && setup.contains("KASEIFU_KEY_SOURCE=env"),
        "got:\n{setup}"
    );
    let auth = help_output(&dir, &home, &["auth", "--help"]);
    assert!(
        auth.contains("macOS Keychain") && auth.contains("KASEIFU_KEY_SOURCE=env"),
        "got:\n{auth}"
    );
}

#[test]
fn completions_help_states_that_install_is_zsh_only() {
    let dir = temp_dir("completions");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let text = help_output(&dir, &home, &["completions", "--help"]);
    assert!(
        text.contains("only zsh can be installed")
            && text.contains("any supported shell can be printed"),
        "got:\n{text}"
    );
    let short = help_output(&dir, &home, &["completions", "-h"]);
    assert!(short.contains("zsh only"), "got:\n{short}");
}

#[test]
fn scan_help_describes_the_banner_only_reality() {
    let dir = temp_dir("scan");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    let text = help_output(&dir, &home, &["scan", "--help"]);
    assert!(
        text.contains("scanning is not implemented yet"),
        "got:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// ZC-6: policies are explained where the user looks first
// ---------------------------------------------------------------------------

#[test]
fn top_level_help_explains_the_three_policies_and_the_thresholds() {
    let dir = temp_dir("policies");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home");

    for flag in ["-h", "--help"] {
        let text = help_output(&dir, &home, &[flag]);
        for needle in [
            "AUTO MOVE",
            "REVIEW",
            "LEAVE",
            "confidence >= 0.85 and margin >= 0.40",
            "confidence >= 0.60",
            "kaseifu config show",
            "never moved unless you pass --execute",
            "not a command",
        ] {
            assert!(text.contains(needle), "{flag}: missing {needle}:\n{text}");
        }
        assert!(text.is_ascii(), "{flag}: help must be ASCII only:\n{text}");
    }
}

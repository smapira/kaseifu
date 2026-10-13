//! Completion generation, installation, and best-effort refresh.
//!
//! Every test stays inside a temporary directory: spawned commands always get
//! `KASEIFU_COMPLETION_DIR` pointing there, so the developer's real
//! site-functions directory (`/opt/homebrew/share/zsh/site-functions` on Apple
//! Silicon) is never written to.

use clap::{CommandFactory, Parser, Subcommand};
use kaseifu::completion::{self, RefreshPlan, Status, WriteOutcome};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-completion-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// Run the real binary with a temporary completion directory.
fn kaseifu(dir: &Path, args: &[&str]) -> Output {
    child(dir, args, &[])
}

fn child(dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .args(args)
        .env(completion::ENV_DIR, dir)
        .env_remove(completion::ENV_DISABLE)
        // The key source is process-wide state: a developer who exported
        // KASEIFU_KEY_SOURCE must not steer these child processes into the
        // real Keychain (the tests always provide JEV_API_KEY instead).
        .env_remove("KASEIFU_KEY_SOURCE");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("kaseifu runs")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn zsh() -> Option<String> {
    for candidate in ["/bin/zsh", "zsh"] {
        let works = ProcessCommand::new(candidate)
            .arg("--version")
            .output()
            .is_ok();
        if works {
            return Some(candidate.to_string());
        }
    }
    None
}

fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

// ---------------------------------------------------------------------------
// Synthetic CLI: every hidden shape in one tree
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "synthetic", version = "0.0.1")]
struct Synthetic {
    #[command(subcommand)]
    command: SyntheticCommands,
}

#[derive(Subcommand)]
enum SyntheticCommands {
    /// Visible leaf.
    Visible { file: String },
    /// Temporary probe.
    #[command(hide = true)]
    SecretLeaf { file: String },
    /// Temporary probe.
    #[command(hide = true)]
    SecretParent {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Visible parent.
    Parent {
        #[command(subcommand)]
        action: ParentAction,
    },
}

#[derive(Subcommand)]
enum SecretAction {
    /// Buried child.
    Buried,
}

#[derive(Subcommand)]
enum ParentAction {
    /// Kept child.
    Kept,
    /// Concealed child.
    #[command(hide = true)]
    Concealed,
}

fn synthetic_script() -> String {
    let mut cmd = Synthetic::command();
    let bytes = completion::render(clap_complete::Shell::Zsh, &mut cmd);
    String::from_utf8(bytes).expect("zsh script is UTF-8")
}

// ---------------------------------------------------------------------------
// Rendering (T2 / T3)
// ---------------------------------------------------------------------------

#[test]
fn completion_zsh_starts_with_compdef_then_the_version_marker() {
    let script = synthetic_script();
    let mut lines = script.lines();
    assert_eq!(lines.next(), Some("#compdef synthetic"));
    assert_eq!(lines.next(), Some(completion::marker().as_str()));
}

#[test]
fn completion_zsh_drops_hidden_commands_and_keeps_visible_ones() {
    let script = synthetic_script();
    for hidden in ["secret-leaf", "secret-parent", "buried", "concealed"] {
        assert!(
            !script.contains(hidden),
            "hidden command {hidden} must not appear"
        );
    }
    assert!(!script.contains("Temporary probe."));
    assert!(!script.contains("Buried child."));
    assert!(!script.contains("Concealed child."));
    for visible in ["visible", "parent", "kept"] {
        assert!(script.contains(visible), "{visible} must remain");
    }
    assert!(script.contains("_synthetic__subcmd__parent_commands"));
    assert!(script.contains("'kept:Kept child'"));
}

#[test]
fn completion_zsh_stays_structurally_sound_after_filtering() {
    let script = synthetic_script();
    let lines: Vec<&str> = script.lines().collect();
    let guards = lines
        .iter()
        .filter(|line| line.trim_start().starts_with("(( $+functions["))
        .count();
    let definitions = lines
        .iter()
        .filter(|line| line.ends_with("() {") && line.starts_with('_'))
        .count();
    // Every guarded definition plus the entry function itself.
    assert_eq!(
        definitions,
        guards + 1,
        "function guards and definitions pair up"
    );
    let cases = lines
        .iter()
        .filter(|line| line.trim_start().starts_with("case "))
        .count();
    let esacs = lines.iter().filter(|line| line.trim() == "esac").count();
    assert_eq!(cases, esacs, "case/esac blocks balance");
}

// ---------------------------------------------------------------------------
// The shipped CLI (T7)
// ---------------------------------------------------------------------------

#[test]
fn completion_cli_prints_a_script_and_writes_nothing() {
    let dir = temp_dir("print");
    let output = kaseifu(&dir, &["completions", "zsh"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let script = stdout_of(&output);
    assert!(script.starts_with("#compdef kaseifu\n"));
    assert!(script.contains(&completion::marker()));
    for command in total_top_level_commands() {
        assert!(
            script.contains(&format!("'{command}:")),
            "{command} must be offered"
        );
        assert!(script.contains(&format!("({command})")), "{command} arm");
    }
    for nested in ["set-key", "status", "remove-key", "show", "set"] {
        assert!(
            script.contains(&format!("'{nested}:")),
            "{nested} must be offered"
        );
    }
    assert!(!script.contains("discover"), "hidden command must not leak");
    assert!(
        std::fs::read_dir(&dir).expect("read dir").next().is_none(),
        "printing must not write anything"
    );
}

#[test]
fn completion_cli_script_is_valid_zsh() {
    let Some(zsh) = zsh() else {
        eprintln!("skipping: zsh is not installed");
        return;
    };
    let dir = temp_dir("syntax");
    let output = kaseifu(&dir, &["completions", "zsh"]);
    let script = write_file(&dir, "_kaseifu-syntax", &stdout_of(&output));
    let status = ProcessCommand::new(&zsh)
        .arg("-n")
        .arg(&script)
        .status()
        .expect("zsh -n runs");
    assert!(status.success(), "zsh -n must accept the generated script");
}

/// Command names from `--help`, so the script can never lose one silently.
fn total_top_level_commands() -> Vec<String> {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"))
        .arg("--help")
        .env(completion::ENV_DISABLE, "1")
        .output()
        .expect("kaseifu --help runs");
    let help = stdout_of(&output);
    let mut commands = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            if line.trim().is_empty() {
                break;
            }
            if let Some(name) = line.split_whitespace().next() {
                commands.push(name.to_string());
            }
        }
    }
    assert!(
        commands.contains(&"inspect".to_string()),
        "help lists the commands: {help}"
    );
    assert!(
        !commands.contains(&"discover".to_string()),
        "hidden commands stay out of help"
    );
    commands
}

// ---------------------------------------------------------------------------
// Installation (T4 / T5)
// ---------------------------------------------------------------------------

#[test]
fn completion_install_is_idempotent_and_leaves_no_temp_files() {
    let dir = temp_dir("install");
    let mut cmd = Synthetic::command();
    assert_eq!(
        completion::install(&dir, &mut cmd).expect("first install"),
        WriteOutcome::Written
    );
    assert_eq!(
        completion::install(&dir, &mut cmd).expect("second install"),
        WriteOutcome::Unchanged
    );
    let names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, vec![completion::FILE_NAME.to_string()]);
}

#[test]
fn completion_candidates_follow_the_documented_order() {
    let home = Path::new("/home/tester");
    let xdg = Path::new("/data");
    let dirs = completion::candidates(Some(home), Some(xdg));
    let rendered: Vec<String> = dirs.iter().map(|dir| dir.display().to_string()).collect();
    assert_eq!(
        rendered,
        vec![
            "/opt/homebrew/share/zsh/site-functions",
            "/usr/local/share/zsh/site-functions",
            "/home/tester/.zprezto/modules/completion/external/src",
            "/data/zsh/site-functions",
            "/home/tester/.zsh/completions",
        ]
    );
    let fallback = completion::candidates(Some(home), None);
    assert_eq!(
        fallback[3].display().to_string(),
        "/home/tester/.local/share/zsh/site-functions"
    );
}

#[test]
fn completion_detect_skips_missing_and_unwritable_candidates() {
    let root = temp_dir("detect");
    let good = root.join("good");
    let read_only = root.join("read-only");
    std::fs::create_dir_all(&good).expect("good dir");
    std::fs::create_dir_all(&read_only).expect("read-only dir");
    std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let missing = root.join("missing");
    let candidates = vec![missing.clone(), read_only.clone(), good.clone()];
    assert_eq!(completion::detect(&candidates), Some(good));
    assert_eq!(completion::detect(&[missing, read_only]), None);
}

#[test]
fn completion_status_distinguishes_missing_stale_and_installed() {
    let dir = temp_dir("status");
    assert_eq!(completion::status_in(&dir), Status::Missing);
    write_file(
        &dir,
        completion::FILE_NAME,
        "#compdef kaseifu\n# kaseifu completions v0.0.1\n",
    );
    assert_eq!(completion::status_in(&dir), Status::Stale);
    assert_eq!(
        completion::installed_version(&dir).as_deref(),
        Some("0.0.1")
    );
    let mut cmd = Synthetic::command();
    completion::install(&dir, &mut cmd).expect("install");
    assert_eq!(completion::status_in(&dir), Status::Installed);
}

#[test]
fn completion_cli_install_is_idempotent_and_reports_the_path() {
    let dir = temp_dir("cli-install");
    let first = kaseifu(&dir, &["completions", "zsh", "--install"]);
    assert!(first.status.success(), "stderr: {}", stderr_of(&first));
    let path = dir.join(completion::FILE_NAME);
    assert!(path.is_file());
    assert!(stdout_of(&first).contains(&path.display().to_string()));
    let before = std::fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = kaseifu(&dir, &["completions", "zsh", "--install"]);
    assert!(second.status.success());
    let after = std::fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    assert_eq!(before, after, "a second install must not rewrite the file");
    let json = kaseifu(
        &dir,
        &["--format", "json", "completions", "zsh", "--install"],
    );
    assert!(stdout_of(&json).contains("\"status\":\"unchanged\""));
}

#[test]
fn completion_cli_install_rejects_unusable_requests() {
    let dir = temp_dir("cli-reject");
    let missing = dir.join("does-not-exist");
    let output = kaseifu(
        &dir,
        &[
            "completions",
            "zsh",
            "--install",
            "--dir",
            missing.to_str().expect("utf-8"),
        ],
    );
    assert!(!output.status.success());
    assert!(stderr_of(&output).contains("--dir is not a writable directory"));
    assert!(!missing.exists());
    let fish = kaseifu(&dir, &["completions", "fish", "--install"]);
    assert!(!fish.status.success());
    assert!(stderr_of(&fish).contains("zsh only"));
}

// ---------------------------------------------------------------------------
// Automatic refresh (T6 / T8)
// ---------------------------------------------------------------------------

#[test]
fn completion_refresh_regenerates_a_stale_script_without_touching_output() {
    let dir = temp_dir("refresh");
    let install = kaseifu(&dir, &["completions", "zsh", "--install"]);
    assert!(install.status.success());
    let path = dir.join(completion::FILE_NAME);

    let control = kaseifu(&dir, &["inspect", "/nonexistent-kaseifu-completion-test"]);
    std::fs::write(&path, "#compdef kaseifu\n# kaseifu completions v0.0.1\n").expect("tamper");
    assert_eq!(completion::status_in(&dir), Status::Stale);

    let refreshed = kaseifu(&dir, &["inspect", "/nonexistent-kaseifu-completion-test"]);
    assert_eq!(completion::status_in(&dir), Status::Installed);
    assert_eq!(control.status.code(), refreshed.status.code());
    assert_eq!(stdout_of(&control), stdout_of(&refreshed));
    assert_eq!(stderr_of(&control), stderr_of(&refreshed));
}

#[test]
fn completion_refresh_is_disabled_by_environment() {
    let dir = temp_dir("refresh-off");
    let path = write_file(
        &dir,
        completion::FILE_NAME,
        "#compdef kaseifu\n# kaseifu completions v0.0.1\n",
    );
    let output = child(
        &dir,
        &["inspect", "/nonexistent-kaseifu-completion-test"],
        &[(completion::ENV_DISABLE, "1")],
    );
    assert!(!output.status.success());
    let contents = std::fs::read_to_string(&path).expect("read");
    assert!(
        contents.contains("v0.0.1"),
        "disabled refresh must not rewrite"
    );
}

#[test]
fn completion_refresh_survives_a_read_only_directory() {
    let dir = temp_dir("refresh-read-only");
    let path = dir.join(completion::FILE_NAME);
    write_file(
        &dir,
        completion::FILE_NAME,
        "#compdef kaseifu\n# kaseifu completions v0.0.1\n",
    );
    let control = kaseifu(&dir, &["inspect", "/nonexistent-kaseifu-completion-test"]);
    std::fs::write(&path, "#compdef kaseifu\n# kaseifu completions v0.0.1\n").expect("tamper");
    let before = std::fs::read(&path).expect("read");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let blocked = kaseifu(&dir, &["inspect", "/nonexistent-kaseifu-completion-test"]);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod back");

    assert_eq!(control.status.code(), blocked.status.code());
    assert_eq!(stdout_of(&control), stdout_of(&blocked));
    assert_eq!(stderr_of(&control), stderr_of(&blocked));
    assert_eq!(
        std::fs::read(&path).expect("read"),
        before,
        "a read-only directory must stay as it is"
    );
}

#[test]
fn completion_refresh_plan_is_injectable() {
    let dir = temp_dir("plan");
    let path = write_file(
        &dir,
        completion::FILE_NAME,
        "#compdef kaseifu\n# kaseifu completions v0.0.1\n",
    );
    let mut cmd = Synthetic::command();

    completion::refresh(
        &mut cmd,
        &RefreshPlan {
            candidates: vec![dir.clone()],
            enabled: false,
        },
    );
    assert!(
        std::fs::read_to_string(&path)
            .expect("read")
            .contains("v0.0.1")
    );

    completion::refresh(
        &mut cmd,
        &RefreshPlan {
            candidates: vec![dir.clone()],
            enabled: true,
        },
    );
    assert_eq!(completion::status_in(&dir), Status::Installed);
}

#[test]
fn completion_refresh_never_replaces_a_user_managed_symlink() {
    let dir = temp_dir("refresh-symlink");
    let real = temp_dir("refresh-symlink-target");
    let target = write_file(
        &real,
        completion::FILE_NAME,
        "#compdef kaseifu\n# kaseifu completions v0.0.1\n",
    );
    std::os::unix::fs::symlink(&target, dir.join(completion::FILE_NAME)).expect("symlink");

    let mut cmd = Synthetic::command();
    completion::refresh(
        &mut cmd,
        &RefreshPlan {
            candidates: vec![dir.clone()],
            enabled: true,
        },
    );

    let link = dir.join(completion::FILE_NAME);
    assert!(
        link.symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink()),
        "the link must stay a link"
    );
    let contents = std::fs::read_to_string(&target).expect("read");
    assert!(contents.contains("v0.0.1"), "the linked file is left alone");
}

// ---------------------------------------------------------------------------
// Doctor + real zsh (T9 / AC 5-6)
// ---------------------------------------------------------------------------

#[test]
fn completion_doctor_reports_the_state() {
    let dir = temp_dir("doctor");
    let missing = child(&dir, &["doctor"], &[("JEV_API_KEY", "completion-test-key")]);
    assert!(stdout_of(&missing).contains("Completion"));
    assert!(stdout_of(&missing).contains("missing"));

    let install = kaseifu(&dir, &["completions", "zsh", "--install"]);
    assert!(install.status.success());
    let installed = child(&dir, &["doctor"], &[("JEV_API_KEY", "completion-test-key")]);
    assert!(stdout_of(&installed).contains("installed"));
    let json = child(
        &dir,
        &["--format", "json", "doctor"],
        &[("JEV_API_KEY", "completion-test-key")],
    );
    assert!(stdout_of(&json).contains("\"name\":\"Completion\""));
}

#[test]
fn completion_installed_file_is_registered_by_compinit() {
    let Some(zsh) = zsh() else {
        eprintln!("skipping: zsh is not installed");
        return;
    };
    let dir = temp_dir("compinit");
    let install = kaseifu(&dir, &["completions", "zsh", "--install"]);
    assert!(install.status.success());
    let functions = "/usr/share/zsh/5.9/functions";
    let script = format!("fpath=({} {} $fpath)\n", dir.display(), functions)
        + "autoload -Uz compinit\n"
        + "compinit -u -D\n"
        + "if (( $+_comps[kaseifu] )); then\n"
        + "  print REGISTERED ${_comps[kaseifu]}\n"
        + "else\n"
        + "  print NOT-REGISTERED\n"
        + "fi\n";
    let output = ProcessCommand::new(&zsh)
        .args(["-f", "-c", &script])
        .output()
        .expect("zsh runs");
    let text = format!("{}{}", stdout_of(&output), stderr_of(&output));
    assert!(
        text.contains("REGISTERED _kaseifu"),
        "compinit must register the installed file: {text}"
    );
}

//! v107 rename response language (`config set language <tag>`).
//!
//! The default rename language is English (`en`); users persist a choice with
//! exactly `kaseifu config set language <tag>` in the existing user TOML.
//! There is no `auth language`, no new config file, and no model involved
//! here: the CLI children only exercise persistence, validation, and
//! `config show` exposure. The wire assertion (default `en`, override `ja`)
//! lives in the provider unit tests and `v090_semantic_provider_test`.
//!
//! CLI children run with an isolated HOME in temp dirs with stdin closed and
//! a deadline.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

use kaseifu::auth::keychain::MemoryStore;
use kaseifu::config::defaults::DEFAULT_LANGUAGE;
use kaseifu::config::key_source::{KeySourceMode, KeySourceSetting};
use kaseifu::config::loader::{UserConfig, effective_language, is_language_tag};
use kaseifu::config::resolver::{CliOverrides, LocatedValue, resolve_with};

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-rename-language-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn fresh_home(case: &str) -> (PathBuf, PathBuf) {
    let dir = temp_dir(case);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("home directory");
    (dir, home)
}

fn base_command(cwd: &Path, home: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
    command
        .current_dir(cwd)
        .env("HOME", home)
        .env("KASEIFU_KEY_SOURCE", "env")
        .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
        .env_remove("JEV_API_KEY")
        .env_remove("JEV_API_URL")
        .env_remove("JEV_API_BASE_URL")
        .env_remove("JEV_MODEL")
        .env_remove("KASEIFU_DEBUG")
        .env_remove("KASEIFU_COMPLETION_DIR");
    command
}

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
                panic!("kaseifu exceeded {timeout:?}");
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    child.wait_with_output().expect("collect output")
}

fn run(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    let mut command = base_command(cwd, home);
    command.args(args);
    run_with_deadline(command, Duration::from_secs(10))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Find the child-written user TOML by walking the isolated HOME.
fn find_config(home: &Path) -> Option<PathBuf> {
    let mut found = None;
    let mut stack = vec![home.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).expect("read home tree");
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some("config.toml") {
                assert!(found.is_none(), "only one config.toml may exist");
                found = Some(path);
            }
        }
    }
    found
}

/// Every regular file under the isolated HOME, relative to it.
fn home_files(home: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack = vec![home.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).expect("read home tree");
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(
                    path.strip_prefix(home)
                        .expect("under home")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    files.sort();
    files
}

fn show_json(home: &Path, cwd: &Path) -> serde_json::Value {
    let output = run(cwd, home, &["--format", "json", "config", "show"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    serde_json::from_str(&stdout_of(&output)).expect("show JSON parses")
}

// ---------------------------------------------------------------------------
// B1: the default is English and `config show` exposes it additively
// ---------------------------------------------------------------------------

#[test]
fn v107_default_language_is_english_in_show_text_and_json() {
    assert_eq!(DEFAULT_LANGUAGE, "en");
    let (dir, home) = fresh_home("default");

    let output = run(&dir, &home, &["config", "show"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("language = en"), "{text}");

    let json = show_json(&home, &dir);
    assert_eq!(json["language"], serde_json::json!("en"));
    // Additive-only: every pre-existing key is still present.
    assert!(json.get("jev").is_some(), "{json}");
    assert!(json.get("classifier").is_some(), "{json}");
    assert!(json.get("operations").is_some(), "{json}");
    assert!(json.get("inactive_settings").is_some(), "{json}");
    assert!(json.get("metadata").is_some(), "{json}");
    assert!(json.get("api_key_configured").is_some(), "{json}");

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// B2: `config set language` persists in the existing user TOML
// ---------------------------------------------------------------------------

#[test]
fn v107_set_language_persists_and_round_trips() {
    let (dir, home) = fresh_home("round-trip");

    let output = run(&dir, &home, &["config", "set", "language", "ja"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(stdout_of(&output).contains("Updated language"));

    let config = find_config(&home).expect("user TOML was created");
    let text = std::fs::read_to_string(&config).expect("read user TOML");
    assert!(text.contains("language = \"ja\""), "{text}");
    // No new config file, database, or cache beside the user TOML.
    assert_eq!(home_files(&home).len(), 1, "{:?}", home_files(&home));

    let shown = run(&dir, &home, &["config", "show"]);
    assert!(shown.status.success(), "{}", stderr_of(&shown));
    assert!(stdout_of(&shown).contains("language = ja"));
    assert_eq!(show_json(&home, &dir)["language"], serde_json::json!("ja"));

    let output = run(&dir, &home, &["config", "set", "language", "en"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let shown = run(&dir, &home, &["config", "show"]);
    assert!(stdout_of(&shown).contains("language = en"));
    assert_eq!(show_json(&home, &dir)["language"], serde_json::json!("en"));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// B3: empty, overlong, and malformed tags are rejected without touching TOML
// ---------------------------------------------------------------------------

#[test]
fn v107_set_language_rejects_bad_tags_and_keeps_the_toml() {
    let (dir, home) = fresh_home("rejections");
    let seed = run(&dir, &home, &["config", "set", "language", "ja"]);
    assert!(seed.status.success(), "{}", stderr_of(&seed));
    let config = find_config(&home).expect("user TOML was created");
    let before = std::fs::read_to_string(&config).expect("read user TOML");

    let long = "a".repeat(17);
    let cases: Vec<String> = vec![
        String::new(),
        " ".to_string(),
        "en_us".to_string(),
        "-en".to_string(),
        "en-".to_string(),
        "en--us".to_string(),
        "e n".to_string(),
        "123".to_string(),
        "en!".to_string(),
        long,
    ];
    for case in &cases {
        // `--` routes leading-dash values past clap into the validator.
        let output = run(&dir, &home, &["config", "set", "language", "--", case]);
        assert!(!output.status.success(), "tag {case:?} must be rejected");
        assert!(
            stderr_of(&output).contains("invalid language"),
            "tag {case:?}: {}",
            stderr_of(&output)
        );
    }
    // Without `--` a leading-dash value is refused by argument parsing itself:
    // still a nonzero exit, still no TOML change.
    let raw = run(&dir, &home, &["config", "set", "language", "-en"]);
    assert!(!raw.status.success());

    let after = std::fs::read_to_string(&config).expect("read user TOML");
    assert_eq!(after, before, "rejected values leave the TOML identical");
    let shown = run(&dir, &home, &["config", "show"]);
    assert!(stdout_of(&shown).contains("language = ja"));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// B4: legacy TOML loads, unknown keys are still rejected
// ---------------------------------------------------------------------------

#[test]
fn v107_unknown_keys_still_rejected_and_legacy_toml_loads() {
    let (dir, home) = fresh_home("compat");

    let output = run(&dir, &home, &["config", "set", "storage.database", "x"]);
    assert!(!output.status.success());
    assert!(
        stderr_of(&output).contains("unknown setting"),
        "{}",
        stderr_of(&output)
    );

    // A pre-existing TOML without `language` keeps loading and means English.
    let seed = run(&dir, &home, &["config", "set", "jev.model", "jev-latest"]);
    assert!(seed.status.success(), "{}", stderr_of(&seed));
    let config = find_config(&home).expect("user TOML was created");
    std::fs::write(&config, "[jev]\nmodel = \"jev-latest\"\n").expect("legacy TOML");
    let shown = run(&dir, &home, &["config", "show"]);
    assert!(shown.status.success(), "{}", stderr_of(&shown));
    assert!(stdout_of(&shown).contains("language = en"));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// B5 (lib level): effective language mapping and tag shape
// ---------------------------------------------------------------------------

#[test]
fn v107_effective_language_falls_back_to_english() {
    assert_eq!(effective_language(&UserConfig::default()), "en");
    assert_eq!(effective_language(&config_with_language("ja")), "ja");
    assert_eq!(
        effective_language(&config_with_language(" zh-Hant ")),
        "zh-Hant"
    );

    // A hand-edited invalid tag never reaches the model as-is.
    for bad in ["", "en_us", "-en", "en-", "123", "this-tag-is-way-too-long"] {
        assert_eq!(
            effective_language(&config_with_language(bad)),
            "en",
            "tag {bad:?}"
        );
    }
}

#[test]
fn v107_language_tag_shape() {
    for good in ["en", "ja", "fr", "en-US", "zh-Hant", "pt-BR"] {
        assert!(is_language_tag(good), "{good:?} is valid");
    }
    for bad in [
        "",
        "en_us",
        "-en",
        "en-",
        "en--us",
        "e n",
        "en!",
        "123",
        "abcdefghi",
        "en-日本語",
    ] {
        assert!(!is_language_tag(bad), "{bad:?} is invalid");
    }
}

fn config_with_language(tag: &str) -> UserConfig {
    UserConfig {
        language: Some(tag.to_string()),
        ..UserConfig::default()
    }
}

#[test]
fn v107_resolved_config_carries_the_language() {
    let lookup = |_: &str| Option::<LocatedValue>::None;
    let store = MemoryStore::new();
    let mode = KeySourceSetting::explicit(KeySourceMode::Auto);

    let resolved = resolve_with(
        &CliOverrides::default(),
        &lookup,
        &UserConfig::default(),
        &store,
        mode,
    )
    .expect("resolves");
    assert_eq!(resolved.language, "en");

    let file = config_with_language("ja");
    let resolved =
        resolve_with(&CliOverrides::default(), &lookup, &file, &store, mode).expect("resolves");
    assert_eq!(resolved.language, "ja");
}

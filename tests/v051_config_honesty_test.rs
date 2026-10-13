//! v0.5.1 configuration-honesty tests (F-05/F-12).
//!
//! `config/default.toml` is documentation of the built-in defaults (nothing
//! reads it at runtime) and must mirror `src/config/defaults.rs`
//! exactly. `operations.dry_run` / `operations.enable_undo` stay accepted in
//! user TOML files (removing them would make `deny_unknown_fields` reject
//! existing files) but are inactive; `config show` and the README say so.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-config-honesty-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
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

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Isolated HOME with a config file that uses the compatibility-only keys.
fn home_with_compat_config(root: &Path) -> PathBuf {
    let home = root.join("home");
    let config = home
        .join("Library")
        .join("Application Support")
        .join("kaseifu")
        .join("config.toml");
    std::fs::create_dir_all(config.parent().expect("config parent")).expect("config dir");
    std::fs::write(
        &config,
        "[operations]\ndry_run = true\nenable_undo = true\n",
    )
    .expect("config file");
    home
}

// ---------------------------------------------------------------------------
// `config/default.toml` mirrors the implementation
// ---------------------------------------------------------------------------

#[test]
fn v051_default_toml_documents_only_settings_that_exist() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config/default.toml"))
        .expect("read config/default.toml");
    let parsed: toml::Value = toml::from_str(&text).expect("parse config/default.toml");

    for section in ["scanner", "context", "storage"] {
        assert!(
            parsed.get(section).is_none(),
            "`[{section}]` must be gone: nothing reads it (F-05)"
        );
    }

    let thresholds = &parsed["classifier"]["thresholds"];
    assert_eq!(
        thresholds["auto_move_confidence"].as_float(),
        Some(kaseifu::policy::decision::AUTO_MOVE_CONFIDENCE)
    );
    assert_eq!(
        thresholds["auto_move_margin"].as_float(),
        Some(kaseifu::policy::decision::AUTO_MOVE_MARGIN)
    );
    assert_eq!(
        thresholds["review_confidence"].as_float(),
        Some(kaseifu::policy::decision::REVIEW_CONFIDENCE)
    );

    assert_eq!(
        parsed["operations"]["dry_run"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_DRY_RUN)
    );
    assert_eq!(
        parsed["operations"]["enable_undo"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_ENABLE_UNDO)
    );

    let image = &parsed["metadata"]["image"];
    assert_eq!(
        image["enabled"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_METADATA_IMAGE_ENABLED)
    );
    assert_eq!(
        image["spotlight"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_METADATA_SPOTLIGHT)
    );
    assert_eq!(
        image["exif_fallback"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_METADATA_EXIF_FALLBACK)
    );
    let privacy = &parsed["metadata"]["privacy"];
    assert_eq!(
        privacy["send_capture_time_to_jev"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_SEND_CAPTURE_TIME_TO_JEV)
    );
    assert_eq!(
        privacy["send_gps_to_jev"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_SEND_GPS_TO_JEV)
    );
    assert_eq!(
        parsed["metadata"]["weather"]["enabled"].as_bool(),
        Some(kaseifu::config::defaults::DEFAULT_WEATHER_ENABLED)
    );
}

// ---------------------------------------------------------------------------
// Existing user files keep loading
// ---------------------------------------------------------------------------

#[test]
fn v051_existing_user_toml_with_inactive_settings_still_loads() {
    let text = "[operations]\ndry_run = true\nenable_undo = true\n";
    let config = kaseifu::config::parse_toml(text).expect("existing user files keep loading");
    assert_eq!(config.operations.dry_run, Some(true));
    assert_eq!(config.operations.enable_undo, Some(true));
}

// ---------------------------------------------------------------------------
// `config show` says the settings are inactive
// ---------------------------------------------------------------------------

#[test]
fn v051_config_show_marks_the_inactive_settings() {
    let dir = temp_dir("show");
    let home = home_with_compat_config(&dir);

    let output = run(&dir, &home, &["config", "show"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);

    assert!(
        text.contains("dry_run = true (inactive; moves are controlled by --execute)"),
        "{text}"
    );
    assert!(
        text.contains("enable_undo = true (inactive; every executed move is journaled)"),
        "{text}"
    );
}

#[test]
fn v051_config_show_json_keeps_existing_keys_and_marks_inactive() {
    let dir = temp_dir("show-json");
    let home = home_with_compat_config(&dir);

    let output = run(&dir, &home, &["--format", "json", "config", "show"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&output).trim()).expect("config show JSON");

    assert!(json["operations"]["dry_run"].is_boolean());
    assert!(json["operations"]["enable_undo"].is_boolean());
    assert_eq!(
        json["inactive_settings"],
        serde_json::json!(["operations.dry_run", "operations.enable_undo"])
    );
    // Pre-existing keys stay present.
    assert!(json["jev"]["endpoint"].is_string());
    assert!(json["classifier"]["thresholds"]["auto_move_confidence"].is_number());
}

// ---------------------------------------------------------------------------
// `config set` validation is unchanged
// ---------------------------------------------------------------------------

#[test]
fn v051_config_set_key_validation_is_unchanged() {
    let dir = temp_dir("set");
    let home = home_with_compat_config(&dir);

    let unknown = run(
        &dir,
        &home,
        &["config", "set", "storage.database", "data/x.db"],
    );
    assert!(!unknown.status.success(), "unknown keys stay rejected");
    assert!(
        stderr_of(&unknown).contains("unknown setting `storage.database`"),
        "{}",
        stderr_of(&unknown)
    );

    let secret = run(&dir, &home, &["config", "set", "jev.api_key", "abc"]);
    assert!(!secret.status.success(), "API keys stay rejected");
    assert!(
        stderr_of(&secret).contains("API key cannot be stored"),
        "{}",
        stderr_of(&secret)
    );

    // The compatibility keys are still writable.
    let accepted = run(
        &dir,
        &home,
        &["config", "set", "operations.dry_run", "true"],
    );
    assert!(accepted.status.success(), "{}", stderr_of(&accepted));
}

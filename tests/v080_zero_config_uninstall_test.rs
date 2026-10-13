//! v0.8.0 finish-batch ZC-11 tests: `kaseifu uninstall`.
//!
//! Two safety layers are pinned here:
//!
//! - library tests inject `UninstallPaths` and a recording `SecretStore`, so
//!   the plan/execute logic runs without touching the developer's completions,
//!   configuration, journal or Keychain;
//! - CLI tests spawn the real binary in a temporary working directory with an
//!   isolated HOME, `KASEIFU_COMPLETION_DIR`, `KASEIFU_KEY_SOURCE=env` and a
//!   deadline, so a regression that deletes something real or hangs fails the
//!   test instead of harming the machine.
//!
//! The production Keychain entry is never read or written: `env` mode proves
//! the non-contact path end to end, and the injected stores carry the
//! deletion path.

use kaseifu::auth::keychain::{AuthError, MemoryStore, SecretStore};
use kaseifu::config::key_source::{KeySourceMode, KeySourceSetting};
use kaseifu::storage::history::OperationHistory;
use kaseifu::uninstall::{
    self, EntryStatus, TargetAction, TargetKind, UninstallEntry, UninstallOptions, UninstallPaths,
    UninstallPlan,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Injected secret store (records every call; never touches the Keychain)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SpyStore {
    inner: MemoryStore,
    gets: AtomicUsize,
    deletes: AtomicUsize,
}

impl SpyStore {
    fn with_secret(secret: &str) -> Self {
        Self {
            inner: MemoryStore::with_secret(secret),
            ..Self::default()
        }
    }

    fn get_calls(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }

    fn delete_calls(&self) -> usize {
        self.deletes.load(Ordering::SeqCst)
    }
}

impl SecretStore for SpyStore {
    fn get(&self) -> Result<String, AuthError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get()
    }

    fn set(&self, secret: &str) -> Result<(), AuthError> {
        self.inner.set(secret)
    }

    fn delete(&self) -> Result<(), AuthError> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.inner.delete()
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kaseifu-v080-uninstall-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// Library-level fixture: explicit temporary paths only.
struct LibFixture {
    root: PathBuf,
}

impl LibFixture {
    fn new(name: &str) -> Self {
        Self {
            root: temp_dir(&format!("lib-{name}")),
        }
    }

    fn completion_dir(&self) -> PathBuf {
        let dir = self.root.join("completions");
        std::fs::create_dir_all(&dir).expect("completion dir");
        dir
    }

    fn install_completion(&self) -> PathBuf {
        let path = self.completion_dir().join("_kaseifu");
        std::fs::write(&path, "# kaseifu completions v0.7.0\n").expect("completion file");
        path
    }

    fn app_dir(&self) -> PathBuf {
        self.root.join("home/Library/Application Support/kaseifu")
    }

    fn config_path(&self) -> PathBuf {
        self.app_dir().join("config.toml")
    }

    fn journal_path(&self) -> PathBuf {
        self.app_dir().join("kaseifu.db")
    }

    fn legacy_path(&self) -> PathBuf {
        self.root.join("cwd/data/kaseifu.db")
    }

    fn install_config(&self) -> PathBuf {
        let path = self.config_path();
        std::fs::create_dir_all(path.parent().expect("config parent")).expect("config dir");
        std::fs::write(&path, "[jev]\nmodel = \"test\"\n").expect("config file");
        path
    }

    /// A real journal (empty) at `path`, created through the library.
    fn install_journal_at(&self, path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().expect("journal parent")).expect("journal dir");
        drop(OperationHistory::open(path).expect("create journal"));
        path.to_path_buf()
    }

    fn install_journal(&self) -> PathBuf {
        self.install_journal_at(&self.journal_path())
    }

    fn install_legacy_journal(&self) -> PathBuf {
        self.install_journal_at(&self.legacy_path())
    }

    fn paths(&self) -> UninstallPaths {
        UninstallPaths {
            completion_dirs: vec![self.completion_dir()],
            config: self.config_path(),
            journal: self.journal_path(),
            legacy_journal: self.legacy_path(),
        }
    }
}

fn default_options() -> UninstallOptions {
    UninstallOptions {
        execute: false,
        purge: false,
        remove_key: false,
    }
}

fn entry(plan: &UninstallPlan, kind: TargetKind) -> &UninstallEntry {
    plan.entries
        .iter()
        .find(|entry| entry.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} entry in {:?}", plan.entries))
}

// ---------------------------------------------------------------------------
// Library: the plan is read-only and the defaults match the ZC-11 table
// ---------------------------------------------------------------------------

#[test]
fn plan_lists_the_default_targets_without_touching_anything() {
    let fixture = LibFixture::new("plan-default");
    let completion = fixture.install_completion();
    let config = fixture.install_config();
    let journal = fixture.install_journal();
    let legacy = fixture.install_legacy_journal();

    let plan = uninstall::plan(&default_options(), &fixture.paths(), KeySourceMode::Env);

    let completion_entry = entry(&plan, TargetKind::Completion);
    assert_eq!(completion_entry.status, EntryStatus::WouldRemove);
    assert_eq!(completion_entry.action, TargetAction::Remove);
    assert!(completion_entry.size_bytes.unwrap_or(0) > 0);
    assert_eq!(entry(&plan, TargetKind::Config).status, EntryStatus::Kept);
    assert_eq!(entry(&plan, TargetKind::Journal).status, EntryStatus::Kept);
    let legacy_entry = entry(&plan, TargetKind::LegacyJournal);
    assert_eq!(legacy_entry.status, EntryStatus::Kept);
    assert_eq!(legacy_entry.legacy_operations, Some(0));
    assert_eq!(entry(&plan, TargetKind::Keychain).status, EntryStatus::Kept);
    assert_eq!(entry(&plan, TargetKind::Binary).status, EntryStatus::Kept);
    assert_eq!(
        entry(&plan, TargetKind::MovedFiles).status,
        EntryStatus::Kept
    );

    for path in [&completion, &config, &journal, &legacy] {
        assert!(path.exists(), "the dry-run plan deleted {}", path.display());
    }
}

#[test]
fn dry_run_report_shows_the_target_count_and_the_sizes() {
    let fixture = LibFixture::new("report-dry");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal();
    let plan = uninstall::plan(&default_options(), &fixture.paths(), KeySourceMode::Env);
    let lines = kaseifu::presentation::renderers::uninstall::report_lines(
        &plan,
        &KeySourceSetting::explicit(KeySourceMode::Env),
    );

    assert!(
        lines[0].contains("1 target(s),") && lines[0].contains("nothing was deleted"),
        "{lines:?}"
    );
    let text = lines.join("\n");
    assert!(text.contains("remove  Completion: "), "{text}");
    assert!(text.contains("keep    Config: "), "{text}");
    assert!(text.contains("keep    Journal: "), "{text}");
    assert!(
        text.contains("keep    Keychain: not touched in env mode"),
        "{text}"
    );
    assert!(text.contains("run `cargo uninstall kaseifu`"), "{text}");
    assert!(text.is_ascii(), "{text}");
}

// ---------------------------------------------------------------------------
// Library: execute
// ---------------------------------------------------------------------------

#[test]
fn execute_removes_only_the_completion_script() {
    let fixture = LibFixture::new("execute-only-completion");
    let completion = fixture.install_completion();
    let config = fixture.install_config();
    let journal = fixture.install_journal();
    let legacy = fixture.install_legacy_journal();
    let store = SpyStore::with_secret("unit-test-secret");

    let options = UninstallOptions {
        execute: true,
        purge: false,
        remove_key: false,
    };
    let mut plan = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Auto);
    let confirm = || -> anyhow::Result<bool> {
        panic!("without --remove-key/--purge the Keychain step must not ask")
    };
    uninstall::execute(&mut plan, &store, true, &confirm);

    assert!(!completion.exists(), "the completion script is removed");
    for kept in [&config, &journal, &legacy] {
        assert!(kept.exists(), "{} must stay", kept.display());
    }
    assert_eq!(
        entry(&plan, TargetKind::Completion).status,
        EntryStatus::Removed
    );
    assert_eq!(entry(&plan, TargetKind::Keychain).status, EntryStatus::Kept);
    assert_eq!(store.get_calls(), 0, "the Keychain must not be read");
    assert_eq!(store.delete_calls(), 0, "the Keychain must not be written");
}

#[test]
fn purge_execute_asks_before_the_keychain_and_declining_keeps_it() {
    let fixture = LibFixture::new("purge-declined");
    fixture.install_completion();
    let config = fixture.install_config();
    let journal = fixture.install_journal();
    let legacy = fixture.install_legacy_journal();
    let store = SpyStore::with_secret("unit-test-secret");
    let asked = AtomicUsize::new(0);

    let options = UninstallOptions {
        execute: true,
        purge: true,
        remove_key: false,
    };
    let mut plan = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Auto);
    let confirm = || -> anyhow::Result<bool> {
        asked.fetch_add(1, Ordering::SeqCst);
        Ok(false)
    };
    uninstall::execute(&mut plan, &store, true, &confirm);

    assert_eq!(
        entry(&plan, TargetKind::Keychain).status,
        EntryStatus::Cancelled
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1, "the confirmation is asked");
    assert_eq!(store.delete_calls(), 0, "a declined prompt deletes nothing");
    assert!(store.get().is_ok(), "the API key stays in the store");
    for removed in [&config, &journal, &legacy] {
        assert!(
            !removed.exists(),
            "{} is removed before the Keychain step",
            removed.display()
        );
    }
}

#[test]
fn purge_execute_with_confirmation_removes_the_key_from_the_injected_store() {
    let fixture = LibFixture::new("purge-key");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal();
    fixture.install_legacy_journal();
    let store = SpyStore::with_secret("unit-test-secret");

    let options = UninstallOptions {
        execute: true,
        purge: true,
        remove_key: true,
    };
    let mut plan = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Auto);
    let confirm = || -> anyhow::Result<bool> { Ok(true) };
    uninstall::execute(&mut plan, &store, true, &confirm);

    assert_eq!(
        entry(&plan, TargetKind::Keychain).status,
        EntryStatus::Removed
    );
    assert_eq!(store.get_calls(), 1);
    assert_eq!(store.delete_calls(), 1);
    assert!(
        matches!(store.get(), Err(AuthError::NotFound)),
        "the injected key is gone"
    );
}

#[test]
fn env_mode_never_calls_the_store_and_forces_keep() {
    let fixture = LibFixture::new("env-noncontact");
    fixture.install_completion();
    let store = SpyStore::with_secret("unit-test-secret");

    let options = UninstallOptions {
        execute: true,
        purge: true,
        remove_key: true,
    };
    let mut plan = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Env);
    // `env` mode forces the Keychain row to Keep in the plan itself.
    assert_eq!(
        entry(&plan, TargetKind::Keychain).action,
        TargetAction::Keep
    );
    assert_eq!(entry(&plan, TargetKind::Keychain).status, EntryStatus::Kept);

    let confirm =
        || -> anyhow::Result<bool> { panic!("env mode must never ask about the Keychain") };
    // Both gates are exercised: the allowed flag passed by a caller cannot
    // resurrect a row the plan already keeps.
    uninstall::execute(&mut plan, &store, false, &confirm);
    assert_eq!(store.get_calls(), 0, "env mode never reads the Keychain");
    assert_eq!(store.delete_calls(), 0, "env mode never deletes from it");
    assert!(store.get().is_ok(), "the key is untouched");
}

#[test]
fn failed_completion_removal_does_not_stop_the_config_and_journal() {
    let fixture = LibFixture::new("completion-failure");
    let completion = fixture.install_completion();
    let config = fixture.install_config();
    let journal = fixture.install_journal();
    let store = SpyStore::default();

    let options = UninstallOptions {
        execute: true,
        purge: true,
        remove_key: false,
    };
    let mut plan = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Env);

    // A read-only directory makes the unlink fail: the run must report it and
    // continue with the rest (ZC-11: failures never roll back).
    let dir = completion
        .parent()
        .expect("completion parent")
        .to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("read-only dir");
    let confirm = || -> anyhow::Result<bool> { Ok(true) };
    uninstall::execute(&mut plan, &store, true, &confirm);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("restore dir");

    assert_eq!(
        entry(&plan, TargetKind::Completion).status,
        EntryStatus::Failed
    );
    assert!(entry(&plan, TargetKind::Completion).error.is_some());
    assert!(!config.exists(), "the config removal still happens");
    assert!(!journal.exists(), "the journal removal still happens");
}

#[test]
fn second_execute_run_reports_nothing_to_remove() {
    let fixture = LibFixture::new("idempotent");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal();
    fixture.install_legacy_journal();
    let store = SpyStore::default();

    let options = UninstallOptions {
        execute: true,
        purge: true,
        remove_key: false,
    };
    let confirm = || -> anyhow::Result<bool> { Ok(true) };
    let mut first = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Env);
    uninstall::execute(&mut first, &store, true, &confirm);

    let mut second = uninstall::plan(&options, &fixture.paths(), KeySourceMode::Env);
    uninstall::execute(&mut second, &store, true, &confirm);
    let lines = kaseifu::presentation::renderers::uninstall::report_lines(
        &second,
        &KeySourceSetting::explicit(KeySourceMode::Env),
    );
    assert!(
        lines[0].contains("nothing to remove"),
        "second run: {lines:?}"
    );
    assert!(!fixture.legacy_path().exists());
}

// ---------------------------------------------------------------------------
// CLI fixture (isolated HOME, completion override, deadline)
// ---------------------------------------------------------------------------

struct CliFixture {
    dir: PathBuf,
    home: PathBuf,
    completion_dir: PathBuf,
    config: PathBuf,
    journal: PathBuf,
    legacy: PathBuf,
}

impl CliFixture {
    fn new(name: &str) -> Self {
        let dir = temp_dir(&format!("cli-{name}"));
        let home = dir.join("home");
        let completion_dir = dir.join("completion");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&completion_dir).expect("completion");
        let app = home.join("Library/Application Support/kaseifu");
        let legacy = dir.join("data/kaseifu.db");
        Self {
            dir,
            home,
            completion_dir,
            config: app.join("config.toml"),
            journal: app.join("kaseifu.db"),
            legacy,
        }
    }

    fn completion_file(&self) -> PathBuf {
        self.completion_dir.join("_kaseifu")
    }

    fn install_completion(&self) {
        std::fs::write(self.completion_file(), "# kaseifu completions v0.7.0\n")
            .expect("completion file");
    }

    fn install_config(&self) {
        std::fs::create_dir_all(self.config.parent().expect("config parent")).expect("config dir");
        std::fs::write(&self.config, "[jev]\nmodel = \"test\"\n").expect("config file");
    }

    fn install_journal(&self, path: &Path) {
        std::fs::create_dir_all(path.parent().expect("journal parent")).expect("journal dir");
        drop(OperationHistory::open(path).expect("create journal"));
    }

    fn run(&self, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_kaseifu"));
        command
            .args(args)
            .current_dir(&self.dir)
            .env("HOME", &self.home)
            .env("KASEIFU_COMPLETION_DIR", &self.completion_dir)
            .env("KASEIFU_NO_COMPLETION_UPDATE", "1")
            .env("NO_COLOR", "1")
            .env_remove("KASEIFU_KEY_SOURCE")
            .env_remove("JEV_API_KEY")
            .env_remove("JEV_API_URL")
            .env_remove("JEV_API_BASE_URL")
            .env_remove("JEV_MODEL")
            .env_remove("KASEIFU_DEBUG");
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("kaseifu spawns");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match child.try_wait().expect("try_wait") {
                Some(_) => break,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let output = child.wait_with_output().expect("collect output");
                    panic!(
                        "kaseifu exceeded the deadline: {} {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        child.wait_with_output().expect("collect output")
    }
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const ENV_MODE: &[(&str, &str)] = &[("KASEIFU_KEY_SOURCE", "env")];

// ---------------------------------------------------------------------------
// CLI: ZC-11 acceptance runs
// ---------------------------------------------------------------------------

#[test]
fn cli_dry_run_lists_everything_and_deletes_nothing() {
    let fixture = CliFixture::new("dry-run");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);
    fixture.install_journal(&fixture.legacy);

    let output = fixture.run(ENV_MODE, &["uninstall"]);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("1 target(s),"), "{text}");
    assert!(
        text.contains("total (dry run; nothing was deleted)"),
        "{text}"
    );
    assert!(text.contains("remove  Completion: "), "{text}");
    assert!(
        text.contains(&fixture.completion_file().display().to_string()),
        "{text}"
    );
    assert!(text.contains("keep    Config: "), "{text}");
    assert!(text.contains("keep    Journal: "), "{text}");
    assert!(text.contains("keep    Legacy Journal: "), "{text}");
    assert!(
        text.contains("keep    Keychain: not touched in env mode"),
        "{text}"
    );
    assert!(text.contains("run `cargo uninstall kaseifu`"), "{text}");
    assert!(text.contains("Moved files"), "{text}");
    assert!(text.is_ascii(), "{text}");
    assert!(stderr_of(&output).is_empty(), "{}", stderr_of(&output));

    assert!(fixture.completion_file().exists(), "nothing is deleted");
    assert!(fixture.config.exists(), "the config stays");
    assert!(fixture.journal.exists(), "the journal stays");
    assert!(fixture.legacy.exists(), "the legacy journal stays");
}

#[test]
fn cli_execute_deletes_only_the_completion_script() {
    let fixture = CliFixture::new("execute");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);
    fixture.install_journal(&fixture.legacy);

    let output = fixture.run(ENV_MODE, &["uninstall", "--execute"]);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("removed Completion: "), "{text}");
    assert!(text.contains("removed 1 target(s)"), "{text}");
    assert!(!fixture.completion_file().exists(), "the script is gone");
    assert!(
        fixture.completion_dir.exists(),
        "the directory itself stays"
    );
    assert!(fixture.config.exists(), "the config must stay");
    assert!(fixture.journal.exists(), "the journal must stay");
    assert!(fixture.legacy.exists(), "the legacy journal must stay");
}

#[test]
fn cli_purge_execute_deletes_config_journal_and_the_cwd_legacy_file() {
    let fixture = CliFixture::new("purge");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);
    fixture.install_journal(&fixture.legacy);
    // A journal in another directory: never a target, kept by structure.
    let other_legacy = fixture.dir.join("other/data/kaseifu.db");
    fixture.install_journal(&other_legacy);
    // Files kaseifu may have moved earlier: out of scope, never touched.
    let moved = fixture.dir.join("MovedPhotos/photo.jpg");
    std::fs::create_dir_all(moved.parent().expect("moved parent")).expect("moved dir");
    std::fs::write(&moved, "not a real photo").expect("moved file");

    let output = fixture.run(ENV_MODE, &["uninstall", "--purge", "--execute"]);

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("removed Config: "), "{text}");
    assert!(text.contains("removed Journal: "), "{text}");
    assert!(text.contains("the undo history is gone"), "{text}");
    assert!(text.contains("removed Legacy Journal: "), "{text}");
    assert!(!fixture.completion_file().exists());
    assert!(!fixture.config.exists(), "purge deletes the config");
    assert!(!fixture.journal.exists(), "purge deletes the journal");
    assert!(
        !fixture.legacy.exists(),
        "the CWD legacy journal is deleted"
    );
    assert!(
        other_legacy.exists(),
        "a legacy journal in another directory is never touched"
    );
    assert!(moved.exists(), "files kaseifu moved are out of scope");
}

#[test]
fn cli_remove_key_in_env_mode_keeps_the_real_keychain_out_of_it() {
    let fixture = CliFixture::new("env-key");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);

    let output = fixture.run(
        ENV_MODE,
        &["uninstall", "--execute", "--purge", "--remove-key", "--yes"],
    );

    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(
        text.contains("keep    Keychain: not touched in env mode (KASEIFU_KEY_SOURCE=env); nothing is read or deleted"),
        "{text}"
    );
    assert!(!fixture.config.exists());
    assert!(!fixture.journal.exists());
}

#[test]
fn cli_non_tty_key_confirmation_fails_before_any_deletion() {
    let fixture = CliFixture::new("non-tty-key");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);

    // Keychain mode makes the removal real, so the confirmation is required.
    let output = fixture.run(
        &[("KASEIFU_KEY_SOURCE", "keychain")],
        &["uninstall", "--execute", "--remove-key"],
    );

    assert!(!output.status.success(), "{} ", stdout_of(&output));
    let error = stderr_of(&output);
    assert!(error.contains("needs a terminal"), "{error}");
    assert!(error.contains("--yes"), "{error}");
    assert!(fixture.completion_file().exists(), "nothing was deleted");
    assert!(fixture.config.exists(), "nothing was deleted");
    assert!(fixture.journal.exists(), "nothing was deleted");
}

#[test]
fn cli_second_run_is_idempotent() {
    let fixture = CliFixture::new("idempotent");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);
    fixture.install_journal(&fixture.legacy);

    let first = fixture.run(
        ENV_MODE,
        &["uninstall", "--execute", "--purge", "--remove-key", "--yes"],
    );
    assert!(first.status.success(), "stderr: {}", stderr_of(&first));

    let second = fixture.run(
        ENV_MODE,
        &["uninstall", "--execute", "--purge", "--remove-key", "--yes"],
    );
    assert!(second.status.success(), "stderr: {}", stderr_of(&second));
    let text = stdout_of(&second);
    assert!(text.contains("nothing to remove"), "{text}");
    assert!(!fixture.completion_file().exists());
}

#[test]
fn cli_json_report_is_structured_and_additive_only() {
    let fixture = CliFixture::new("json");
    fixture.install_completion();
    fixture.install_config();
    fixture.install_journal(&fixture.journal);
    fixture.install_journal(&fixture.legacy);

    let dry = fixture.run(ENV_MODE, &["uninstall", "--format", "json"]);
    assert!(dry.status.success(), "stderr: {}", stderr_of(&dry));
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&dry).trim()).expect("one JSON document");
    assert_eq!(json["mode"], "dry_run");
    assert_eq!(json["purge"], false);
    assert_eq!(json["key_source_mode"], "env");
    assert_eq!(json["key_source_default"], false);
    let targets = json["targets"].as_array().expect("targets");
    let completion = targets
        .iter()
        .find(|target| target["kind"] == "completion")
        .expect("completion target");
    assert_eq!(completion["action"], "remove");
    assert_eq!(completion["status"], "would_remove");
    assert!(completion["size_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(
        completion["path"]
            .as_str()
            .unwrap_or("")
            .ends_with("_kaseifu")
    );
    let keychain = targets
        .iter()
        .find(|target| target["kind"] == "keychain")
        .expect("keychain target");
    assert_eq!(keychain["status"], "kept");
    assert_eq!(keychain["service"], "com.routeflags.kaseifu.jev");
    assert_eq!(json["totals"]["remove_count"], 1);
    assert!(json["totals"]["remove_bytes"].as_u64().unwrap_or(0) > 0);

    let executed = fixture.run(
        ENV_MODE,
        &[
            "uninstall",
            "--execute",
            "--purge",
            "--remove-key",
            "--yes",
            "--format",
            "json",
        ],
    );
    assert!(
        executed.status.success(),
        "stderr: {}",
        stderr_of(&executed)
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout_of(&executed).trim()).expect("one JSON document");
    assert_eq!(json["mode"], "execute");
    assert_eq!(json["totals"]["remove_count"], 4);
    for kind in ["completion", "config", "journal", "legacy_journal"] {
        let target = json["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .find(|target| target["kind"] == kind)
            .unwrap_or_else(|| panic!("no {kind} target"));
        assert_eq!(target["status"], "removed", "{kind}: {target}");
    }
}

#[test]
fn cli_dry_run_creates_no_directories() {
    let fixture = CliFixture::new("no-side-effects");
    fixture.install_completion();

    let output = fixture.run(ENV_MODE, &["uninstall"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        !fixture
            .home
            .join("Library/Application Support/kaseifu")
            .exists(),
        "the app directory must not be created"
    );
    assert!(!fixture.dir.join("data").exists(), "no data/ directory");
}

#[test]
fn cli_help_lists_the_flags_that_work() {
    let fixture = CliFixture::new("help");
    let output = fixture.run(ENV_MODE, &["uninstall", "--help"]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("dry run"), "{text}");
    for flag in ["--execute", "--purge", "--remove-key", "--yes"] {
        assert!(text.contains(flag), "{flag} missing: {text}");
    }
    assert!(text.contains("cargo uninstall kaseifu"), "{text}");
    assert!(!text.contains("discover"), "{text}");
    assert!(text.is_ascii(), "{text}");

    let top = fixture.run(ENV_MODE, &["--help"]);
    assert!(stdout_of(&top).contains("uninstall"), "{top:?}");
}

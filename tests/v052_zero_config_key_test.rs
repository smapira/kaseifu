//! v0.5.2 zero-config key acquisition tests (ZC-1).
//!
//! The interactive entry itself needs a terminal; these tests inject the reader
//! and the secret store, so they prove the storage contract without a TTY and
//! without touching the developer's `.env` or Keychain:
//!
//! - `env` mode writes the project-local `.env` (created with 0600, other lines
//!   kept) and makes **zero** store calls,
//! - `auto`/`keychain` write the store and leave `.env` alone,
//! - an empty entry, or a failed read, stores nothing,
//! - the key value never appears in an error message, and
//! - the non-TTY guidance names the commands that need no key plus `setup`.

use kaseifu::auth::acquire::{
    DOTENV_KEY_NAME, KeySaveTarget, non_tty_guidance, prompt_and_store_key, save_key_to_dotenv,
};
use kaseifu::auth::keychain::{AuthError, SecretStore};
use kaseifu::config::key_source::KeySourceMode;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const DUMMY_KEY: &str = "test-key-not-a-secret-0123456789";

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-key-acquire-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// A store that records every call and never touches the real Keychain.
#[derive(Default)]
struct CountingStore {
    reads: AtomicUsize,
    writes: AtomicUsize,
    secret: Mutex<Option<String>>,
}

impl CountingStore {
    fn calls(&self) -> (usize, usize) {
        (
            self.reads.load(Ordering::SeqCst),
            self.writes.load(Ordering::SeqCst),
        )
    }
}

impl SecretStore for CountingStore {
    fn get(&self) -> Result<String, AuthError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.secret
            .lock()
            .expect("lock")
            .clone()
            .ok_or(AuthError::NotFound)
    }

    fn set(&self, secret: &str) -> Result<(), AuthError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        *self.secret.lock().expect("lock") = Some(secret.to_string());
        Ok(())
    }

    fn delete(&self) -> Result<(), AuthError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.secret
            .lock()
            .expect("lock")
            .take()
            .ok_or(AuthError::NotFound)?;
        Ok(())
    }
}

fn mode_of(file_mode: u32) -> u32 {
    file_mode & 0o777
}

fn dotenv_map(path: &Path) -> std::collections::HashMap<String, String> {
    dotenvy::from_path_iter(path)
        .expect("dotenv parses")
        .map(|pair| pair.expect("dotenv pair"))
        .collect()
}

// ---------------------------------------------------------------------------
// ZC-1: env mode writes .env, never the Keychain
// ---------------------------------------------------------------------------

#[test]
fn v052_env_mode_writes_dotenv_with_0600_and_zero_store_calls() {
    let dir = temp_dir("env-write");
    let dotenv = dir.join(".env");
    let store = CountingStore::default();

    let target = prompt_and_store_key(KeySourceMode::Env, &store, &dotenv, &|| {
        Ok(format!(" {DUMMY_KEY} \n"))
    })
    .expect("the key is stored");

    assert_eq!(target, KeySaveTarget::Dotenv);
    assert_eq!(dotenv_map(&dotenv)[DOTENV_KEY_NAME], DUMMY_KEY);
    assert_eq!(
        mode_of(
            std::fs::metadata(&dotenv)
                .expect("metadata")
                .permissions()
                .mode()
        ),
        0o600
    );
    assert_eq!(
        store.calls(),
        (0, 0),
        "env mode must not touch the Keychain"
    );
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        leftovers.len(),
        1,
        "no temporary file may remain: {leftovers:?}"
    );
}

#[test]
fn v052_env_mode_replaces_only_the_key_line_and_keeps_the_rest() {
    let dir = temp_dir("env-replace");
    let dotenv = dir.join(".env");
    std::fs::write(
        &dotenv,
        "# project settings\nexport JEV_API_KEY=old-key\nJEV_MODEL=jev-latest\n",
    )
    .expect("write .env");

    save_key_to_dotenv(&dotenv, DUMMY_KEY).expect("update .env");

    let text = std::fs::read_to_string(&dotenv).expect("read .env");
    assert_eq!(
        text,
        format!("# project settings\nexport {DOTENV_KEY_NAME}={DUMMY_KEY}\nJEV_MODEL=jev-latest\n"),
        "the export prefix stays, other lines are untouched"
    );
    assert!(!text.contains("old-key"), "the old value is replaced");
    assert_eq!(
        mode_of(
            std::fs::metadata(&dotenv)
                .expect("metadata")
                .permissions()
                .mode()
        ),
        0o600
    );
}

#[test]
fn v052_env_mode_round_trips_values_through_dotenvy() {
    let dir = temp_dir("env-roundtrip");
    for (name, value) in [
        ("plain", "abc123-_./:;@=+".to_string()),
        ("space", "key with spaces".to_string()),
        ("dollar", "key$with$dollars".to_string()),
        ("quote", "key\"with\"quotes".to_string()),
        ("hash", "key#with#hash".to_string()),
        ("backslash", "key\\with\\slash".to_string()),
    ] {
        let dotenv = dir.join(format!(".env-{name}"));
        save_key_to_dotenv(&dotenv, &value).expect("save key");
        assert_eq!(
            dotenv_map(&dotenv)[DOTENV_KEY_NAME],
            value,
            "the value must come back verbatim ({name})"
        );
    }
}

#[test]
fn v052_keychain_modes_write_the_store_and_leave_dotenv_alone() {
    for mode in [KeySourceMode::Auto, KeySourceMode::Keychain] {
        let dir = temp_dir("keychain-write");
        let dotenv = dir.join(".env");
        let store = CountingStore::default();

        let target = prompt_and_store_key(mode, &store, &dotenv, &|| Ok(DUMMY_KEY.to_string()))
            .expect("the key is stored");

        assert_eq!(target, KeySaveTarget::Keychain);
        let (reads, writes) = store.calls();
        assert_eq!((reads, writes), (0, 1), "one write, no read ({mode:?})");
        assert!(
            !dotenv.exists(),
            "the .env file must stay untouched ({mode:?})"
        );
    }
}

// ---------------------------------------------------------------------------
// ZC-1: nothing is stored on an empty entry or a failed read
// ---------------------------------------------------------------------------

#[test]
fn v052_empty_entry_stores_nothing() {
    for input in ["", "   \n"] {
        let dir = temp_dir("empty");
        let dotenv = dir.join(".env");
        let store = CountingStore::default();

        let error = prompt_and_store_key(KeySourceMode::Env, &store, &dotenv, &|| Ok(input.into()))
            .expect_err("an empty key is rejected");

        assert!(error.to_string().contains("must not be empty"), "{error}");
        assert!(!dotenv.exists(), "nothing is written");
        assert_eq!(store.calls(), (0, 0));
    }
}

#[test]
fn v052_failed_read_stores_nothing() {
    let dir = temp_dir("interrupted");
    let dotenv = dir.join(".env");
    let store = CountingStore::default();

    let error = prompt_and_store_key(KeySourceMode::Env, &store, &dotenv, &|| {
        anyhow::bail!("interactive input needs a terminal (stdin is not a TTY)")
    })
    .expect_err("a failed read is an error");

    assert!(error.to_string().contains("terminal"), "{error}");
    assert!(!dotenv.exists(), "an interrupted entry stores nothing");
    assert_eq!(store.calls(), (0, 0));
}

#[test]
fn v052_errors_never_contain_the_key() {
    let dir = temp_dir("error-secret");
    // A directory in the path makes the atomic rename fail after the read.
    let dotenv = dir.join(".env");
    std::fs::create_dir_all(&dotenv).expect("occupy the path");

    let error = save_key_to_dotenv(&dotenv, DUMMY_KEY).expect_err("writing must fail");

    let text = format!("{error:#}");
    assert!(
        text.contains("replace") || text.contains("Is a directory"),
        "{text}"
    );
    assert!(
        !text.contains(DUMMY_KEY),
        "the key must never appear: {text}"
    );
}

// ---------------------------------------------------------------------------
// ZC-1: non-TTY guidance
// ---------------------------------------------------------------------------

#[test]
fn v052_non_tty_guidance_names_the_keyless_commands_and_setup() {
    for mode in [
        KeySourceMode::Auto,
        KeySourceMode::Env,
        KeySourceMode::Keychain,
    ] {
        let text = non_tty_guidance(mode);
        assert!(text.contains("no API key"), "{text}");
        assert!(text.contains("kaseifu setup"), "{text}");
        assert!(text.contains("kaseifu inspect"), "{text}");
        assert!(text.contains("kaseifu doctor"), "{text}");
        assert!(text.contains("stdin is not a TTY"), "{text}");
        assert!(!text.contains("discover"), "debug-only command: {text}");
        assert!(!text.contains(DUMMY_KEY));
    }
    assert!(non_tty_guidance(KeySourceMode::Env).contains("KASEIFU_KEY_SOURCE=env"));
    assert!(non_tty_guidance(KeySourceMode::Keychain).contains("KASEIFU_KEY_SOURCE=keychain"));
}

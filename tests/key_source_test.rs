//! API key source separation (`KASEIFU_KEY_SOURCE`): `auto` / `env` / `keychain`.
//!
//! Every test injects its inputs (lookup values and the secret store), so the
//! process environment, the working directory and the real Keychain are never
//! touched. `CountingStore` records get/set/delete calls: the `env`-mode tests
//! assert zero calls, which is the guarantee that no macOS Keychain approval
//! dialog can appear in development/test runs.

use kaseifu::auth::keychain::{AuthError, MemoryStore, SecretStore};
use kaseifu::config::key_source::{ApiKeyOrigin, KeySourceMode, KeySourceSetting};
use kaseifu::config::loader::UserConfig;
use kaseifu::config::resolver::{CliOverrides, LocatedValue, resolve_with};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

const SECRET: &str = "test-secret-value-abcdef-123456";
const KEYCHAIN_SECRET: &str = "keychain-secret-value-abcdef-123456";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Secret store that records every call (get/set/delete) for zero-access proofs.
#[derive(Default)]
struct CountingStore {
    inner: MemoryStore,
    gets: AtomicUsize,
    sets: AtomicUsize,
    deletes: AtomicUsize,
}

impl CountingStore {
    fn empty() -> Self {
        Self::default()
    }

    fn with_secret(secret: &str) -> Self {
        Self {
            inner: MemoryStore::with_secret(secret),
            ..Self::default()
        }
    }

    fn gets(&self) -> usize {
        self.gets.load(Ordering::SeqCst)
    }

    fn total(&self) -> usize {
        self.gets() + self.sets.load(Ordering::SeqCst) + self.deletes.load(Ordering::SeqCst)
    }
}

impl SecretStore for CountingStore {
    fn get(&self) -> Result<String, AuthError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.inner.get()
    }

    fn set(&self, secret: &str) -> Result<(), AuthError> {
        self.sets.fetch_add(1, Ordering::SeqCst);
        self.inner.set(secret)
    }

    fn delete(&self) -> Result<(), AuthError> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.inner.delete()
    }
}

/// Store that always denies access (locked Keychain / refused prompt).
struct DeniedStore;

impl SecretStore for DeniedStore {
    fn get(&self) -> Result<String, AuthError> {
        Err(AuthError::AccessDenied)
    }

    fn set(&self, _secret: &str) -> Result<(), AuthError> {
        Err(AuthError::AccessDenied)
    }

    fn delete(&self) -> Result<(), AuthError> {
        Err(AuthError::AccessDenied)
    }
}

fn env_vars(pairs: &[(&str, &str)]) -> HashMap<String, LocatedValue> {
    located(pairs, false)
}

fn dotenv_vars(pairs: &[(&str, &str)]) -> HashMap<String, LocatedValue> {
    located(pairs, true)
}

fn located(pairs: &[(&str, &str)], from_dotenv: bool) -> HashMap<String, LocatedValue> {
    pairs
        .iter()
        .map(|(name, value)| {
            (
                (*name).to_string(),
                LocatedValue {
                    value: (*value).to_string(),
                    from_dotenv,
                },
            )
        })
        .collect()
}

fn resolve_from(
    vars: &HashMap<String, LocatedValue>,
    store: &dyn SecretStore,
    mode: KeySourceMode,
) -> kaseifu::config::ResolvedConfig {
    let lookup = |name: &str| vars.get(name).cloned();
    resolve_with(
        &CliOverrides::default(),
        &lookup,
        &UserConfig::default(),
        store,
        KeySourceSetting::explicit(mode),
    )
    .expect("resolution succeeds")
}

fn doctor_api_key_check(report: &kaseifu::doctor::DoctorReport) -> &kaseifu::doctor::CheckResult {
    report
        .checks
        .iter()
        .find(|check| check.name == "API Key")
        .expect("API key check exists")
}

// ---------------------------------------------------------------------------
// K0. Mode selection
// ---------------------------------------------------------------------------

#[test]
fn key_source_debug_build_defaults_to_env_and_release_to_auto() {
    assert_eq!(KeySourceMode::default_for_build(true), KeySourceMode::Env);
    assert_eq!(KeySourceMode::default_for_build(false), KeySourceMode::Auto);
    // This test binary is a debug build: the effective default is `env`, so
    // `cargo run` and `cargo test` never consult the Keychain by default.
    assert_eq!(KeySourceMode::build_default(), KeySourceMode::Env);
}

#[test]
fn key_source_parse_accepts_documented_values_and_trims() {
    assert_eq!(KeySourceMode::parse("auto").unwrap(), KeySourceMode::Auto);
    assert_eq!(KeySourceMode::parse(" env ").unwrap(), KeySourceMode::Env);
    assert_eq!(
        KeySourceMode::parse("KEYCHAIN").unwrap(),
        KeySourceMode::Keychain
    );

    let error = KeySourceMode::parse("xyz").unwrap_err();
    assert_eq!(error.value, "xyz");
    let message = error.to_string();
    for expected in ["KASEIFU_KEY_SOURCE", "xyz", "auto", "env", "keychain"] {
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn key_source_setting_unset_is_the_build_default_without_explicit_flag() {
    let setting = KeySourceSetting::from_raw(None).unwrap();
    assert_eq!(setting.mode, KeySourceMode::Env); // debug test binary
    assert!(!setting.explicit);
    assert_eq!(setting.label(), "env (default)");

    let explicit = KeySourceSetting::from_raw(Some("keychain")).unwrap();
    assert_eq!(explicit.mode, KeySourceMode::Keychain);
    assert!(explicit.explicit);
    assert_eq!(explicit.label(), "keychain");
}

#[test]
fn key_source_setting_empty_value_falls_back_to_the_default() {
    let empty = KeySourceSetting::from_raw(Some("   ")).unwrap();
    assert_eq!(empty.mode, KeySourceMode::Env);
    assert!(!empty.explicit);

    assert!(KeySourceSetting::from_raw(Some("not-a-mode")).is_err());
}

#[test]
fn key_source_mode_allows_the_keychain_except_in_env_mode() {
    assert!(KeySourceMode::Auto.keychain_allowed());
    assert!(KeySourceMode::Keychain.keychain_allowed());
    assert!(!KeySourceMode::Env.keychain_allowed());
    assert_eq!(KeySourceMode::label(&KeySourceMode::Auto), "auto");
    assert_eq!(KeySourceMode::label(&KeySourceMode::Env), "env");
    assert_eq!(KeySourceMode::label(&KeySourceMode::Keychain), "keychain");
}

#[test]
fn api_key_origin_labels_match_the_documented_words() {
    assert_eq!(ApiKeyOrigin::Environment.label(), "env");
    assert_eq!(ApiKeyOrigin::Dotenv.label(), ".env");
    assert_eq!(ApiKeyOrigin::Keychain.label(), "keychain");
    assert_eq!(ApiKeyOrigin::Missing.label(), "not configured");
    assert_eq!(
        ApiKeyOrigin::Unavailable("x".to_string()).label(),
        "unavailable"
    );
}

// ---------------------------------------------------------------------------
// K1. `env` mode: the Keychain is never touched
// ---------------------------------------------------------------------------

#[test]
fn env_mode_uses_the_process_environment_key_without_touching_the_store() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(
        &env_vars(&[("JEV_API_KEY", SECRET)]),
        &store,
        KeySourceMode::Env,
    );

    let key = resolved.jev.api_key.expect("environment key is used");
    assert_eq!(key.expose(), SECRET);
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Environment);
    assert!(resolved.api_key_from_env);
    assert_eq!(store.total(), 0, "env mode must not touch the store");
}

#[test]
fn env_mode_uses_the_dotenv_key() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(
        &dotenv_vars(&[("JEV_API_KEY", SECRET)]),
        &store,
        KeySourceMode::Env,
    );

    assert_eq!(resolved.jev.api_key.expect("key").expose(), SECRET);
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Dotenv);
    assert!(resolved.api_key_from_env);
    assert_eq!(store.total(), 0);
}

#[test]
fn env_mode_missing_key_is_reported_without_store_access() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(&HashMap::new(), &store, KeySourceMode::Env);

    assert!(resolved.jev.api_key.is_none());
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Missing);
    assert!(!resolved.api_key_from_env);
    assert_eq!(store.total(), 0);
}

#[test]
fn env_mode_ignores_the_keychain_even_when_a_key_is_stored() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(
        &env_vars(&[("JEV_API_KEY", "   ")]),
        &store,
        KeySourceMode::Env,
    );

    assert!(resolved.jev.api_key.is_none(), "blank env key is missing");
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Missing);
    assert_eq!(store.total(), 0, "the stored key must stay invisible");
}

// ---------------------------------------------------------------------------
// K2. `keychain` mode: environment values are ignored
// ---------------------------------------------------------------------------

#[test]
fn keychain_mode_prefers_the_store_over_environment_and_dotenv() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    for vars in [
        env_vars(&[("JEV_API_KEY", SECRET)]),
        dotenv_vars(&[("JEV_API_KEY", SECRET)]),
    ] {
        let resolved = resolve_from(&vars, &store, KeySourceMode::Keychain);
        assert_eq!(resolved.jev.api_key.expect("key").expose(), KEYCHAIN_SECRET);
        assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Keychain);
        assert!(!resolved.api_key_from_env);
    }
    assert_eq!(store.gets(), 2, "one read per resolution, no more");
}

#[test]
fn keychain_mode_without_a_stored_key_ignores_the_environment() {
    let store = CountingStore::empty();
    let resolved = resolve_from(
        &env_vars(&[("JEV_API_KEY", SECRET)]),
        &store,
        KeySourceMode::Keychain,
    );

    assert!(resolved.jev.api_key.is_none());
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Missing);
    assert_eq!(store.gets(), 1);
}

#[test]
fn keychain_mode_access_denied_keeps_the_cause() {
    let resolved = resolve_from(&HashMap::new(), &DeniedStore, KeySourceMode::Keychain);
    assert!(resolved.jev.api_key.is_none());
    match &resolved.api_key_origin {
        ApiKeyOrigin::Unavailable(message) => {
            assert!(message.contains("denied"), "{message}");
            assert!(!message.contains(SECRET));
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// K3. `auto` mode keeps the historical behavior
// ---------------------------------------------------------------------------

#[test]
fn auto_mode_prefers_the_environment_and_leaves_the_store_alone() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(
        &env_vars(&[("JEV_API_KEY", SECRET)]),
        &store,
        KeySourceMode::Auto,
    );

    assert_eq!(resolved.jev.api_key.expect("key").expose(), SECRET);
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Environment);
    assert_eq!(store.total(), 0);
}

#[test]
fn auto_mode_blank_env_key_never_falls_back_to_the_keychain() {
    // Historical behavior kept: a present-but-blank `JEV_API_KEY` blocks the
    // Keychain fallback, so a copied `.env.example` cannot open a dialog.
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(
        &env_vars(&[("JEV_API_KEY", "  ")]),
        &store,
        KeySourceMode::Auto,
    );

    assert!(resolved.jev.api_key.is_none());
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Missing);
    assert_eq!(
        store.total(),
        0,
        "a blank value must not trigger a Keychain read"
    );
}

#[test]
fn auto_mode_falls_back_to_the_keychain() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(&HashMap::new(), &store, KeySourceMode::Auto);

    assert_eq!(resolved.jev.api_key.expect("key").expose(), KEYCHAIN_SECRET);
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Keychain);
    assert_eq!(store.gets(), 1);
}

#[test]
fn auto_mode_missing_everywhere_reports_missing() {
    let store = CountingStore::empty();
    let resolved = resolve_from(&HashMap::new(), &store, KeySourceMode::Auto);

    assert!(resolved.jev.api_key.is_none());
    assert_eq!(resolved.api_key_origin, ApiKeyOrigin::Missing);
    assert_eq!(store.gets(), 1);
}

// ---------------------------------------------------------------------------
// K4. doctor: source display and zero-access in `env` mode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn doctor_env_mode_missing_key_reports_the_mode_without_store_access() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(&HashMap::new(), &store, KeySourceMode::Env);
    let report = kaseifu::doctor::diagnose(&resolved, false).await;

    let check = doctor_api_key_check(&report);
    assert!(!check.ok);
    assert!(
        check.detail.contains("KASEIFU_KEY_SOURCE=env"),
        "{}",
        check.detail
    );
    assert!(check.detail.contains("JEV_API_KEY"), "{}", check.detail);
    let debug = format!("{:?}", report);
    assert!(!debug.contains(SECRET));
    assert_eq!(
        store.total(),
        0,
        "doctor must not touch the store in env mode"
    );
}

#[tokio::test]
async fn doctor_reports_the_dotenv_source_in_env_mode() {
    let store = CountingStore::with_secret(KEYCHAIN_SECRET);
    let resolved = resolve_from(
        &dotenv_vars(&[("JEV_API_KEY", SECRET)]),
        &store,
        KeySourceMode::Env,
    );
    let report = kaseifu::doctor::diagnose(&resolved, false).await;

    let check = doctor_api_key_check(&report);
    assert!(check.ok);
    assert_eq!(check.detail, "provided via .env");
    assert_eq!(store.total(), 0);
}

#[tokio::test]
async fn doctor_reports_every_source() {
    assert_eq!(
        doctor_api_key_check(
            &kaseifu::doctor::diagnose(
                &resolve_from(
                    &env_vars(&[("JEV_API_KEY", SECRET)]),
                    &CountingStore::empty(),
                    KeySourceMode::Env
                ),
                false,
            )
            .await
        )
        .detail,
        "provided via environment (JEV_API_KEY)"
    );
    assert_eq!(
        doctor_api_key_check(
            &kaseifu::doctor::diagnose(
                &resolve_from(
                    &HashMap::new(),
                    &CountingStore::with_secret(SECRET),
                    KeySourceMode::Keychain
                ),
                false,
            )
            .await
        )
        .detail,
        "resolved from macOS Keychain"
    );
    assert_eq!(
        doctor_api_key_check(
            &kaseifu::doctor::diagnose(
                &resolve_from(
                    &HashMap::new(),
                    &CountingStore::empty(),
                    KeySourceMode::Auto
                ),
                false,
            )
            .await
        )
        .detail,
        "no API key configured (run kaseifu setup)"
    );
}

#[tokio::test]
async fn doctor_keychain_denied_reports_the_cause() {
    let resolved = resolve_from(&HashMap::new(), &DeniedStore, KeySourceMode::Keychain);
    let report = kaseifu::doctor::diagnose(&resolved, false).await;

    let check = doctor_api_key_check(&report);
    assert!(!check.ok);
    assert!(check.detail.contains("denied"), "{}", check.detail);
}

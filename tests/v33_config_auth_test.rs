//! v3.3 - user-facing configuration and Keychain authentication.
//!
//! Resolver and loader tests inject every input (no process-environment
//! mutation, no working-directory dependence). Keychain tests use the
//! reserved `.test` service id or `MemoryStore` and never touch the
//! production entry `com.routeflags.kaseifu.jev` / `default`.

use kaseifu::auth::keychain::{
    AuthError, KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, KeychainStore, MemoryStore, SecretStore,
    TEST_KEYCHAIN_SERVICE,
};
use kaseifu::config::loader::{
    UserConfig, apply_setting, parse_toml, save_to, validate, validate_endpoint_value,
};
use kaseifu::config::resolver::{CliOverrides, resolve_with};
use kaseifu::config::{load_from, resolve_with_env};
use std::collections::HashMap;
use std::path::PathBuf;

const SECRET: &str = "test-secret-value-abcdef-123456";
const PROD_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn lookup_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn resolve_with_map(
    cli: &CliOverrides,
    vars: &HashMap<String, String>,
    file: &UserConfig,
    store: &dyn SecretStore,
) -> kaseifu::config::ResolvedConfig {
    let lookup = |name: &str| vars.get(name).cloned();
    resolve_with(cli, &lookup, file, store).expect("resolution succeeds")
}

fn empty_cli() -> CliOverrides {
    CliOverrides::default()
}

fn temp_path(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("kaseifu-v33-{}-{}", std::process::id(), name));
    dir
}

// ---------------------------------------------------------------------------
// Resolver priority (T1-T6)
// ---------------------------------------------------------------------------

#[test]
fn v33_resolver_defaults_when_nothing_is_configured() {
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[]),
        &UserConfig::default(),
        &MemoryStore::new(),
    );
    assert_eq!(resolved.jev.endpoint, PROD_ENDPOINT);
    assert_eq!(resolved.jev.model, "jev-latest");
    assert!(resolved.jev.api_key.is_none());
    assert!(!resolved.api_key_from_env);
}

#[test]
fn v33_resolver_endpoint_priority_url_over_legacy_over_file_over_default() {
    let mut file = UserConfig::default();
    file.jev.endpoint = Some("https://file.example.com".to_string());

    let only_file = resolve_with_map(&empty_cli(), &lookup_of(&[]), &file, &MemoryStore::new());
    assert_eq!(only_file.jev.endpoint, "https://file.example.com");

    let legacy = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[("JEV_API_BASE_URL", "https://legacy.example.com")]),
        &file,
        &MemoryStore::new(),
    );
    assert_eq!(legacy.jev.endpoint, "https://legacy.example.com");

    let current = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[
            ("JEV_API_URL", "https://current.example.com"),
            ("JEV_API_BASE_URL", "https://legacy.example.com"),
        ]),
        &file,
        &MemoryStore::new(),
    );
    assert_eq!(current.jev.endpoint, "https://current.example.com");
}

#[test]
fn v33_resolver_model_priority_env_over_file_over_default() {
    let mut file = UserConfig::default();
    file.jev.model = Some("file-model".to_string());

    let from_file = resolve_with_map(&empty_cli(), &lookup_of(&[]), &file, &MemoryStore::new());
    assert_eq!(from_file.jev.model, "file-model");

    let from_env = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[("JEV_MODEL", "env-model")]),
        &file,
        &MemoryStore::new(),
    );
    assert_eq!(from_env.jev.model, "env-model");
}

#[test]
fn v33_resolver_env_key_wins_without_touching_keychain() {
    let store = MemoryStore::with_secret("keychain-secret");
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[("JEV_API_KEY", "env-secret")]),
        &UserConfig::default(),
        &store,
    );
    let key = resolved.jev.api_key.expect("env key is used");
    assert_eq!(key.expose(), "env-secret");
    assert!(resolved.api_key_from_env);
}

#[test]
fn v33_resolver_keychain_key_used_when_env_is_absent() {
    let store = MemoryStore::with_secret(SECRET);
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[]),
        &UserConfig::default(),
        &store,
    );
    let key = resolved.jev.api_key.expect("keychain key is used");
    assert_eq!(key.expose(), SECRET);
    assert!(!resolved.api_key_from_env);
}

#[test]
fn v33_resolver_missing_key_is_a_shortage_not_an_error() {
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[]),
        &UserConfig::default(),
        &MemoryStore::new(),
    );
    assert!(resolved.jev.api_key.is_none());
}

#[test]
fn v33_resolver_empty_env_key_counts_as_missing() {
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[("JEV_API_KEY", "   ")]),
        &UserConfig::default(),
        &MemoryStore::new(),
    );
    assert!(resolved.jev.api_key.is_none());
    assert!(!resolved.api_key_from_env);
}

#[test]
fn v33_resolver_cli_overrides_win_over_everything() {
    let mut file = UserConfig::default();
    file.jev.endpoint = Some("https://file.example.com".to_string());
    file.jev.model = Some("file-model".to_string());
    let cli = CliOverrides {
        endpoint: Some("https://cli.example.com".to_string()),
        model: Some("cli-model".to_string()),
    };
    let vars = lookup_of(&[
        ("JEV_API_URL", "https://env.example.com"),
        ("JEV_MODEL", "env-model"),
    ]);
    let resolved = resolve_with_map(&cli, &vars, &file, &MemoryStore::new());
    assert_eq!(resolved.jev.endpoint, "https://cli.example.com");
    assert_eq!(resolved.jev.model, "cli-model");
}

#[test]
fn v33_resolver_thresholds_come_from_toml() {
    let mut file = UserConfig::default();
    file.classifier.thresholds.auto_move_confidence = Some(0.9);
    file.classifier.thresholds.auto_move_margin = Some(0.5);
    file.classifier.thresholds.review_confidence = Some(0.7);
    file.operations.dry_run = Some(false);
    file.operations.enable_undo = Some(false);
    let resolved = resolve_with_map(&empty_cli(), &lookup_of(&[]), &file, &MemoryStore::new());
    assert_eq!(resolved.auto_move_confidence, 0.9);
    assert_eq!(resolved.auto_move_margin, 0.5);
    assert_eq!(resolved.review_confidence, 0.7);
    assert!(!resolved.dry_run);
    assert!(!resolved.enable_undo);
}

// ---------------------------------------------------------------------------
// Secret hygiene (T4/T11)
// ---------------------------------------------------------------------------

#[test]
fn v33_api_key_debug_output_is_redacted() {
    let store = MemoryStore::with_secret(SECRET);
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[]),
        &UserConfig::default(),
        &store,
    );
    let debug = format!("{:?}", resolved.jev);
    assert!(!debug.contains(SECRET), "debug leaked the key: {debug}");
    assert!(debug.contains("REDACTED"));
}

#[test]
fn v33_keychain_errors_never_contain_the_key() {
    let store = MemoryStore::new();
    let error = store.get().expect_err("empty store has no key");
    assert!(matches!(error, AuthError::NotFound));
    assert!(!format!("{error}").contains(SECRET));

    let store = MemoryStore::with_secret(SECRET);
    let missing = KeychainStore::with_service("kaseifu-v33-nonexistent-service-zzz");
    let _ = (store, missing);
}

// ---------------------------------------------------------------------------
// Loader (T7-T9)
// ---------------------------------------------------------------------------

#[test]
fn v33_loader_missing_file_yields_defaults() {
    let missing = temp_path("missing-config.toml");
    let _ = std::fs::remove_file(&missing);
    let config = load_from(&missing).expect("missing file is fine");
    assert!(config.jev.endpoint.is_none());
    assert!(config.jev.model.is_none());
}

#[test]
fn v33_loader_parses_a_full_config() {
    let config = parse_toml(
        "[jev]\nendpoint = \"https://example.com/v1/systemone\"\nmodel = \"jev-latest\"\n\
         [classifier.thresholds]\nauto_move_confidence = 0.9\nauto_move_margin = 0.5\nreview_confidence = 0.7\n\
         [operations]\ndry_run = false\nenable_undo = false\n",
    )
    .expect("valid TOML parses");
    assert_eq!(
        config.jev.endpoint.as_deref(),
        Some("https://example.com/v1/systemone")
    );
    assert_eq!(config.classifier.thresholds.auto_move_confidence, Some(0.9));
    assert_eq!(config.operations.dry_run, Some(false));
}

#[test]
fn v33_loader_rejects_broken_toml() {
    assert!(parse_toml("[jev\nendpoint = ").is_err());
}

#[test]
fn v33_loader_rejects_unknown_keys() {
    assert!(parse_toml("[jev]\nendpooint = \"x\"\n").is_err());
    assert!(parse_toml("[jev]\napi_key = \"secret\"\n").is_err());
    assert!(parse_toml("[nope]\nvalue = 1\n").is_err());
}

#[test]
fn v33_loader_rejects_type_mismatch() {
    assert!(parse_toml("[operations]\ndry_run = \"yes\"\n").is_err());
    assert!(parse_toml("[classifier.thresholds]\nauto_move_confidence = \"high\"\n").is_err());
}

#[test]
fn v33_loader_rejects_out_of_range_thresholds() {
    let mut config = UserConfig::default();
    config.classifier.thresholds.auto_move_confidence = Some(1.5);
    assert!(validate(&config).is_err());
    config.classifier.thresholds.auto_move_confidence = Some(-0.1);
    assert!(validate(&config).is_err());
}

#[test]
fn v33_loader_rejects_bad_endpoint_and_empty_model() {
    assert!(validate_endpoint_value("https://example.com/v1/systemone").is_ok());
    assert!(validate_endpoint_value("http://127.0.0.1:9").is_ok());
    assert!(validate_endpoint_value("not a url").is_err());
    assert!(validate_endpoint_value("ftp://example.com").is_err());

    let mut config = UserConfig::default();
    config.jev.model = Some("   ".to_string());
    assert!(validate(&config).is_err());
}

#[test]
fn v33_loader_save_round_trip_is_atomic() {
    let dir = temp_path("save-dir");
    let path = dir.join("config.toml");
    let _ = std::fs::remove_dir_all(&dir);

    let mut config = UserConfig::default();
    config.jev.model = Some("jev-latest".to_string());
    save_to(&path, &config).expect("save succeeds");

    let text = std::fs::read_to_string(&path).expect("file exists");
    assert!(text.contains("jev-latest"));
    assert!(!text.contains("api_key"));

    let stray: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir exists")
        .filter_map(|entry| entry.ok().map(|e| e.file_name()))
        .filter(|name| name.to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(stray.is_empty(), "no temp files left behind");

    let reloaded = load_from(&path).expect("saved file reloads");
    assert_eq!(reloaded.jev.model.as_deref(), Some("jev-latest"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v33_loader_save_validates_before_writing() {
    let dir = temp_path("save-invalid-dir");
    let path = dir.join("config.toml");
    let _ = std::fs::remove_dir_all(&dir);

    let mut config = UserConfig::default();
    config.jev.endpoint = Some("not a url".to_string());
    assert!(save_to(&path, &config).is_err());
    assert!(!path.exists(), "failed save leaves no file");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// config set (T15)
// ---------------------------------------------------------------------------

#[test]
fn v33_config_set_accepts_supported_keys() {
    let mut config = UserConfig::default();
    apply_setting(&mut config, "jev.endpoint", "https://example.com").unwrap();
    apply_setting(&mut config, "jev.model", "jev-latest").unwrap();
    apply_setting(
        &mut config,
        "classifier.thresholds.auto_move_confidence",
        "0.9",
    )
    .unwrap();
    apply_setting(&mut config, "classifier.thresholds.auto_move_margin", "0.5").unwrap();
    apply_setting(
        &mut config,
        "classifier.thresholds.review_confidence",
        "0.7",
    )
    .unwrap();
    apply_setting(&mut config, "operations.dry_run", "false").unwrap();
    apply_setting(&mut config, "operations.enable_undo", "true").unwrap();
    validate(&config).expect("applied values are valid");
    assert_eq!(config.operations.dry_run, Some(false));
}

#[test]
fn v33_config_set_rejects_unknown_and_secret_keys() {
    let mut config = UserConfig::default();
    assert!(apply_setting(&mut config, "jev.color", "red").is_err());
    assert!(apply_setting(&mut config, "jev.api_key", SECRET).is_err());
    assert!(apply_setting(&mut config, "api_key", SECRET).is_err());
    assert!(apply_setting(&mut config, "jev.api-key", SECRET).is_err());
    assert!(apply_setting(&mut config, "JEV_API_KEY", SECRET).is_err());
}

#[test]
fn v33_config_set_rejects_bad_values() {
    let mut config = UserConfig::default();
    assert!(apply_setting(&mut config, "jev.endpoint", "not a url").is_err());
    assert!(apply_setting(&mut config, "jev.model", "   ").is_err());
    assert!(
        apply_setting(
            &mut config,
            "classifier.thresholds.auto_move_confidence",
            "2"
        )
        .is_err()
    );
    assert!(
        apply_setting(
            &mut config,
            "classifier.thresholds.auto_move_confidence",
            "high"
        )
        .is_err()
    );
    assert!(apply_setting(&mut config, "operations.dry_run", "maybe").is_err());
}

#[test]
fn v33_config_set_failure_keeps_old_values() {
    let mut config = UserConfig::default();
    config.jev.model = Some("kept".to_string());
    assert!(apply_setting(&mut config, "jev.endpoint", "not a url").is_err());
    assert_eq!(config.jev.model.as_deref(), Some("kept"));
    assert!(config.jev.endpoint.is_none());
}

// ---------------------------------------------------------------------------
// MemoryStore Keychain contract (T10-T11)
// ---------------------------------------------------------------------------

#[test]
fn v33_memory_store_round_trip() {
    let store = MemoryStore::new();
    assert!(matches!(store.get(), Err(AuthError::NotFound)));
    assert!(matches!(store.delete(), Err(AuthError::NotFound)));

    store.set(SECRET).expect("set succeeds");
    assert_eq!(store.get().expect("get succeeds"), SECRET);

    store.set("rotated-secret").expect("update succeeds");
    assert_eq!(store.get().expect("get succeeds"), "rotated-secret");

    store.delete().expect("delete succeeds");
    assert!(matches!(store.get(), Err(AuthError::NotFound)));
}

#[test]
fn v33_memory_store_rejects_empty_secrets() {
    let store = MemoryStore::new();
    assert!(matches!(store.set(""), Err(AuthError::EmptySecret)));
    assert!(matches!(store.set("   "), Err(AuthError::EmptySecret)));
    assert!(matches!(store.get(), Err(AuthError::NotFound)));
}

// ---------------------------------------------------------------------------
// Real Keychain on the reserved test service id (T10)
// ---------------------------------------------------------------------------

#[test]
fn v33_keychain_test_service_round_trip() {
    let store = KeychainStore::with_service(TEST_KEYCHAIN_SERVICE);
    let _ = store.delete();

    assert!(matches!(store.get(), Err(AuthError::NotFound)));
    store.set(SECRET).expect("keychain set succeeds");
    assert_eq!(store.get().expect("keychain get succeeds"), SECRET);
    store
        .set("rotated-secret")
        .expect("keychain update succeeds");
    assert_eq!(
        store.get().expect("keychain get succeeds"),
        "rotated-secret"
    );
    store.delete().expect("keychain delete succeeds");
    assert!(matches!(store.get(), Err(AuthError::NotFound)));
}

#[test]
fn v33_keychain_service_ids_do_not_collide() {
    assert_ne!(KEYCHAIN_SERVICE, TEST_KEYCHAIN_SERVICE);
    assert_eq!(KEYCHAIN_SERVICE, "com.routeflags.kaseifu.jev");
    assert_eq!(KEYCHAIN_ACCOUNT, "default");
}

// ---------------------------------------------------------------------------
// Doctor (T16)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v33_doctor_local_checks_without_network() {
    let store = MemoryStore::new();
    let file = UserConfig::default();
    let resolved = resolve_with_env(&empty_cli(), &file, &store).expect("resolves");
    let report = kaseifu::doctor::diagnose(&resolved, &store, false).await;

    assert!(matches!(
        report.connectivity,
        kaseifu::doctor::Connectivity::NotTested
    ));
    assert_eq!(report.checks.len(), 6);
    let keychain = report
        .checks
        .iter()
        .find(|check| check.name == "Keychain")
        .expect("keychain check exists");
    assert!(!keychain.ok, "missing key must fail the keychain check");
    let debug = format!("{:?}", report);
    assert!(!debug.contains(SECRET));
}

#[tokio::test]
async fn v33_doctor_keychain_check_passes_with_a_key() {
    let store = MemoryStore::with_secret(SECRET);
    let file = UserConfig::default();
    let resolved = resolve_with_env(&empty_cli(), &file, &store).expect("resolves");
    let report = kaseifu::doctor::diagnose(&resolved, &store, false).await;
    let keychain = report
        .checks
        .iter()
        .find(|check| check.name == "Keychain")
        .expect("keychain check exists");
    assert!(keychain.ok);
}

#[tokio::test]
async fn v33_doctor_endpoint_check_catches_bad_urls() {
    let store = MemoryStore::new();
    let mut file = UserConfig::default();
    file.jev.endpoint = Some("not a url".to_string());
    let resolved = resolve_with_env(&empty_cli(), &file, &store).expect("resolves");
    let report = kaseifu::doctor::diagnose(&resolved, &store, false).await;
    let endpoint = report
        .checks
        .iter()
        .find(|check| check.name == "Jev Endpoint")
        .expect("endpoint check exists");
    assert!(!endpoint.ok);
}

// ---------------------------------------------------------------------------
// Jev adapter from resolved config (T17)
// ---------------------------------------------------------------------------

#[test]
fn v33_jev_adapter_builds_from_resolved_config() {
    let store = MemoryStore::with_secret(SECRET);
    let resolved = resolve_with_map(
        &empty_cli(),
        &lookup_of(&[]),
        &UserConfig::default(),
        &store,
    );
    let adapter = kaseifu::classifier::jev::RealJevIntegration::from_resolved(&resolved.jev);
    assert_eq!(adapter.endpoint(), PROD_ENDPOINT);
    assert!(adapter.has_api_key());
}

// ---------------------------------------------------------------------------
// User thresholds gate the whole execution path (T17)
// ---------------------------------------------------------------------------

fn plan_decision() -> kaseifu::models::decision::JevDecision {
    kaseifu::models::decision::JevDecision {
        destination_id: Some("D1".to_string()),
        confidence: 0.9,
        margin: 0.5,
        ..Default::default()
    }
}

fn plan_candidates(dir: &std::path::Path) -> Vec<kaseifu::models::candidate::CandidateDestination> {
    vec![kaseifu::models::candidate::CandidateDestination {
        id: "D1".to_string(),
        path: dir.to_path_buf(),
        score: 1.0,
        evidence_count: 1,
        reasons: vec![],
    }]
}

fn plan_source(dir: &std::path::Path) -> PathBuf {
    let source = dir.join("note.txt");
    std::fs::write(&source, "hello").expect("write source");
    source
}

fn plan_dirs(name: &str) -> (PathBuf, PathBuf) {
    let root = temp_path(name);
    let _ = std::fs::remove_dir_all(&root);
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).expect("create dest");
    (root, dest)
}

#[test]
fn v33_user_thresholds_gate_classify_and_plan_together() {
    use kaseifu::operations::plan::{PlanError, build_move_plan, build_move_plan_with};
    use kaseifu::policy::decision::{Thresholds, evaluate_with};

    let (root, dest) = plan_dirs("plan-thresholds");
    let source = plan_source(&root);
    let decision = plan_decision();
    let candidates = plan_candidates(&dest);

    // Defaults allow this decision (0.90 conf, 0.50 margin).
    assert!(build_move_plan(&source, &decision, &candidates).is_ok());
    assert!(matches!(
        evaluate_with(&decision, Thresholds::default()),
        kaseifu::models::decision::DecisionAction::AutoMove(_)
    ));

    // Strict user thresholds refuse the same decision in both places.
    let strict = Thresholds {
        auto_move_confidence: 0.99,
        auto_move_margin: 0.90,
        review_confidence: 0.80,
    };
    assert!(matches!(
        build_move_plan_with(&source, &decision, &candidates, strict),
        Err(PlanError::NotAutoMove)
    ));
    assert!(matches!(
        evaluate_with(&decision, strict),
        kaseifu::models::decision::DecisionAction::Review(_)
    ));

    let _ = std::fs::remove_dir_all(&root);
}

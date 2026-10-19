//! v111 repository policy file (`.kaseifu.toml`) tests.
//!
//! A policy rule is explicit, committed evidence, so a hit decides the file
//! before Discovery or Jev run. Everything else keeps its existing invariant:
//! dry run by default, `--execute` only, journal plus `undo`, never-a-
//! destination rules, collision refusal, and a workspace boundary for the
//! destination. Every test builds a synthetic repository under
//! `target/v111-...` (never `$HOME` and never `std::env::temp_dir()`, whose
//! `/var` path the destination rules exclude) and removes it when it ends.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};

use kaseifu::classifier::mock_jev::MockJevIntegration;
use kaseifu::context::image_metadata::MetadataOptions;
use kaseifu::operations::organize::{self, OrganizeOutcome, OrganizeReport};
use kaseifu::operations::plan::PlanError;
use kaseifu::operations::undo;
use kaseifu::policy::decision::Thresholds;
use kaseifu::presentation::renderers::organize as organize_view;
use kaseifu::repo_policy;
use kaseifu::storage::history::OperationHistory;

// ---------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------

/// One synthetic repository tree, removed when the test ends.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("v111-repo-policy-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("sandbox root");
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Create a directory chain and return it.
    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        std::fs::create_dir_all(&path).expect("sandbox directory");
        path
    }

    /// Create a file (with its parents) and return it.
    fn file(&self, relative: &str) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("sandbox directory");
        }
        std::fs::write(&path, b"fixture").expect("sandbox file");
        path
    }

    /// Write a `.kaseifu.toml` at `directory` (relative to the sandbox).
    fn policy(&self, directory: &str, text: &str) -> PathBuf {
        let dir = self.dir(directory);
        let path = dir.join(repo_policy::POLICY_FILE_NAME);
        std::fs::write(&path, text).expect("policy file");
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A Jev that fails if anything calls it: the rule path must never ask.
fn no_classifier() -> MockJevIntegration {
    MockJevIntegration::configured_error("the classifier must not be consulted")
}

/// Run the production organize flow in a dry run.
async fn dry_run(
    file: &Path,
    history: &OperationHistory,
    jev: &MockJevIntegration,
) -> OrganizeReport {
    organize::organize_file_with_full(
        file,
        jev,
        history,
        false,
        Thresholds::default(),
        &MetadataOptions::default(),
    )
    .await
    .expect("organize runs")
}

/// Render one report as plain text (stable for pipes and tests).
fn plain(report: &OrganizeReport, file: &Path) -> String {
    organize_view::render_text_with(report, file, false, Thresholds::default(), false)
}

/// The site-owned content rule (the issue's shape).
const SITE_RULE: &str = r#"
[[rule]]
name = "site-owned-content"
match_glob = "content/{project}/**"
match_contains = "sites/thch-vape"
destination = "sites/<project>/content/"
reason = "a content project that names a site belongs to that site"
"#;

// ---------------------------------------------------------------------------
// B1-B4: finding the policy and deciding a rule hit
// ---------------------------------------------------------------------------

#[test]
fn b1_a_repository_without_a_policy_has_no_rule() {
    let sandbox = Sandbox::new("no-policy");
    let file = sandbox.file("repo/content/brief.md");

    let hit = repo_policy::evaluate(&file, Some(&sandbox.root)).expect("evaluation succeeds");
    assert!(
        hit.is_none(),
        "an empty repository keeps the classifier path"
    );
    assert_eq!(
        repo_policy::locate_root(file.parent().unwrap(), Some(&sandbox.root)),
        None
    );
}

#[test]
fn b2_the_nearest_policy_file_governs_the_file() {
    let sandbox = Sandbox::new("nested");
    sandbox.policy(
        "repo",
        r#"[[rule]]
name = "outer"
match_glob = "**"
destination = "out""#,
    );
    sandbox.policy(
        "repo/sub",
        r#"[[rule]]
name = "inner"
match_glob = "**"
destination = "in""#,
    );
    let inner_file = sandbox.file("repo/sub/content/brief.md");
    let outer_file = sandbox.file("repo/content/brief.md");

    let inner = repo_policy::evaluate(&inner_file, Some(&sandbox.root))
        .expect("evaluation succeeds")
        .expect("inner rule matches");
    assert_eq!(inner.rule, "inner");
    assert_eq!(inner.destination, sandbox.path("repo/sub/in"));

    let outer = repo_policy::evaluate(&outer_file, Some(&sandbox.root))
        .expect("evaluation succeeds")
        .expect("outer rule matches");
    assert_eq!(outer.rule, "outer");
    assert_eq!(outer.destination, sandbox.path("repo/out"));
}

#[test]
fn b3_a_policy_above_home_never_governs() {
    let sandbox = Sandbox::new("home-bound");
    sandbox.policy(
        "",
        r#"[[rule]]
name = "volume-root"
match_glob = "**"
destination = "out""#,
    );
    let home = sandbox.dir("home");
    let file = sandbox.file("home/repo/content/brief.md");

    assert!(
        repo_policy::evaluate(&file, Some(&home))
            .expect("evaluation succeeds")
            .is_none(),
        "the lookup stops at home"
    );
    assert!(
        repo_policy::evaluate(&file, None)
            .expect("evaluation succeeds")
            .is_some(),
        "without the bound the same file would be governed"
    );
}

#[test]
fn b4_glob_captures_and_contains_decide_the_rule() {
    let sandbox = Sandbox::new("match");
    sandbox.policy("repo", SITE_RULE);
    let matching = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let without_contains = sandbox.file("repo/content/cannabis-hearing/brief.md");
    let outside_glob = sandbox.file("repo/docs/cannabis-hearing/sites/thch-vape/brief.md");

    let hit = repo_policy::evaluate(&matching, Some(&sandbox.root))
        .expect("evaluation succeeds")
        .expect("the rule matches");
    assert_eq!(hit.rule, "site-owned-content");
    assert_eq!(hit.match_glob, "content/{project}/**");
    assert_eq!(
        hit.reason.as_deref(),
        Some("a content project that names a site belongs to that site")
    );
    assert_eq!(
        hit.destination,
        sandbox.path("repo/sites/cannabis-hearing/content")
    );

    for file in [&without_contains, &outside_glob] {
        assert!(
            repo_policy::evaluate(file, Some(&sandbox.root))
                .expect("evaluation succeeds")
                .is_none(),
            "{} must not be governed",
            file.display()
        );
    }
}

// ---------------------------------------------------------------------------
// B5-B8: the classify-and-move pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b5_a_rule_hit_skips_discovery_and_jev() {
    let sandbox = Sandbox::new("skip-classifier");
    sandbox.policy("repo", SITE_RULE);
    sandbox.dir("repo/sites/cannabis-hearing/content");
    let file = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    // The mock fails if it is asked, so a successful report proves the hit
    // decided the file without a model call.
    let report = dry_run(&file, &history, &no_classifier()).await;

    let rule = report.rule.as_ref().expect("the rule decided the file");
    assert_eq!(rule.rule, "site-owned-content");
    assert_eq!(
        report.decision.destination_id, None,
        "no classifier selection"
    );
    assert_eq!(report.candidate_path, None, "no discovered candidate");
    assert_eq!(
        report.outcome,
        OrganizeOutcome::WouldMove {
            destination: sandbox.path("repo/sites/cannabis-hearing/content/brief.md")
        }
    );
    assert!(file.exists(), "a dry run never moves the file");
}

#[tokio::test]
async fn b6_a_file_outside_every_rule_keeps_the_classifier_path() {
    let sandbox = Sandbox::new("classifier-path");
    sandbox.policy(
        "repo",
        r#"[[rule]]
name = "only-archive"
match_glob = "archive/**"
destination = "kept""#,
    );
    let file = sandbox.file("repo/notes/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &MockJevIntegration::select_none(0.9)).await;

    assert!(report.rule.is_none(), "the rule list did not match");
    assert_eq!(report.decision.destination_id, Some("NONE".to_string()));
    assert_eq!(
        report.policy,
        kaseifu::models::decision::DecisionAction::Leave {
            what: "no candidate selected".to_string()
        }
    );
    assert!(file.exists());
}

#[tokio::test]
async fn b7_a_dry_run_reports_without_touching_the_journal() {
    let sandbox = Sandbox::new("dry-run-journal");
    sandbox.policy("repo", SITE_RULE);
    sandbox.dir("repo/sites/cannabis-hearing/content");
    let file = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &no_classifier()).await;

    assert!(matches!(report.outcome, OrganizeOutcome::WouldMove { .. }));
    assert!(history.entries().expect("journal rows").is_empty());
    assert!(file.exists(), "dry run is the default");
}

#[tokio::test]
async fn b8_execute_moves_journals_the_rule_and_undo_restores_it() {
    let sandbox = Sandbox::new("execute");
    sandbox.policy("repo", SITE_RULE);
    let destination_dir = sandbox.dir("repo/sites/cannabis-hearing/content");
    let file = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = organize::organize_file_with_full(
        &file,
        &no_classifier(),
        &history,
        true,
        Thresholds::default(),
        &MetadataOptions::default(),
    )
    .await
    .expect("organize runs");

    let moved_to = destination_dir.join("brief.md");
    assert!(matches!(report.outcome, OrganizeOutcome::Moved { .. }));
    assert!(
        moved_to.exists(),
        "the file moved into the rule destination"
    );
    assert!(!file.exists(), "the source is gone");

    let rows = history.entries().expect("journal rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].candidate_id.as_deref(),
        Some("rule:site-owned-content")
    );
    assert_eq!(rows[0].confidence, None);
    assert_eq!(rows[0].margin, None);
    assert_eq!(rows[0].result, "success");

    let undone = undo::undo_last_move(&history).expect("undo succeeds");
    assert_eq!(undone.restored_to, file);
    assert!(file.exists(), "undo restores the original path");
    assert!(!moved_to.exists());
}

// ---------------------------------------------------------------------------
// B9-B13: what a rule may not do
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b9_a_destination_outside_the_workspace_is_reviewed() {
    let sandbox = Sandbox::new("outside-workspace");
    sandbox.policy(
        "repo",
        r#"[[rule]]
name = "publish"
match_glob = "content/**"
destination = "sites/x/content""#,
    );
    sandbox.dir("outside/content");
    let link = sandbox.path("repo/sites/x");
    std::fs::create_dir_all(link.parent().expect("parent")).expect("sites dir");
    std::os::unix::fs::symlink(sandbox.path("outside"), &link).expect("symlink");
    let file = sandbox.file("repo/content/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &no_classifier()).await;

    assert_eq!(
        report.outcome,
        OrganizeOutcome::Unplanable(PlanError::DestinationOutsideWorkspace(
            sandbox.path("outside/content")
        ))
    );
    let text = plain(&report, &file);
    assert!(text.contains("== REVIEW =="), "got:\n{text}");
    assert!(
        text.contains("destination is outside the policy workspace"),
        "got:\n{text}"
    );
    assert!(file.exists(), "a refused destination moves nothing");
}

#[tokio::test]
async fn b10_an_authoring_mistake_is_an_error_and_moves_nothing() {
    let sandbox = Sandbox::new("broken-policy");
    let policy = sandbox.policy(
        "repo",
        r#"[[rule]]
name = "escape"
match_glob = "content/*"
destination = "../elsewhere""#,
    );
    let file = sandbox.file("repo/content/brief.md");

    let error =
        repo_policy::evaluate(&file, Some(&sandbox.root)).expect_err("a broken policy is an error");
    let message = format!("{error:#}");
    assert!(message.contains(".kaseifu.toml"), "got:\n{message}");
    assert!(message.contains("rule #1"), "got:\n{message}");
    assert!(policy.exists());

    let history = OperationHistory::open_in_memory().expect("journal");
    let error = organize::organize_file_with_full(
        &file,
        &no_classifier(),
        &history,
        false,
        Thresholds::default(),
        &MetadataOptions::default(),
    )
    .await
    .expect_err("organize fails closed");
    assert!(
        error.to_string().contains("repository policy")
            || format!("{error:#}").contains("repository policy")
    );
    assert!(file.exists());
    assert!(history.entries().expect("journal rows").is_empty());
}

#[tokio::test]
async fn b11_a_never_a_destination_folder_is_refused() {
    let sandbox = Sandbox::new("excluded");
    sandbox.policy(
        "repo",
        r#"[[rule]]
name = "cache"
match_glob = "content/**"
destination = ".cache""#,
    );
    sandbox.dir("repo/.cache");
    let file = sandbox.file("repo/content/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &no_classifier()).await;

    assert!(matches!(
        report.outcome,
        OrganizeOutcome::Unplanable(PlanError::DestinationExcluded(_))
    ));
    let text = plain(&report, &file);
    assert!(
        text.contains("destination is not a usable folder"),
        "got:\n{text}"
    );
    assert!(file.exists());
}

#[tokio::test]
async fn b12_a_collision_is_reviewed() {
    let sandbox = Sandbox::new("collision");
    sandbox.policy(
        "repo",
        r#"[[rule]]
name = "publish"
match_glob = "content/**"
destination = "published""#,
    );
    sandbox.file("repo/published/brief.md");
    let file = sandbox.file("repo/content/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &no_classifier()).await;

    assert!(matches!(
        report.outcome,
        OrganizeOutcome::Unplanable(PlanError::Collision(_))
    ));
    assert!(plain(&report, &file).contains("== REVIEW =="));
    assert!(file.exists());
    assert_eq!(
        std::fs::read(sandbox.path("repo/published/brief.md")).expect("read"),
        b"fixture"
    );
}

#[tokio::test]
async fn b13_a_missing_destination_directory_is_refused() {
    let sandbox = Sandbox::new("missing-dir");
    sandbox.policy(
        "repo",
        r#"[[rule]]
name = "publish"
match_glob = "content/**"
destination = "not-created-yet""#,
    );
    let file = sandbox.file("repo/content/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &no_classifier()).await;

    assert!(matches!(
        report.outcome,
        OrganizeOutcome::Unplanable(PlanError::DestinationNotADirectory(_))
    ));
    assert!(!sandbox.path("repo/not-created-yet").exists());
    assert!(file.exists());
}

// ---------------------------------------------------------------------------
// B14-B16: read-only input, the rendered proposal, and the CLI
// ---------------------------------------------------------------------------

#[tokio::test]
async fn b14_the_policy_file_is_read_only_input() {
    let sandbox = Sandbox::new("read-only");
    let policy = sandbox.policy("repo", SITE_RULE);
    let before = std::fs::read(&policy).expect("read policy");
    sandbox.dir("repo/sites/cannabis-hearing/content");
    let file = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let _ = dry_run(&file, &history, &no_classifier()).await;
    let _ = organize::organize_file_with_full(
        &file,
        &no_classifier(),
        &history,
        true,
        Thresholds::default(),
        &MetadataOptions::default(),
    )
    .await
    .expect("organize runs");

    assert_eq!(std::fs::read(&policy).expect("read policy"), before);
    let policies: Vec<PathBuf> = walk_policies(&sandbox.root);
    assert_eq!(policies, vec![policy], "kaseifu creates no policy file");
}

/// Every `.kaseifu.toml` under `root` (the sandbox is small by construction).
fn walk_policies(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .is_some_and(|name| name == repo_policy::POLICY_FILE_NAME)
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[tokio::test]
async fn b15_the_report_names_the_rule_and_its_reason() {
    let sandbox = Sandbox::new("report");
    sandbox.policy("repo", SITE_RULE);
    sandbox.dir("repo/sites/cannabis-hearing/content");
    let file = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let report = dry_run(&file, &history, &no_classifier()).await;
    let text = plain(&report, &file);
    assert!(text.contains("== Repo policy =="), "got:\n{text}");
    assert!(text.contains("site-owned-content"), "got:\n{text}");
    assert!(text.contains("content/{project}/**"), "got:\n{text}");
    assert!(
        text.contains("a content project that names a site belongs to that site"),
        "got:\n{text}"
    );
    assert!(
        !text.contains("== Decision =="),
        "no classifier rows:\n{text}"
    );
    assert!(text.contains("dry run (no files moved)"), "got:\n{text}");
    assert!(text.is_ascii(), "output stays ASCII");

    let json = organize_view::json_report(&report, &file, false, None);
    assert_eq!(json["rule"]["name"], "site-owned-content");
    assert_eq!(json["rule"]["match_glob"], "content/{project}/**");
    assert_eq!(
        json["rule"]["reason"],
        "a content project that names a site belongs to that site"
    );
    assert_eq!(
        json["rule"]["destination"],
        sandbox
            .path("repo/sites/cannabis-hearing/content")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(json["outcome"]["type"], "would_move");
    assert_eq!(json["mode"], "dry_run");
    for key in ["source", "decision", "policy", "candidate_path", "outcome"] {
        assert!(
            json.get(key).is_some(),
            "existing key {key} stays present (values may be null)"
        );
    }
    assert!(
        json["candidate_path"].is_null(),
        "a rule hit names no discovered candidate"
    );
}

/// One command in an isolated HOME with no key available.
fn run_cli(home: &Path, cwd: &Path, args: &[&str]) -> Output {
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
        .env_remove("KASEIFU_COMPLETION_DIR")
        .stdin(Stdio::null())
        .args(args);
    command.output().expect("kaseifu runs")
}

#[test]
fn b16_the_cli_reports_a_rule_hit_without_an_api_key() {
    let sandbox = Sandbox::new("cli");
    sandbox.policy("repo", SITE_RULE);
    sandbox.dir("repo/sites/cannabis-hearing/content");
    let file = sandbox.file("repo/content/cannabis-hearing/sites/thch-vape/brief.md");
    let home = sandbox.dir("home");
    let cwd = sandbox.dir("repo");
    let file_arg = file.to_string_lossy().into_owned();

    let json_run = run_cli(&home, &cwd, &["organize", &file_arg, "--format", "json"]);
    let stderr = String::from_utf8_lossy(&json_run.stderr).into_owned();
    assert!(json_run.status.success(), "got:\n{stderr}");
    let document: serde_json::Value =
        serde_json::from_slice(&json_run.stdout).expect("one JSON document");
    assert_eq!(document["rule"]["name"], "site-owned-content");
    assert_eq!(document["outcome"]["type"], "would_move");
    assert_eq!(document["mode"], "dry_run");
    assert!(file.exists(), "the default is a dry run");

    let text_run = run_cli(&home, &cwd, &["organize", &file_arg]);
    let stdout = String::from_utf8_lossy(&text_run.stdout).into_owned();
    assert!(text_run.status.success(), "got:\n{stdout}");
    assert!(stdout.contains("site-owned-content"), "got:\n{stdout}");
    assert!(
        stdout.contains("dry run (no files moved)"),
        "got:\n{stdout}"
    );
    assert!(stdout.contains("Would move"), "got:\n{stdout}");
    assert!(stdout.is_ascii(), "output stays ASCII:\n{stdout}");
    assert!(file.exists());
}

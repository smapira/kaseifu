//! v0.11.0 - issue #11: a stale Spotlight index is never read as the
//! filesystem, and a verdict built on an index that has not caught up with
//! the file says so.
//!
//! The issue reported two things after `git mv` + `organize`:
//!
//! 1. `destination already exists` for a path the reporter believed gone.
//! 2. The classifier proposing the pre-move directory (confidence 0.950).
//!
//! These tests pin what the code actually does:
//!
//! - Destination existence is a `stat`, never the index: a hit whose path is
//!   gone is dropped by discovery, and a plan collides only when a file with
//!   that name really exists in the destination directory.
//! - Discovery reports whether Spotlight knew the target; `organize` shows
//!   that as one notice row (and one add-only JSON key) so the reported
//!   scenario is explainable from the output alone, without changing the
//!   policy, the plan gate or the execution path.
//!
//! Fixtures live under the crate's own `target/` directory (never a user
//! file; `/tmp` is not indexed by Spotlight and is excluded as a
//! destination by discovery), and the journal is in memory.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kaseifu::classifier::jev::JevResponse;
use kaseifu::classifier::mock_jev::MockJevIntegration;
use kaseifu::context::image_metadata::MetadataOptions;
use kaseifu::discovery::{DiscoveryOutcome, discover_with_outcome};
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::{DecisionAction, JevDecision};
use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};
use kaseifu::operations::organize::{OrganizeOutcome, OrganizeReport, organize_file_with_search};
use kaseifu::operations::plan::{PlanError, build_move_plan};
use kaseifu::policy::decision::Thresholds;
use kaseifu::presentation::components::path::{relative_display, strip_ansi, visible_width};
use kaseifu::presentation::renderers::organize as organize_view;
use kaseifu::storage::history::OperationHistory;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn make_fixture(tag: &str) -> Fixture {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("v110-fixture-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture root");
    Fixture { root }
}

fn write_file(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create fixture dir");
    let path = dir.join(name);
    std::fs::write(&path, b"fixture").expect("write fixture file");
    path
}

fn as_hit(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// A Markdown target context shaped like production: the file-name words ride
/// along as `F:` signals, which is what the mdfind query is built from.
fn markdown_context(path: &Path) -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: path.to_path_buf(),
            filename: path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            extension: Some("md".to_string()),
            ..Default::default()
        },
        semantic_signals: ["F:ai", "F:generated", "F:text"]
            .iter()
            .map(|term| (*term).to_string())
            .collect(),
        parent_directory: path
            .parent()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default(),
        ..Default::default()
    }
}

fn discover_hits(normalized: NormalizedFileContext, hits: Vec<String>) -> DiscoveryOutcome {
    discover_with_outcome(normalized, move |_query, _limit| Ok(hits.clone()))
        .expect("discover_with_outcome")
}

fn candidate_dirs(outcome: &DiscoveryOutcome) -> Vec<PathBuf> {
    outcome
        .candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect()
}

/// The issue's numbers: a decisive selection for the first candidate.
fn jev_selecting_d1() -> MockJevIntegration {
    let mut probabilities = HashMap::new();
    probabilities.insert("D1".to_string(), 0.950);
    probabilities.insert("D2".to_string(), 0.030);
    MockJevIntegration::configured_decision(JevResponse {
        destination_id: Some("D1".to_string()),
        ..JevResponse::default()
    })
    .set_probabilities(probabilities)
}

fn candidate(id: &str, directory: &Path) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: directory.to_path_buf(),
        score: 1.0,
        evidence_count: 1,
        reasons: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Acceptance 1: the filesystem decides, never the index
// ---------------------------------------------------------------------------

#[test]
fn v110_a_stale_hit_for_a_missing_path_never_becomes_a_candidate() {
    let fixture = make_fixture("stale-hit");
    let notes_dir = fixture.root.join("notes");
    let notes = write_file(&notes_dir, "other-ai-text.md");
    let target = write_file(
        &fixture.root.join("scripts/ai-tools"),
        "ai-generated-text.md",
    );

    // The pre-move path is what a not-yet-updated index still answers with.
    let pre_move = fixture.root.join("ai-generated-text.md");
    assert!(!pre_move.exists(), "the stale path must not exist here");

    let outcome = discover_hits(
        markdown_context(&target),
        vec![as_hit(&pre_move), as_hit(&notes)],
    );

    assert_eq!(
        candidate_dirs(&outcome),
        vec![notes_dir],
        "a hit whose path is gone is not evidence: {:?}",
        candidate_dirs(&outcome)
    );
    assert_eq!(
        outcome.target_indexed,
        Some(false),
        "the index answered but the target was not among the hits"
    );
}

#[test]
fn v110_the_index_knowing_the_target_is_reported_and_stays_out_of_its_own_evidence() {
    let fixture = make_fixture("indexed");
    let notes = write_file(&fixture.root.join("notes"), "other-ai-text.md");
    let target = write_file(
        &fixture.root.join("scripts/ai-tools"),
        "ai-generated-text.md",
    );

    let outcome = discover_hits(
        markdown_context(&target),
        vec![as_hit(&target), as_hit(&notes)],
    );

    assert_eq!(outcome.target_indexed, Some(true), "the target was indexed");
    assert!(
        !candidate_dirs(&outcome).contains(&fixture.root.join("scripts/ai-tools")),
        "the target's own directory is never a destination: {:?}",
        candidate_dirs(&outcome)
    );
}

#[test]
fn v110_no_search_terms_claim_nothing_about_the_index() {
    let fixture = make_fixture("no-terms");
    let target = write_file(&fixture.root, "ai-generated-text.md");
    let mut context = markdown_context(&target);
    context.semantic_signals = Vec::new();

    let searched = Cell::new(false);
    let outcome = discover_with_outcome(context, |_query, _limit| {
        searched.set(true);
        Ok(vec![as_hit(&target)])
    })
    .expect("discover_with_outcome");

    assert!(!searched.get(), "no terms means no search");
    assert!(outcome.candidates.is_empty());
    assert_eq!(
        outcome.target_indexed, None,
        "without a search nothing about the index can be claimed"
    );
}

#[test]
fn v110_a_plan_collides_only_when_the_file_really_exists() {
    let fixture = make_fixture("collision-stat");
    let target = write_file(
        &fixture.root.join("scripts/ai-tools"),
        "ai-generated-text.md",
    );
    let candidates = vec![candidate("D1", &fixture.root)];
    let decision = JevDecision {
        destination_id: Some("D1".to_string()),
        confidence: 0.950,
        margin: 0.920,
        ..JevDecision::default()
    };

    // Nothing at the destination: the plan is buildable, so no index entry
    // (stale or otherwise) can turn this into a collision.
    let plan = build_move_plan(&target, &decision, &candidates).expect("plan builds");
    assert_eq!(
        plan.destination_path,
        fixture.root.join("ai-generated-text.md")
    );

    // A real file there: refused, and the reported path is the one with the
    // real file (a `stat`, not a search result).
    let blocker = write_file(&fixture.root, "ai-generated-text.md");
    let error = build_move_plan(&target, &decision, &candidates).expect_err("collision");
    assert_eq!(error, PlanError::Collision(blocker.clone()));
    assert!(blocker.exists(), "the colliding path is a real file");
}

// ---------------------------------------------------------------------------
// Acceptance 2: the verdict discloses an index that has not caught up
// ---------------------------------------------------------------------------

/// The reported scenario with the filesystem state that makes the collision
/// true: the pre-move directory really holds a file of the same name, the
/// index has not seen the new location yet.
#[tokio::test]
async fn v110_organize_shows_the_collision_and_the_index_notice() {
    let fixture = make_fixture("organize-collision");
    let target = write_file(
        &fixture.root.join("scripts/ai-tools"),
        "ai-generated-text.md",
    );
    let blocker = write_file(&fixture.root, "ai-generated-text.md");
    let root_evidence = write_file(&fixture.root, "ai-generated-text-notes.md");
    let other = write_file(&fixture.root.join("notes"), "other-ai-text.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    // The index knows the pre-move directory only; the target is not there.
    let hits = vec![as_hit(&blocker), as_hit(&root_evidence), as_hit(&other)];
    let report = organize_file_with_search(
        &target,
        &jev_selecting_d1(),
        &history,
        false,
        Thresholds::default(),
        &MetadataOptions::default(),
        move |_query, _limit| Ok(hits.clone()),
    )
    .await
    .expect("organize");

    assert_eq!(report.decision.destination_id.as_deref(), Some("D1"));
    assert_eq!(
        report.policy,
        DecisionAction::AutoMove("D1".to_string()),
        "the policy still allows the move"
    );
    assert_eq!(
        report.outcome,
        OrganizeOutcome::Unplanable(PlanError::Collision(blocker.clone())),
        "the collision is the real file, found by stat"
    );
    assert_eq!(report.target_indexed, Some(false));

    let text = strip_ansi(&organize_view::render_text(&report, &target, false));
    assert!(text.contains("REVIEW"), "got:\n{text}");
    assert!(
        text.contains(&format!(
            "destination already exists: {}",
            relative_display(&blocker.to_string_lossy())
        )),
        "got:\n{text}"
    );
    assert!(
        text.contains("not indexed yet; candidates may be incomplete"),
        "the verdict says why the candidates may be incomplete:\n{text}"
    );

    // The Rich panel keeps its box aligned with the longer row present.
    let panel = strip_ansi(&organize_view::render_panel(&report, &target, false, true));
    assert!(
        panel.contains("not indexed yet; candidates may be incomplete"),
        "got:\n{panel}"
    );
    let widths: std::collections::BTreeSet<usize> = panel.lines().map(visible_width).collect();
    assert_eq!(widths.len(), 1, "panel rows stay aligned: {widths:?}");

    // A dry run with a notice still records nothing and moves nothing.
    assert!(
        history.entries().expect("journal").is_empty(),
        "no operation is recorded"
    );
    assert!(blocker.exists() && target.exists(), "both files stay");

    let json = organize_view::json_report(&report, &target, false, None);
    assert_eq!(json["target_indexed"], serde_json::json!(false));
    for key in [
        "source",
        "mode",
        "decision",
        "policy",
        "candidate_path",
        "outcome",
    ] {
        assert!(!json[key].is_null(), "existing key stays: {key}");
    }
    assert_eq!(json["outcome"]["kind"], serde_json::json!("collision"));
}

/// The same run without the file the reporter believed gone: the plan is
/// buildable, which is what proves the collision came from the filesystem and
/// not from the index.
#[tokio::test]
async fn v110_organize_plans_the_move_when_no_file_blocks_it() {
    let fixture = make_fixture("organize-planned");
    let target = write_file(
        &fixture.root.join("scripts/ai-tools"),
        "ai-generated-text.md",
    );
    let root_evidence = write_file(&fixture.root, "ai-generated-text-notes.md");
    let other = write_file(&fixture.root.join("notes"), "other-ai-text.md");
    let history = OperationHistory::open_in_memory().expect("journal");

    let hits = vec![as_hit(&root_evidence), as_hit(&other)];
    let report = organize_file_with_search(
        &target,
        &jev_selecting_d1(),
        &history,
        false,
        Thresholds::default(),
        &MetadataOptions::default(),
        move |_query, _limit| Ok(hits.clone()),
    )
    .await
    .expect("organize");

    assert_eq!(
        report.outcome,
        OrganizeOutcome::WouldMove {
            destination: fixture.root.join("ai-generated-text.md")
        },
        "no file at the destination means no collision"
    );
    assert_eq!(report.target_indexed, Some(false));

    assert!(
        history.entries().expect("journal").is_empty(),
        "a dry run records nothing"
    );
    assert!(target.exists(), "the source stays in place");
    assert!(
        !fixture.root.join("ai-generated-text.md").exists(),
        "the dry run moved nothing into the planned destination"
    );
    let text = strip_ansi(&organize_view::render_text(&report, &target, false));
    assert!(text.contains("Would move"), "got:\n{text}");
    assert!(
        text.contains("not indexed yet; candidates may be incomplete"),
        "got:\n{text}"
    );
}

/// Without an index claim (`None`) or with an indexed target there is no
/// notice: the rows a caller already sees are unchanged.
#[test]
fn v110_no_notice_without_an_index_claim() {
    let fixture = make_fixture("no-notice");
    let target = write_file(&fixture.root, "ai-generated-text.md");
    let selected = fixture.root.join("notes");

    for target_indexed in [None, Some(true)] {
        let report = OrganizeReport {
            decision: JevDecision {
                destination_id: Some("D1".to_string()),
                confidence: 0.950,
                margin: 0.920,
                ..JevDecision::default()
            },
            policy: DecisionAction::AutoMove("D1".to_string()),
            candidate_path: Some(selected.clone()),
            target_indexed,
            rule: None,
            outcome: OrganizeOutcome::WouldMove {
                destination: selected.join("ai-generated-text.md"),
            },
        };

        let text = strip_ansi(&organize_view::render_text(&report, &target, false));
        assert!(
            !text.contains("not indexed yet"),
            "target_indexed={target_indexed:?} must stay silent:\n{text}"
        );
        assert!(text.contains("Selected"), "got:\n{text}");
        assert_eq!(
            organize_view::json_report(&report, &target, false, None)["target_indexed"],
            serde_json::json!(target_indexed)
        );
    }
}

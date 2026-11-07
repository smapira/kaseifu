//! v0.13.2 issue #20: a LEAVE verdict says why nothing was proposed.
//!
//! Display-only: the reports are built by hand, so no search, no Jev call and
//! no filesystem access happen. The measured run mixed "no candidate at all"
//! (unindexed file) with "candidates below the review threshold" and printed
//! the same `[LEAVE] LEAVE` line for both.

use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::{DecisionAction, JevDecision};
use kaseifu::operations::organize::{OrganizeOutcome, OrganizeReport};
use kaseifu::operations::plan::PlanError;
use kaseifu::policy::decision::{self, Thresholds};
use kaseifu::presentation::components::path::strip_ansi;
use kaseifu::presentation::renderers::organize as organize_view;
use std::path::{Path, PathBuf};

const SOURCE: &str = "/data/inbox/IMG_2741.jpeg";
const SINK: &str = "/data/sinks/State Of Mind Labs";

fn candidate(id: &str, path: &str, score: f64) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: PathBuf::from(path),
        score,
        evidence_count: 1,
        reasons: vec!["fixture".to_string()],
    }
}

fn report(
    destination: Option<&str>,
    confidence: f64,
    candidates: Vec<CandidateDestination>,
    target_indexed: Option<bool>,
) -> OrganizeReport {
    let decision = JevDecision {
        destination_id: destination.map(str::to_string),
        confidence,
        // A clear margin keeps AUTO MOVE reachable, so a test that wants the
        // decided verdict is not silently about the margin instead.
        margin: 0.50,
        ..JevDecision::default()
    };
    let policy = decision::evaluate_with(&decision, Thresholds::default());
    let candidate_path = decision
        .destination_id
        .as_deref()
        .and_then(|id| candidates.iter().find(|candidate| candidate.id == id))
        .map(|candidate| candidate.path.clone());
    OrganizeReport {
        decision,
        policy,
        candidate_path,
        candidates,
        target_indexed,
        outcome: OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
        rule: None,
        keywords: Default::default(),
    }
}

fn text(report: &OrganizeReport) -> String {
    strip_ansi(&organize_view::render_text_with(
        report,
        Path::new(SOURCE),
        false,
        Thresholds::default(),
        false,
    ))
}

fn explained(report: &OrganizeReport) -> String {
    strip_ansi(&organize_view::render_text_with(
        report,
        Path::new(SOURCE),
        false,
        Thresholds::default(),
        true,
    ))
}

fn panel(report: &OrganizeReport) -> String {
    strip_ansi(&organize_view::render_panel_with(
        report,
        Path::new(SOURCE),
        false,
        false,
        Thresholds::default(),
        false,
    ))
}

// ---------------------------------------------------------------------------
// The two LEAVE states
// ---------------------------------------------------------------------------

#[test]
fn v117_zero_candidates_says_so() {
    // The measured first kind: an unindexed file, so Discovery found nothing.
    let report = report(Some("NONE"), 0.23, Vec::new(), Some(false));
    let out = text(&report);

    assert!(
        out.contains("no candidate destinations found; the index may not know this file yet"),
        "got:\n{out}"
    );
    assert!(
        out.contains("name a folder yourself: kaseifu organize <file> --to <dir>"),
        "a zero-candidate LEAVE needs a next step that exists:\n{out}"
    );
    assert!(
        !out.contains("--select"),
        "there is nothing to select from:\n{out}"
    );
    // The rich panel carries the same words.
    let rich = panel(&report);
    assert!(
        rich.contains("no candidate destinations found"),
        "got:\n{rich}"
    );
}

#[test]
fn v117_below_the_threshold_lists_the_candidates() {
    // The measured second kind: candidates existed, the confidence did not.
    let report = report(
        Some("D1"),
        0.34,
        vec![
            candidate("D1", SINK, 0.34),
            candidate("D2", "/data/sinks/2024-photos", 0.31),
            candidate("D3", "/data/sinks/archive", 0.29),
            candidate("D4", "/data/sinks/deeper/archive", 0.22),
        ],
        Some(true),
    );
    let out = text(&report);

    assert!(
        out.contains("below the review threshold (0.60): 4 candidates offered"),
        "the number is stated:\n{out}"
    );
    for id in ["D1", "D2", "D3"] {
        assert!(out.contains(id), "{id} must be listed:\n{out}");
    }
    assert!(
        !out.contains("D4"),
        "at most three candidates are listed:\n{out}"
    );
    assert!(
        out.contains("choose one yourself: kaseifu organize <file> --select"),
        "the next step is --select:\n{out}"
    );
    assert!(
        !out.contains("no candidate destinations found"),
        "candidates were found:\n{out}"
    );
}

#[test]
fn v117_the_two_leave_reasons_differ() {
    let zero = text(&report(Some("NONE"), 0.23, Vec::new(), Some(false)));
    let weak = text(&report(
        Some("D1"),
        0.34,
        vec![candidate("D1", SINK, 0.34)],
        Some(true),
    ));
    let zero_line = zero
        .lines()
        .find(|line| line.contains("candidate"))
        .unwrap_or_default();
    let weak_line = weak
        .lines()
        .find(|line| line.contains("review threshold"))
        .unwrap_or_default();
    assert!(!zero_line.is_empty() && !weak_line.is_empty());
    assert_ne!(zero_line, weak_line, "the two states must read differently");
}

#[test]
fn v117_a_selection_of_none_is_not_a_threshold_miss() {
    // The classifier answered NONE although candidates existed: that is not a
    // confidence miss, and saying "below the review threshold" would be wrong.
    let report = report(
        Some("NONE"),
        0.80,
        vec![candidate("D1", SINK, 0.31)],
        Some(true),
    );
    let out = text(&report);
    assert!(
        out.contains("no destination was selected; 1 candidate offered"),
        "got:\n{out}"
    );
    assert!(!out.contains("below the review threshold"), "got:\n{out}");
}

// ---------------------------------------------------------------------------
// --explain carries the evidence counts
// ---------------------------------------------------------------------------

#[test]
fn v117_explain_reports_candidates_and_index() {
    let report = report(
        Some("D1"),
        0.34,
        vec![candidate("D1", SINK, 0.34), candidate("D2", SINK, 0.30)],
        Some(false),
    );
    let out = explained(&report);
    assert!(out.contains("Candidates   2"), "got:\n{out}");
    assert!(
        out.contains("Index        not indexed yet; candidates may be incomplete"),
        "got:\n{out}"
    );

    // The plain report keeps its rows (no explanation section without --explain).
    assert!(!text(&report).contains("Candidates   "));
}

#[test]
fn v117_the_index_row_distinguishes_the_three_states() {
    let indexed = explained(&report(
        Some("D1"),
        0.34,
        vec![candidate("D1", SINK, 0.3)],
        Some(true),
    ));
    assert!(indexed.contains("Index        indexed"), "got:\n{indexed}");

    let unknown = explained(&report(
        Some("D1"),
        0.34,
        vec![candidate("D1", SINK, 0.3)],
        None,
    ));
    assert!(
        unknown.contains("Index        unknown (no search ran)"),
        "got:\n{unknown}"
    );
}

// ---------------------------------------------------------------------------
// Additive JSON, unchanged decided verdicts
// ---------------------------------------------------------------------------

#[test]
fn v117_json_stays_additive() {
    let report = report(
        Some("D1"),
        0.34,
        vec![candidate("D1", SINK, 0.34)],
        Some(false),
    );
    let explanation = organize_view::explanation_for(&report, Thresholds::default());
    let json = organize_view::json_report(&report, Path::new(SOURCE), false, Some(&explanation));

    // Historical keys keep their names and types.
    assert_eq!(json["mode"], "dry_run");
    assert!(
        json["policy"]["leave"]["what"].is_string(),
        "the policy object keeps its shape: {json}"
    );
    assert_eq!(json["decision"]["destination_id"], "D1");
    assert_eq!(json["target_indexed"], false);
    assert_eq!(json["outcome"]["type"], "unplanable");
    // Additions only.
    assert_eq!(json["explanation"]["candidate_count"], 1);
    assert_eq!(json["explanation"]["target_indexed"], false);
    assert_eq!(json["explanation"]["policy"], "leave");
    assert_eq!(json["explanation"]["thresholds"]["review_confidence"], 0.60);
}

#[test]
fn v117_a_decided_verdict_keeps_its_rows() {
    // REVIEW is decided: no LEAVE notice, and the near-miss line still appears.
    let review = report(
        Some("D1"),
        0.70,
        vec![candidate("D1", SINK, 0.70)],
        Some(true),
    );
    let out = text(&review);
    assert!(out.contains("[REVIEW]"), "got:\n{out}");
    assert!(!out.contains("review threshold ("), "got:\n{out}");
    assert!(!out.contains("no candidate destinations"), "got:\n{out}");

    let auto = report(
        Some("D1"),
        0.95,
        vec![candidate("D1", SINK, 0.95)],
        Some(true),
    );
    let out = text(&auto);
    assert!(out.contains("[AUTO MOVE]"), "got:\n{out}");
    assert!(!out.contains("no candidate destinations"), "got:\n{out}");
}

#[test]
fn v117_a_rule_hit_never_shows_a_leave_reason() {
    let mut report = report(Some("NONE"), 0.0, Vec::new(), None);
    report.policy = DecisionAction::None;
    if !matches!(report.policy, DecisionAction::Leave { .. }) {
        assert!(
            !text(&report).contains("no candidate destinations found"),
            "a non-LEAVE policy never renders the LEAVE rows"
        );
    }
}

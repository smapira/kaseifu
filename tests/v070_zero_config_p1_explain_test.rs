//! v0.7.0 P1 ZC-5 tests: near-threshold display and `--explain`.
//!
//! The renderers are pure views: these tests build decisions directly and
//! fix the near-miss wording, the EXPLANATION rows (effective thresholds),
//! the additive JSON object, and the fact that the historical entry points
//! keep their exact output.

use kaseifu::classifier::pipeline::ClassifyReport;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::{JevDecision, validate_probabilities};
use kaseifu::operations::organize::{OrganizeOutcome, OrganizeReport};
use kaseifu::operations::plan::PlanError;
use kaseifu::policy;
use kaseifu::policy::decision::Thresholds;
use kaseifu::presentation::components::path::strip_ansi;
use kaseifu::presentation::renderers::classify as classify_view;
use kaseifu::presentation::renderers::organize as organize_view;
use std::path::{Path, PathBuf};

const SINK_DIR: &str = "/data/sinks/State Of Mind Labs";

fn candidate(id: &str) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: PathBuf::from(SINK_DIR),
        score: 12.0,
        evidence_count: 4,
        reasons: vec!["exact normalized filename".to_string()],
    }
}

/// A classify report whose top probability is `confidence` and whose margin
/// is `margin` (`top - second`).
fn classify_report(confidence: f64, margin: f64) -> ClassifyReport {
    let second = (confidence - margin).max(0.0);
    let probabilities = vec![
        ("D1".to_string(), confidence),
        ("D2".to_string(), second),
        ("NONE".to_string(), 0.01),
    ];
    validate_probabilities(&probabilities).expect("valid probabilities");
    let decision = JevDecision::from_selection("D1", &probabilities);
    ClassifyReport {
        decision,
        probabilities,
        candidates: vec![candidate("D1"), candidate("D2")],
        target_filename: "IMG_2741.jpeg".to_string(),
        target_extension: Some("jpeg".to_string()),
        target_file_type: None,
        target_size_bytes: 126597,
        semantic_signals: vec!["K:JPEG".to_string()],
    }
}

fn organize_report(confidence: f64, margin: f64) -> OrganizeReport {
    let classification = classify_report(confidence, margin);
    let policy = policy::decision::evaluate(&classification.decision);
    OrganizeReport {
        decision: classification.decision,
        policy,
        candidate_path: Some(PathBuf::from(SINK_DIR)),
        target_indexed: None,
        outcome: OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
        rule: None,
    }
}

// ---------------------------------------------------------------------------
// Near-threshold display
// ---------------------------------------------------------------------------

#[test]
fn classify_panel_shows_the_near_miss_line_below_auto_move() {
    let report = classify_report(0.840, 0.500);
    let policy = policy::decision::evaluate_with(&report.decision, Thresholds::default());
    let out = strip_ansi(&classify_view::render_panel_with(
        &report,
        &policy,
        false,
        false,
        false,
        Thresholds::default(),
        false,
    ));

    assert!(
        out.contains("near auto move: 0.010 more confidence would reach AUTO MOVE"),
        "got:\n{out}"
    );
    assert!(!out.contains("EXPLANATION"), "opt-in only:\n{out}");
    // The existing verdict rows are untouched.
    assert!(out.contains("[REVIEW] REVIEW"), "got:\n{out}");
    assert!(
        out.contains("below auto-move thresholds; human review required"),
        "got:\n{out}"
    );
}

#[test]
fn near_miss_line_names_both_unmet_thresholds() {
    let report = classify_report(0.840, 0.380);
    let policy = policy::decision::evaluate_with(&report.decision, Thresholds::default());
    let out = strip_ansi(&classify_view::render_panel_with(
        &report,
        &policy,
        false,
        false,
        false,
        Thresholds::default(),
        false,
    ));
    assert!(
        out.contains(
            "near auto move: 0.010 more confidence and 0.020 more margin would reach AUTO MOVE"
        ),
        "got:\n{out}"
    );
}

#[test]
fn far_decisions_and_auto_moves_carry_no_near_line() {
    for (confidence, margin) in [(0.780, 0.500), (0.900, 0.500), (0.300, 0.050)] {
        let report = classify_report(confidence, margin);
        let policy = policy::decision::evaluate_with(&report.decision, Thresholds::default());
        let out = strip_ansi(&classify_view::render_panel_with(
            &report,
            &policy,
            false,
            false,
            false,
            Thresholds::default(),
            false,
        ));
        assert!(
            !out.contains("near auto move"),
            "{confidence}/{margin}:\n{out}"
        );
    }
}

// ---------------------------------------------------------------------------
// --explain
// ---------------------------------------------------------------------------

#[test]
fn classify_explain_section_shows_the_basis_in_one_screen() {
    let report = classify_report(0.840, 0.500);
    let policy = policy::decision::evaluate_with(&report.decision, Thresholds::default());
    let out = strip_ansi(&classify_view::render_panel_with(
        &report,
        &policy,
        false,
        false,
        false,
        Thresholds::default(),
        true,
    ));

    assert!(out.contains("|-- EXPLANATION"), "got:\n{out}");
    assert!(
        out.contains("Confidence   0.840 (auto move needs >= 0.850)"),
        "got:\n{out}"
    );
    assert!(
        out.contains("Margin       0.500 (auto move needs >= 0.400)"),
        "got:\n{out}"
    );
    assert!(
        out.contains(
            "Thresholds   auto move: confidence >= 0.850 and margin >= 0.400; review: confidence >= 0.600"
        ),
        "got:\n{out}"
    );
    assert!(out.contains("Verdict      REVIEW"), "got:\n{out}");
    assert!(
        out.contains("Destination  D1 - /data/sinks/State Of Mind Labs"),
        "got:\n{out}"
    );
}

#[test]
fn explain_uses_the_effective_thresholds_not_the_defaults() {
    let thresholds = Thresholds {
        auto_move_confidence: 0.70,
        auto_move_margin: 0.10,
        review_confidence: 0.40,
    };
    let report = classify_report(0.680, 0.090);
    let policy = policy::decision::evaluate_with(&report.decision, thresholds);
    let out = strip_ansi(&classify_view::render_panel_with(
        &report, &policy, false, false, false, thresholds, true,
    ));

    assert!(
        out.contains("Confidence   0.680 (auto move needs >= 0.700)"),
        "got:\n{out}"
    );
    assert!(
        out.contains("Margin       0.090 (auto move needs >= 0.100)"),
        "got:\n{out}"
    );
    assert!(
        out.contains("review: confidence >= 0.400"),
        "effective review threshold:\n{out}"
    );
    assert!(
        out.contains(
            "near auto move: 0.020 more confidence and 0.010 more margin would reach AUTO MOVE"
        ),
        "got:\n{out}"
    );
}

#[test]
fn historical_entry_points_keep_their_exact_output() {
    let report = classify_report(0.360, 0.130);
    let policy = policy::decision::evaluate_with(&report.decision, Thresholds::default());
    assert_eq!(
        classify_view::render_panel(&report, &policy, false, false, false),
        classify_view::render_panel_with(
            &report,
            &policy,
            false,
            false,
            false,
            Thresholds::default(),
            false
        )
    );
    let out = strip_ansi(&classify_view::render_panel(
        &report, &policy, false, false, false,
    ));
    assert!(
        !out.contains("near auto move") && !out.contains("EXPLANATION"),
        "{out}"
    );
}

// ---------------------------------------------------------------------------
// organize (plain text)
// ---------------------------------------------------------------------------

#[test]
fn organize_text_shows_near_and_explanation_and_keeps_the_old_view() {
    let report = organize_report(0.840, 0.500);

    let plain = organize_view::render_text(&report, Path::new("/data/inbox/IMG_2741.jpeg"), false);
    assert!(
        plain.contains("near auto move: 0.010 more confidence would reach AUTO MOVE"),
        "got:\n{plain}"
    );
    assert!(!plain.contains("== Explanation =="), "got:\n{plain}");

    let explained = organize_view::render_text_with(
        &report,
        Path::new("/data/inbox/IMG_2741.jpeg"),
        false,
        Thresholds::default(),
        true,
    );
    assert!(explained.contains("== Explanation =="), "got:\n{explained}");
    assert!(
        explained.contains("Confidence   0.840 (auto move needs >= 0.850)"),
        "got:\n{explained}"
    );
    assert!(
        explained.contains("Destination  D1 - /data/sinks/State Of Mind Labs"),
        "got:\n{explained}"
    );
    // Section order: Explanation sits between Policy and the result.
    let policy = explained.find("== Policy ==").expect("policy");
    let explanation = explained.find("== Explanation ==").expect("explanation");
    let result = explained.find("== No move ==").expect("result");
    assert!(policy < explanation && explanation < result, "{explained}");
    assert!(explained.is_ascii(), "{explained}");
}

// ---------------------------------------------------------------------------
// additive JSON object
// ---------------------------------------------------------------------------

#[test]
fn explanation_json_carries_the_policy_and_gaps() {
    let report = classify_report(0.840, 0.380);
    let policy = policy::decision::evaluate_with(&report.decision, Thresholds::default());
    let json =
        classify_view::decision_explanation(&report, &policy, Thresholds::default()).to_json();

    assert_eq!(json["policy"], "review");
    assert_eq!(json["destination_id"], "D1");
    assert_eq!(json["destination_path"], SINK_DIR);
    assert_eq!(json["near_auto_move"], true);
    assert_eq!(json["thresholds"]["auto_move_confidence"], 0.85);
    let gap = json["confidence_gap"].as_f64().expect("gap");
    assert!((gap - 0.01).abs() < 1e-9, "{gap}");
}

#[test]
fn decision_json_is_unchanged_without_explain() {
    // The default (no --explain) output is the decision object itself; the
    // explanation is only ever inserted on request.
    let report = classify_report(0.840, 0.380);
    let serialized = serde_json::to_string_pretty(&report.decision).expect("json");
    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("parse");
    assert!(parsed.get("explanation").is_none(), "{serialized}");
    for key in ["destination_id", "confidence", "margin", "action"] {
        assert!(parsed.get(key).is_some(), "{key} missing: {serialized}");
    }
}

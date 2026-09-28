//! Phase 4 - Jev Decision domain tests.
//!
//! These tests exercise the production domain model directly
//! (`models::decision`). There is no test-local copy of the logic.
//!
//! Invariants under test:
//! 1. Candidate membership: selection must be an existing candidate ID or NONE
//! 2. NONE is always a valid selection (no file moves)
//! 3. confidence = highest probability (P1)
//! 4. margin = P1 - P2
//! 5. Invalid probabilities never become confidence/margin

use kaseifu::models::decision::{
    DecisionAction, DecisionContext, JevDecision, confidence_and_margin, validate_probabilities,
};

fn probs(entries: &[(&str, f64)]) -> Vec<(String, f64)> {
    entries
        .iter()
        .map(|(id, probability)| ((*id).to_string(), *probability))
        .collect()
}

fn context_with(candidates: &[&str]) -> DecisionContext {
    DecisionContext::from_candidates(candidates.iter().map(|c| c.to_string()).collect())
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ---------------------------------------------------------------------------
// Candidate membership
// ---------------------------------------------------------------------------

#[test]
fn candidate_d1_is_accepted() {
    let context = context_with(&["D1"]);
    assert_eq!(context.validate_decision("D1"), Ok("D1".to_string()));
}

#[test]
fn candidate_d2_is_accepted() {
    let context = context_with(&["D1", "D2"]);
    assert_eq!(context.validate_decision("D2"), Ok("D2".to_string()));
}

#[test]
fn candidate_d6_is_accepted() {
    // Discovery may emit any D<n>; membership is not a fixed whitelist.
    let context = context_with(&["D6"]);
    assert_eq!(context.validate_decision("D6"), Ok("D6".to_string()));
}

#[test]
fn unknown_candidate_d99_is_rejected() {
    let context = context_with(&["D1", "D2"]);
    assert!(context.validate_decision("D99").is_err());
}

#[test]
fn arbitrary_filesystem_path_is_rejected() {
    // Jev may select a candidate; it may never create a destination.
    let context = context_with(&["D1", "D2"]);
    assert!(
        context.validate_decision("/Users/foo/Documents").is_err(),
        "a filesystem path must never validate as a candidate"
    );
}

#[test]
fn none_is_accepted_with_and_without_candidates() {
    assert_eq!(
        context_with(&[]).validate_decision("NONE"),
        Ok("NONE".to_string())
    );
    assert_eq!(
        context_with(&["D1"]).validate_decision("NONE"),
        Ok("NONE".to_string())
    );
}

#[test]
fn empty_selection_is_rejected() {
    let context = context_with(&["D1"]);
    assert!(context.validate_decision("").is_err());
}

// ---------------------------------------------------------------------------
// Confidence / margin
// ---------------------------------------------------------------------------

#[test]
fn confidence_and_margin_documented_example() {
    let (confidence, margin) =
        confidence_and_margin(&probs(&[("D1", 0.82), ("D2", 0.11), ("NONE", 0.07)]));
    assert!(close(confidence, 0.82), "confidence was {confidence}");
    assert!(close(margin, 0.71), "margin was {margin}");
}

#[test]
fn single_probability_has_zero_margin() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", 0.82)]));
    assert!(close(confidence, 0.82));
    assert!(close(margin, 0.0));
}

#[test]
fn tie_has_zero_margin() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", 0.50), ("D2", 0.50)]));
    assert!(close(confidence, 0.50));
    assert!(close(margin, 0.0), "a tie is not a confident decision");
}

#[test]
fn all_zero_probabilities() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", 0.0), ("D2", 0.0)]));
    assert!(close(confidence, 0.0));
    assert!(close(margin, 0.0));
}

#[test]
fn certain_probability() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", 1.0), ("NONE", 0.0)]));
    assert!(close(confidence, 1.0));
    assert!(close(margin, 1.0));
}

#[test]
fn empty_probabilities_are_zero() {
    let (confidence, margin) = confidence_and_margin(&[]);
    assert!(close(confidence, 0.0));
    assert!(close(margin, 0.0));
}

#[test]
fn nan_probability_is_ignored() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", f64::NAN), ("D2", 0.50)]));
    assert!(close(confidence, 0.50), "NaN must not become confidence");
    assert!(close(margin, 0.0));
}

#[test]
fn infinite_probability_is_ignored() {
    let (confidence, margin) =
        confidence_and_margin(&probs(&[("D1", f64::INFINITY), ("D2", 0.40)]));
    assert!(
        close(confidence, 0.40),
        "infinity must not become confidence"
    );
    assert!(close(margin, 0.0));
}

#[test]
fn negative_probability_is_ignored() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", -0.30), ("D2", 0.50)]));
    assert!(close(confidence, 0.50));
    assert!(close(margin, 0.0), "margin was {margin}");
}

#[test]
fn probability_above_one_is_ignored() {
    let (confidence, margin) = confidence_and_margin(&probs(&[("D1", 1.50), ("D2", 0.50)]));
    assert!(close(confidence, 0.50), "confidence was {confidence}");
    assert!(close(margin, 0.0), "margin was {margin}");
}

// ---------------------------------------------------------------------------
// JevDecision construction (canonical path)
// ---------------------------------------------------------------------------

#[test]
fn from_selection_uses_the_validated_choice() {
    let decision =
        JevDecision::from_selection("D2", &probs(&[("D1", 0.10), ("D2", 0.80), ("NONE", 0.10)]));

    assert_eq!(decision.destination_id, Some("D2".to_string()));
    assert_eq!(decision.action, DecisionAction::MoveTo("D2".to_string()));
    assert!(close(decision.confidence, 0.80));
    assert!(close(decision.margin, 0.70));
}

#[test]
fn from_selection_none_moves_nowhere() {
    let decision = JevDecision::from_selection("NONE", &probs(&[("NONE", 0.70), ("D1", 0.20)]));

    assert_eq!(decision.destination_id, Some("NONE".to_string()));
    assert_eq!(decision.action, DecisionAction::None);
}

// ---------------------------------------------------------------------------
// Probability validation (domain-owned; used by the pipeline)
// ---------------------------------------------------------------------------

#[test]
fn valid_probabilities_pass_validation() {
    assert!(validate_probabilities(&probs(&[("D1", 0.82), ("NONE", 0.07)])).is_ok());
    assert!(validate_probabilities(&[]).is_ok());
}

#[test]
fn nan_probability_is_rejected_by_validation() {
    assert!(validate_probabilities(&probs(&[("D1", f64::NAN)])).is_err());
}

#[test]
fn non_finite_or_out_of_range_probability_is_rejected_by_validation() {
    assert!(validate_probabilities(&probs(&[("D1", f64::INFINITY)])).is_err());
    assert!(validate_probabilities(&probs(&[("D1", -0.10)])).is_err());
    assert!(validate_probabilities(&probs(&[("D1", 1.10)])).is_err());
}

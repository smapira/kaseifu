#![cfg(target_os = "macos")]
//! Phase 4 - Full Decision Pipeline integration test.
//!
//! Protects the production chain:
//!
//! ```text
//! File
//!  -> Observe   (context::builder)
//!  -> Normalize (normalize::normalize)
//!  -> Discover  (discovery::discover_with + production aggregation)
//!  -> DecisionContext
//!  -> MockJevIntegration
//!  -> validation
//!  -> JevDecision
//! ```
//!
//! The only stubbed boundary is Spotlight (`mdfind`): a temp fixture is not
//! in the Spotlight index and `/var` temp paths are rejected by the
//! production system-path filter, so the search results are injected while
//! every other stage (including aggregating real fixture files) is
//! production code.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process;

use semantic_file_organizer::classifier::jev::JevResponse;
use semantic_file_organizer::classifier::mock_jev::MockJevIntegration;
use semantic_file_organizer::classifier::pipeline::classify_with_search;
use semantic_file_organizer::models::CandidateDestination;
use semantic_file_organizer::models::decision::DecisionAction;
use semantic_file_organizer::{context, discovery, normalize};

/// Temporary fixture inside the crate's own `target/` directory.
///
/// It is not a user file and it is removed on drop.
struct Fixture {
    root: PathBuf,
    target: PathBuf,
    cbx_dir: PathBuf,
    similar: Vec<PathBuf>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn make_fixture(tag: &str) -> Fixture {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("phase4-fixture-{tag}-{}", process::id()));
    let _ = fs::remove_dir_all(&root);

    let cbx_dir = root.join("Research").join("CBX");
    let misc_dir = root.join("Misc");
    fs::create_dir_all(&cbx_dir).unwrap();
    fs::create_dir_all(&misc_dir).unwrap();

    let target = root.join("CBX（カンナビノイド）とは？｜NATURECAN.png");
    fs::write(&target, b"fixture").unwrap();

    // Normalize turns text xattrs containing "Firefox" into `X:<key>`
    // semantic signals; this seeds the search term "CBX" for the fixture
    // without depending on the Spotlight index.
    xattr::set(&target, "CBX", b"Firefox\0").unwrap();

    let mut similar = Vec::new();
    for (dir, name) in [
        (&cbx_dir, "note-a.md"),
        (&cbx_dir, "note-b.md"),
        (&misc_dir, "extra-note.md"),
    ] {
        let path = dir.join(name);
        fs::write(&path, b"fixture").unwrap();
        similar.push(path);
    }

    Fixture {
        root,
        target,
        cbx_dir,
        similar,
    }
}

/// Spotlight stub: returns the fixture files for the "CBX" term only,
/// so a broken term extraction surfaces as "no candidates".
fn stub_search(files: Vec<PathBuf>) -> impl Fn(&str, usize) -> anyhow::Result<Vec<String>> + Clone {
    move |query: &str, _limit: usize| {
        if query.contains("CBX") {
            Ok(files
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

/// Run production Observe -> Normalize -> Discover over the fixture.
fn discovered_candidates(
    fixture: &Fixture,
    search: impl Fn(&str, usize) -> anyhow::Result<Vec<String>>,
) -> Vec<CandidateDestination> {
    let raw = context::builder::build(&fixture.target).unwrap();
    let normalized = normalize::normalize(raw).unwrap();
    discovery::discover_with(normalized, search).unwrap()
}

fn mock_selecting(destination: &str, probabilities: &[(&str, f64)]) -> MockJevIntegration {
    let mut map = HashMap::new();
    for (id, prob) in probabilities {
        map.insert((*id).to_string(), *prob);
    }
    MockJevIntegration::configured_decision(JevResponse {
        destination_id: Some(destination.to_string()),
        probabilities: map,
        confidence: 0.0,
        margin: 0.0,
    })
}

#[tokio::test]
async fn phase4_full_pipeline_happy_path_selects_discovered_candidate() {
    let fixture = make_fixture("happy");
    let search = stub_search(fixture.similar.clone());

    // The expected candidates come from the production Discover result.
    let candidates = discovered_candidates(&fixture, search.clone());
    assert_eq!(candidates.len(), 2, "fixture must yield two candidates");
    let chosen = candidates[0].id.clone();
    let other = candidates[1].id.clone();
    assert_eq!(candidates[0].path, fixture.cbx_dir);
    assert!(candidates[0].path.is_dir(), "candidate must be a real dir");

    let mock = mock_selecting(
        &chosen,
        &[
            (chosen.as_str(), 0.82),
            (other.as_str(), 0.11),
            ("NONE", 0.07),
        ],
    );

    let decision = classify_with_search(&fixture.target, &mock, search)
        .await
        .expect("pipeline should succeed");

    assert_eq!(decision.destination_id, Some(chosen.clone()));
    assert!((decision.confidence - 0.82).abs() < 1e-9);
    assert!((decision.margin - 0.71).abs() < 1e-9);
    assert_eq!(decision.action, DecisionAction::MoveTo(chosen));
}

#[tokio::test]
async fn phase4_full_pipeline_honors_lower_ranked_candidate_id() {
    // Candidate IDs are stable identities, not rankings: a higher score does
    // not make an ID privileged. If Jev selects the lower-scored candidate,
    // the decision must name exactly that candidate.
    let fixture = make_fixture("rank-independence");
    let search = stub_search(fixture.similar.clone());

    let candidates = discovered_candidates(&fixture, search.clone());
    assert_eq!(candidates.len(), 2);
    let higher = &candidates[0];
    let lower = &candidates[1];
    assert!(higher.score >= lower.score);
    assert_ne!(higher.id, lower.id);

    let mock = mock_selecting(
        &lower.id,
        &[
            (lower.id.as_str(), 0.75),
            (higher.id.as_str(), 0.10),
            ("NONE", 0.15),
        ],
    );

    let decision = classify_with_search(&fixture.target, &mock, search)
        .await
        .expect("pipeline should succeed");

    assert_eq!(decision.destination_id, Some(lower.id.clone()));
    assert_eq!(decision.action, DecisionAction::MoveTo(lower.id.clone()));
    assert_eq!(lower.path, fixture.root.join("Misc"));
    assert!(lower.path.is_dir());
}

#[tokio::test]
async fn phase4_full_pipeline_no_candidates_yields_safe_none() {
    let fixture = make_fixture("no-candidates");
    let search = stub_search(Vec::new()); // Spotlight finds nothing

    // A mock that would fail loudly proves Jev is not consulted when there
    // are no choices to select from.
    let mock = MockJevIntegration::configured_error("must-not-be-called");

    let decision = classify_with_search(&fixture.target, &mock, search)
        .await
        .expect("no candidates is a safe outcome, not an error");

    assert_eq!(decision.destination_id, Some("NONE".to_string()));
    assert_eq!(decision.action, DecisionAction::None);
    assert_eq!(decision.confidence, 0.0);
    assert_eq!(decision.margin, 0.0);
}

#[tokio::test]
async fn phase4_full_pipeline_none_selection_moves_nowhere() {
    let fixture = make_fixture("none-selected");
    let search = stub_search(fixture.similar.clone());
    let candidates = discovered_candidates(&fixture, search.clone());
    assert!(!candidates.is_empty());

    let mock = mock_selecting(
        "NONE",
        &[
            ("NONE", 0.70),
            (candidates[0].id.as_str(), 0.20),
            (candidates[1].id.as_str(), 0.10),
        ],
    );

    let decision = classify_with_search(&fixture.target, &mock, search)
        .await
        .expect("NONE is a normal decision");

    assert_eq!(decision.destination_id, Some("NONE".to_string()));
    assert_eq!(decision.action, DecisionAction::None);
    assert!((decision.confidence - 0.70).abs() < 1e-9);
    assert!((decision.margin - 0.50).abs() < 1e-9);
}

#[tokio::test]
async fn phase4_full_pipeline_unknown_candidate_rejected() {
    let fixture = make_fixture("unknown-candidate");
    let search = stub_search(fixture.similar.clone());
    assert!(!discovered_candidates(&fixture, search.clone()).is_empty());

    let mock = MockJevIntegration::default().set_response(Some("D99".to_string()));

    let result = classify_with_search(&fixture.target, &mock, search).await;

    let err = result.expect_err("D99 must be rejected").to_string();
    assert!(
        err.contains("D99"),
        "error should name the rejected id: {err}"
    );
}

#[tokio::test]
async fn phase4_full_pipeline_arbitrary_path_rejected() {
    let fixture = make_fixture("arbitrary-path");
    let search = stub_search(fixture.similar.clone());
    assert!(!discovered_candidates(&fixture, search.clone()).is_empty());

    let mock = MockJevIntegration::default().set_response(Some("/Users/foo/Documents".to_string()));

    let result = classify_with_search(&fixture.target, &mock, search).await;

    let err = result
        .expect_err("Jev may select a candidate, never create a destination")
        .to_string();
    assert!(
        err.contains("/Users/foo/Documents"),
        "error should name the rejected path: {err}"
    );
}

#[tokio::test]
async fn phase4_full_pipeline_transport_error_propagated() {
    let fixture = make_fixture("transport-error");
    let search = stub_search(fixture.similar.clone());
    assert!(!discovered_candidates(&fixture, search.clone()).is_empty());

    let mock = MockJevIntegration::configured_error("timeout");

    let result = classify_with_search(&fixture.target, &mock, search).await;

    let err = result
        .expect_err("transport errors must propagate")
        .to_string();
    assert!(
        err.contains("timeout"),
        "error should carry the cause: {err}"
    );
}

#[tokio::test]
async fn phase4_full_pipeline_invalid_probability_rejected() {
    let fixture = make_fixture("invalid-probability");
    let search = stub_search(fixture.similar.clone());
    let candidates = discovered_candidates(&fixture, search.clone());
    assert!(!candidates.is_empty());
    let chosen = candidates[0].id.clone();

    let mock = mock_selecting(&chosen, &[(chosen.as_str(), f64::NAN)]);

    let result = classify_with_search(&fixture.target, &mock, search).await;

    assert!(
        result.is_err(),
        "malformed probabilities must not become confidence"
    );
}

#[tokio::test]
async fn phase4_full_pipeline_empty_response_rejected() {
    let fixture = make_fixture("empty-response");
    let search = stub_search(fixture.similar.clone());
    assert!(!discovered_candidates(&fixture, search.clone()).is_empty());

    let mock = MockJevIntegration::configured_decision(JevResponse {
        destination_id: None,
        probabilities: HashMap::new(),
        confidence: 0.0,
        margin: 0.0,
    });

    let result = classify_with_search(&fixture.target, &mock, search).await;

    assert!(
        result.is_err(),
        "a response without a destination must be rejected"
    );
}

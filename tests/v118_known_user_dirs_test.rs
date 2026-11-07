//! v0.13.2 issue #18: OS standard directories as candidate priors.
//!
//! The prior is deliberately not machine-dependent in these tests: every case
//! passes an explicit directory list, so a machine with `~/Documents` and one
//! without behave the same. The sandbox lives under `target/`, not under the
//! system temp directory: `is_excluded_destination` refuses anything under
//! `/var` (where macOS puts its temp directories), and the point of the test is
//! a usable destination.

use kaseifu::discovery::add_standard_dir_priors;
use kaseifu::models::candidate::{CandidateDestination, CandidateOrigin};
use kaseifu::platform::user_dirs::{self, UserDirKind};
use std::path::{Path, PathBuf};

fn sandbox(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-sandboxes")
        .join(format!("v118-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("sandbox");
    root
}

fn candidate(id: &str, path: &Path, score: f64) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: path.to_path_buf(),
        score,
        evidence_count: 1,
        reasons: vec!["found 1 items".to_string()],
        origin: CandidateOrigin::Spotlight,
    }
}

// ---------------------------------------------------------------------------
// The prior itself
// ---------------------------------------------------------------------------

#[test]
fn v118_an_unindexed_file_still_gets_the_standard_folder() {
    let root = sandbox("prior");
    let documents = root.join("Documents");
    std::fs::create_dir_all(&documents).expect("documents");
    let known = user_dirs::existing_dirs(vec![(UserDirKind::Document, documents.clone())]);

    // An unindexed file: the search produced nothing at all.
    let mut candidates: Vec<CandidateDestination> = Vec::new();
    add_standard_dir_priors(&mut candidates, Some("pdf"), &known);

    assert_eq!(candidates.len(), 1, "the standard folder is a candidate");
    let prior = &candidates[0];
    assert_eq!(prior.path, documents);
    assert_eq!(prior.origin, CandidateOrigin::Prior);
    assert_eq!(prior.score, 0.0, "a prior carries no similarity score");
    assert_eq!(prior.evidence_count, 0, "and no index evidence");
    assert!(
        prior
            .reasons
            .iter()
            .any(|reason| reason == "standard folder for documents (prior)"),
        "reasons: {:?}",
        prior.reasons
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v118_a_missing_standard_folder_adds_nothing() {
    let root = sandbox("missing");
    // The OS defines the folder but it was never created: `existing_dirs`
    // keeps nothing, and discovery goes on with what the search produced.
    let known = user_dirs::existing_dirs(vec![(UserDirKind::Document, root.join("Documents"))]);
    assert!(known.is_empty());
    let mut candidates: Vec<CandidateDestination> = Vec::new();
    add_standard_dir_priors(&mut candidates, Some("pdf"), &known);
    assert!(candidates.is_empty(), "no folder, no candidate, no panic");

    // An extension that maps to no standard folder adds nothing either.
    let documents = root.join("Documents");
    std::fs::create_dir_all(&documents).expect("documents");
    let known = user_dirs::existing_dirs(vec![(UserDirKind::Document, documents)]);
    add_standard_dir_priors(&mut candidates, Some("xyz"), &known);
    add_standard_dir_priors(&mut candidates, None, &known);
    assert!(candidates.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v118_prior_ids_continue_after_the_ranked_candidates() {
    let root = sandbox("ids");
    let pictures = root.join("Pictures");
    std::fs::create_dir_all(&pictures).expect("pictures");
    let known = user_dirs::existing_dirs(vec![(UserDirKind::Picture, pictures.clone())]);

    let mut candidates = vec![
        candidate("D1", &root.join("albums"), 0.72),
        candidate("D2", &root.join("archive"), 0.31),
    ];
    add_standard_dir_priors(&mut candidates, Some("png"), &known);

    assert_eq!(candidates.len(), 3);
    // Every ranked candidate keeps its identity: a decision that named D1
    // still names the same directory after the prior is appended.
    assert_eq!(candidates[0].id, "D1");
    assert_eq!(candidates[1].id, "D2");
    assert_eq!(candidates[2].id, "D3");
    assert_eq!(candidates[2].origin, CandidateOrigin::Prior);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v118_a_folder_that_is_already_a_candidate_is_not_duplicated() {
    let root = sandbox("duplicate");
    let pictures = root.join("Pictures");
    std::fs::create_dir_all(&pictures).expect("pictures");
    let known = user_dirs::existing_dirs(vec![(UserDirKind::Picture, pictures.clone())]);

    let mut candidates = vec![candidate("D1", &pictures, 0.72)];
    add_standard_dir_priors(&mut candidates, Some("png"), &known);
    assert_eq!(candidates.len(), 1, "one directory, one candidate");
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Additive JSON
// ---------------------------------------------------------------------------

#[test]
fn v118_origin_is_additive() {
    let search = candidate("D1", Path::new("/data/sinks"), 0.8);
    let json = serde_json::to_value(&search).expect("json");
    assert_eq!(
        json["origin"], "spotlight",
        "historical origin is spelled out"
    );

    let prior = CandidateDestination {
        origin: CandidateOrigin::Prior,
        ..search.clone()
    };
    let json = serde_json::to_value(&prior).expect("json");
    assert_eq!(json["origin"], "prior");

    // A report written before the field existed reads as the historical one.
    let legacy: CandidateDestination = serde_json::from_str(
        r#"{"id":"D1","path":"/data/sinks","score":0.8,"evidence_count":1,"reasons":[]}"#,
    )
    .expect("legacy json");
    assert_eq!(legacy.origin, CandidateOrigin::Spotlight);
}

// ---------------------------------------------------------------------------
// The production path
// ---------------------------------------------------------------------------

#[test]
fn v118_the_production_path_offers_the_prior_when_the_folder_exists() {
    let root = sandbox("view");
    let target = root.join("invoice.pdf");
    std::fs::write(&target, b"%PDF-1.4\n").expect("fixture");
    let documents = root.join("Documents");
    std::fs::create_dir_all(&documents).expect("documents");
    // An explicit standard-folder list: the production seam takes the list as a
    // parameter for exactly this reason (issue #18), so this test does not
    // depend on the machine's own home layout.
    let known = user_dirs::existing_dirs(vec![(UserDirKind::Document, documents.clone())]);

    let (_, outcome) = kaseifu::classifier::pipeline::discover_candidates_with_priors(
        &target,
        |_query, _limit| Ok(Vec::new()),
        &kaseifu::context::image_metadata::MetadataOptions::default(),
        &known,
    )
    .expect("discover");

    assert_eq!(outcome.candidates.len(), 1, "only the prior is offered");
    let prior = &outcome.candidates[0];
    assert_eq!(prior.origin, CandidateOrigin::Prior);
    assert_eq!(prior.path, documents);
    assert_eq!(outcome.target_indexed, Some(false));

    // No standard folder of that kind: nothing is invented.
    let (_, outcome) = kaseifu::classifier::pipeline::discover_candidates_with_priors(
        &target,
        |_query, _limit| Ok(Vec::new()),
        &kaseifu::context::image_metadata::MetadataOptions::default(),
        &[],
    )
    .expect("discover");
    assert!(outcome.candidates.is_empty(), "{:?}", outcome.candidates);
    let _ = std::fs::remove_dir_all(&root);
}

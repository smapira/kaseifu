#![cfg(target_os = "macos")]
//! Phase 4D.5 - DecisionContext wiring tests.
//!
//! These tests protect the information lineage:
//!
//! ```text
//! File -> Observe -> NormalizedFileContext
//!                       |  target / semantic_signals
//!                       v
//!                    DecisionContext  -> JevIntegration
//!                       ^
//!     Discover -> candidates
//! ```
//!
//! A spy `JevIntegration` records the context the pipeline passes to Jev.
//! The spy contains no business logic; it records the context and returns a
//! `NONE` response, so the pipeline outcome itself stays trivial.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use kaseifu::classifier::jev::{JevError, JevIntegration, JevResponse};
use kaseifu::classifier::pipeline::classify_with_search;
use kaseifu::models::CandidateDestination;
use kaseifu::models::decision::DecisionContext;
use kaseifu::models::file_context::NormalizedFileContext;
use kaseifu::{context, discovery, normalize};

// ---------------------------------------------------------------------------
// Spy JevIntegration: records the DecisionContext, decides nothing
// ---------------------------------------------------------------------------

struct SpyJevIntegration {
    recorded: Arc<Mutex<Vec<DecisionContext>>>,
}

impl SpyJevIntegration {
    fn new() -> (Self, Arc<Mutex<Vec<DecisionContext>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                recorded: Arc::clone(&recorded),
            },
            recorded,
        )
    }
}

impl JevIntegration for SpyJevIntegration {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        let recorded = Arc::clone(&self.recorded);
        let context = context.clone();

        Box::pin(async move {
            recorded.lock().expect("spy log").push(context);
            // NONE: the spy makes no selection.
            Ok(JevResponse::default())
        })
    }
}

// ---------------------------------------------------------------------------
// Fixture (same hermetic pattern as the full pipeline tests: only the
// Spotlight search boundary is stubbed)
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
    target: PathBuf,
    similar: Vec<PathBuf>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn make_fixture(tag: &str) -> Fixture {
    use std::fs;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!(
            "phase4-context-fixture-{tag}-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);

    let cbx_dir = root.join("Research").join("CBX");
    let misc_dir = root.join("Misc");
    fs::create_dir_all(&cbx_dir).unwrap();
    fs::create_dir_all(&misc_dir).unwrap();

    let target = root.join("CBX（カンナビノイド）とは？｜NATURECAN.png");
    fs::write(&target, b"fixture").unwrap();
    // Production Normalize turns this xattr into the "X:CBX" semantic signal.
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
        similar,
    }
}

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

/// Order-insensitive comparison helper (see the reasons assertion).
fn sorted(reasons: Vec<String>) -> Vec<String> {
    let mut reasons = reasons;
    reasons.sort();
    reasons
}

/// Production Observe -> Normalize, used as the lineage reference.
fn production_normalized(fixture: &Fixture) -> NormalizedFileContext {
    let raw = context::builder::build(&fixture.target).unwrap();
    normalize::normalize(raw).unwrap()
}

/// Production Observe -> Normalize -> Discover, used as the lineage reference.
fn production_candidates(
    fixture: &Fixture,
    search: impl Fn(&str, usize) -> anyhow::Result<Vec<String>>,
) -> Vec<CandidateDestination> {
    discovery::discover_with(production_normalized(fixture), search).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase4_context_carries_normalized_target_and_signals() {
    let fixture = make_fixture("target-signals");
    let search = stub_search(fixture.similar.clone());
    let normalized = production_normalized(&fixture);

    let (spy, recorded) = SpyJevIntegration::new();
    let decision = classify_with_search(&fixture.target, &spy, search)
        .await
        .expect("pipeline should run");
    assert_eq!(decision.destination_id, Some("NONE".to_string()));

    let recorded = recorded.lock().expect("spy log");
    assert_eq!(recorded.len(), 1, "Jev must be consulted exactly once");
    let context = &recorded[0];

    // Lineage: what Normalize produced is exactly what Jev received.
    assert_eq!(context.target().filename, normalized.target.filename);
    assert_eq!(context.target().extension, normalized.target.extension);
    assert_eq!(context.target().file_type, normalized.file_type);
    assert_eq!(
        context.semantic_signals(),
        normalized.semantic_signals.as_slice()
    );

    // And the context is genuinely populated (not the empty ID-only default).
    assert!(
        context.target().filename.contains("NATURECAN"),
        "filename was {:?}",
        context.target().filename
    );
    assert_eq!(context.target().extension.as_deref(), Some("png"));
    assert!(
        !context.semantic_signals().is_empty(),
        "semantic_signals must come from production normalization"
    );
    assert!(
        context.semantic_signals().iter().any(|s| s == "X:CBX"),
        "expected the fixture-derived signal, got {:?}",
        context.semantic_signals()
    );
}

#[tokio::test]
async fn phase4_context_candidates_preserve_discovery_evidence() {
    let fixture = make_fixture("candidates");
    let search = stub_search(fixture.similar.clone());
    let discovered = production_candidates(&fixture, search.clone());
    assert_eq!(discovered.len(), 2, "fixture must yield two candidates");

    let (spy, recorded) = SpyJevIntegration::new();
    let _ = classify_with_search(&fixture.target, &spy, search)
        .await
        .expect("pipeline should run");

    let recorded = recorded.lock().expect("spy log");
    let context = &recorded[0];

    assert_eq!(context.candidates().len(), discovered.len());
    for (context_candidate, discovered_candidate) in
        context.candidates().iter().zip(discovered.iter())
    {
        assert_eq!(
            context_candidate.id, discovered_candidate.id,
            "candidate identity must be preserved"
        );
        assert_eq!(
            context_candidate.path,
            discovered_candidate.path.display().to_string(),
            "candidate destination path must be preserved for {}",
            discovered_candidate.id
        );
        assert_eq!(
            context_candidate.evidence_count, discovered_candidate.evidence_count,
            "evidence_count must be preserved for {}",
            discovered_candidate.id
        );
        // Content equality, not order: Phase 3 currently emits matched terms
        // in HashSet iteration order, which varies between runs. Preserving
        // the evidence itself is this cycle's invariant.
        assert_eq!(
            sorted(context_candidate.reasons.clone()),
            sorted(discovered_candidate.reasons.clone()),
            "reasons must be preserved for {}",
            discovered_candidate.id
        );
    }

    // Candidate integrity: every candidate is a real discovered directory.
    for discovered_candidate in &discovered {
        assert!(discovered_candidate.path.is_dir());
    }
    assert!(context.candidates().iter().all(|c| !c.id.is_empty()));
}

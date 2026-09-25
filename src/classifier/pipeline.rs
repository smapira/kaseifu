//! Phase 4 - Decision pipeline orchestration.
//!
//! Connects the existing production components:
//!
//! ```text
//! File
//!  -> Observe   (context::builder)
//!  -> Normalize (normalize::normalize)
//!  -> Discover  (discovery::discover_with)
//!  -> DecisionContext
//!  -> JevIntegration (mock or real adapter)
//!  -> validation
//!  -> JevDecision
//! ```
//!
//! This module only coordinates components. It owns no scoring and no
//! decision policy; those live in `models::decision`.
//!
//! Safety invariants:
//! - No candidates: a safe `NONE` decision is produced without consulting Jev.
//! - Jev may select an offered candidate or `NONE`; anything else (unknown
//!   IDs, filesystem paths, empty responses) is rejected.
//! - Malformed probabilities are rejected and never become confidence.

use anyhow::{Result, anyhow};
use std::path::Path;

use super::jev::JevIntegration;
use crate::models::candidate::CandidateDestination;
use crate::models::decision::{DecisionContext, JevDecision, validate_probabilities};
use crate::models::file_context::NormalizedFileContext;
use crate::{context, discovery, normalize};

/// Run the full decision pipeline using the real Spotlight (mdfind) search.
pub async fn classify(file: &Path, jev: &dyn JevIntegration) -> Result<JevDecision> {
    let (normalized, candidates) = discover_candidates(file, |query, limit| {
        crate::platform::macos::spotlight::search(query, limit)
    })?;

    decide_from_candidates(&normalized, candidates, jev).await
}

/// Run the full decision pipeline with an injectable Spotlight search
/// boundary. `search` receives the mdfind query and the result limit.
pub async fn classify_with_search<F>(
    file: &Path,
    jev: &dyn JevIntegration,
    search: F,
) -> Result<JevDecision>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    let (normalized, candidates) = discover_candidates(file, search)?;

    decide_from_candidates(&normalized, candidates, jev).await
}

/// Observe -> Normalize -> Discover.
///
/// Returns the normalized context together with the discovered candidates:
/// both are needed to build the `DecisionContext` without losing
/// information or re-deriving it.
fn discover_candidates<F>(
    file: &Path,
    search: F,
) -> Result<(NormalizedFileContext, Vec<CandidateDestination>)>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    let raw = context::builder::build(file)?; // Observe
    let normalized = normalize::normalize(raw)?; // Normalize
    let candidates = discovery::discover_with(normalized.clone(), search)?; // Discover

    Ok((normalized, candidates))
}

/// Ask Jev to select among the discovered candidates and validate the answer.
async fn decide_from_candidates(
    normalized: &NormalizedFileContext,
    candidates: Vec<CandidateDestination>,
    jev: &dyn JevIntegration,
) -> Result<JevDecision> {
    if candidates.is_empty() {
        // No choices exist; never invent one and never ask Jev.
        return Ok(JevDecision::new_none());
    }

    let decision_context = DecisionContext::from_normalized(normalized, &candidates);

    let response = jev
        .decide(&decision_context)
        .await
        .map_err(|error| anyhow!("Jev decision failed: {error}"))?;

    let choice = response
        .destination_id
        .clone()
        .ok_or_else(|| anyhow!("empty Jev response: no destination_id"))?;

    let validated = decision_context
        .validate_decision(&choice)
        .map_err(|error| anyhow!("rejected Jev decision: {error}"))?;

    let probabilities: Vec<(String, f64)> = response
        .probabilities
        .iter()
        .map(|(id, probability)| (id.clone(), *probability))
        .collect();

    validate_probabilities(&probabilities)
        .map_err(|error| anyhow!("malformed Jev response: {error}"))?;

    Ok(JevDecision::from_selection(&validated, &probabilities))
}

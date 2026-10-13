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
use crate::context::image_metadata::MetadataOptions;
use crate::models::candidate::CandidateDestination;
use crate::models::decision::{DecisionContext, JevDecision, validate_probabilities};
use crate::models::file_context::NormalizedFileContext;
use crate::{context, discovery, normalize};

/// Run the full decision pipeline using the real Spotlight (mdfind) search.
pub async fn classify(file: &Path, jev: &dyn JevIntegration) -> Result<JevDecision> {
    classify_with_options(file, jev, &MetadataOptions::default()).await
}

/// Run the full decision pipeline with explicit image toggles (v3.2).
///
/// Honors `[metadata.image]` without re-reading configuration inside the
/// pipeline; callers pass the already-resolved toggles.
pub async fn classify_with_options(
    file: &Path,
    jev: &dyn JevIntegration,
    image_options: &MetadataOptions,
) -> Result<JevDecision> {
    let (normalized, candidates) = discover_candidates_with_options(
        file,
        crate::platform::macos::spotlight::search,
        image_options,
    )?;

    let (decision, _) = decide_from_candidates(&normalized, &candidates, jev).await?;

    Ok(decision)
}

/// Run the full decision pipeline and also return the discovered candidates.
///
/// Execution (Phase 5) needs the candidate list to resolve the destination
/// path; the decision only ever names a candidate ID.
pub async fn classify_with_candidates(
    file: &Path,
    jev: &dyn JevIntegration,
) -> Result<(JevDecision, Vec<CandidateDestination>)> {
    classify_with_candidates_with_options(file, jev, &MetadataOptions::default()).await
}

/// [`classify_with_candidates`] with explicit image toggles (v3.2).
pub async fn classify_with_candidates_with_options(
    file: &Path,
    jev: &dyn JevIntegration,
    image_options: &MetadataOptions,
) -> Result<(JevDecision, Vec<CandidateDestination>)> {
    let (normalized, candidates) = discover_candidates_with_options(
        file,
        crate::platform::macos::spotlight::search,
        image_options,
    )?;

    let (decision, _) = decide_from_candidates(&normalized, &candidates, jev).await?;

    Ok((decision, candidates))
}

/// Everything a Rich renderer needs to display one classification.
///
/// Built in a single pipeline pass: the decision, the raw Jev probability
/// distribution (never renormalized downstream), the discovered candidates
/// for ID-to-directory resolution, and the target/signal summary.
/// Read-only payload: renderers must not mutate it.
#[derive(Debug, Clone)]
pub struct ClassifyReport {
    pub decision: JevDecision,
    pub probabilities: Vec<(String, f64)>,
    pub candidates: Vec<CandidateDestination>,
    pub target_filename: String,
    pub target_extension: Option<String>,
    pub target_file_type: Option<String>,
    pub target_size_bytes: u64,
    pub semantic_signals: Vec<String>,
}

/// Run the full decision pipeline and keep everything needed for display.
///
/// Same single pass as [`classify`]; the report additionally carries the
/// validated probability distribution and the candidate list so renderers
/// never re-run Discovery or Jev (v3.1 section 9).
pub async fn classify_report(file: &Path, jev: &dyn JevIntegration) -> Result<ClassifyReport> {
    classify_report_with_options(file, jev, &MetadataOptions::default()).await
}

/// [`classify_report`] with explicit image toggles (v3.2).
pub async fn classify_report_with_options(
    file: &Path,
    jev: &dyn JevIntegration,
    image_options: &MetadataOptions,
) -> Result<ClassifyReport> {
    classify_report_with_search_and_options(
        file,
        jev,
        crate::platform::macos::spotlight::search,
        image_options,
    )
    .await
}

/// [`classify_report`] with an injectable Spotlight search boundary.
pub async fn classify_report_with_search<F>(
    file: &Path,
    jev: &dyn JevIntegration,
    search: F,
) -> Result<ClassifyReport>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    classify_report_with_search_and_options(file, jev, search, &MetadataOptions::default()).await
}

/// [`classify_report_with_search`] with explicit image toggles (v3.2).
pub async fn classify_report_with_search_and_options<F>(
    file: &Path,
    jev: &dyn JevIntegration,
    search: F,
    image_options: &MetadataOptions,
) -> Result<ClassifyReport>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    let (normalized, candidates) = discover_candidates_with_options(file, search, image_options)?;
    let (decision, probabilities) = decide_from_candidates(&normalized, &candidates, jev).await?;

    Ok(ClassifyReport {
        decision,
        probabilities,
        candidates,
        target_filename: normalized.target.filename.clone(),
        target_extension: normalized.target.extension.clone(),
        target_file_type: normalized.file_type.clone(),
        target_size_bytes: normalized.size_bytes,
        semantic_signals: normalized.semantic_signals.clone(),
    })
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
    classify_with_search_and_options(file, jev, search, &MetadataOptions::default()).await
}

/// [`classify_with_search`] with explicit image toggles (v3.2).
pub async fn classify_with_search_and_options<F>(
    file: &Path,
    jev: &dyn JevIntegration,
    search: F,
    image_options: &MetadataOptions,
) -> Result<JevDecision>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    let (normalized, candidates) = discover_candidates_with_options(file, search, image_options)?;

    let (decision, _) = decide_from_candidates(&normalized, &candidates, jev).await?;

    Ok(decision)
}

/// Observe -> Normalize -> Discover with explicit image toggles (v3.2).
///
/// Returns the normalized context together with the discovered candidates:
/// both are needed to build the `DecisionContext` without losing
/// information or re-deriving it.
fn discover_candidates_with_options<F>(
    file: &Path,
    search: F,
    image_options: &MetadataOptions,
) -> Result<(NormalizedFileContext, Vec<CandidateDestination>)>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    let raw = context::builder::build(file)?; // Observe
    let normalized = normalize::normalize_with_options(raw, image_options)?; // Normalize
    let candidates = discovery::discover_with(normalized.clone(), search)?; // Discover

    Ok((normalized, candidates))
}

/// Ask Jev to select among the discovered candidates and validate the answer.
///
/// Returns the validated decision together with the raw probability
/// distribution (kept for display; never renormalized downstream).
async fn decide_from_candidates(
    normalized: &NormalizedFileContext,
    candidates: &[CandidateDestination],
    jev: &dyn JevIntegration,
) -> Result<(JevDecision, Vec<(String, f64)>)> {
    if candidates.is_empty() {
        // No choices exist; never invent one and never ask Jev.
        return Ok((JevDecision::new_none(), Vec::new()));
    }

    let decision_context = DecisionContext::from_normalized(normalized, candidates);

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

    Ok((
        JevDecision::from_selection(&validated, &probabilities),
        probabilities,
    ))
}

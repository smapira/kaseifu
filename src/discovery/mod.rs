use crate::models::{SimilarFile, file_context::NormalizedFileContext};

// Internal functions have placeholder mode disabled for now
mod candidates;
mod destinations;
mod scoring;
mod search_terms;
mod similarity;

// Re-export public API
pub use candidates::{
    aggregate_by_directory, aggregate_by_directory_with_exclusions, breakdown_line,
};
pub use destinations::is_excluded_destination;
pub use scoring::{
    aggregate_directory_score, extension_match, filename_similarity, normalize_name, score_file,
    token_jaccard,
};
pub use search_terms::{extract_terms, normalize_terms};
pub use similarity::find_similar_files;

/// Maximum number of raw Spotlight hits fetched and scored per run.
///
/// `mdfind` has no result-limit option and returns every match in index
/// order, so truncating *before* ranking made the candidate set depend on
/// that arbitrary order (image hits were crowded out by unrelated files).
/// The measured image-scale query for one photo series returns ~3.5k paths
/// (~0.5 MB of text), so 10k leaves headroom while bounding memory and the
/// per-hit scoring cost (`exists` + `canonicalize` + `metadata` per path)
/// for pathological word queries.
pub const MAX_SPOTLIGHT_HITS: usize = 10_000;

/// Maximum number of ranked destinations returned by [`discover`].
///
/// Applied after aggregation and ranking, so the cut follows the scores
/// rather than `mdfind`'s hit order. 50 keeps the `classify` payload sent
/// to Jev and the `discover` listing bounded; it matches the raw hit window
/// this pipeline used before (50 hits), so consumers do not grow.
pub const MAX_CANDIDATE_DESTINATIONS: usize = 50;

/// Main discovery function: NormalizedFileContext -> CandidateDestination[]
///
/// Pipeline:
/// 1. Extract search terms from NormalizedFileContext
/// 2. Search using Spotlight (mdfind)
/// 3. Aggregate similar files by parent directory
/// 4. Generate CandidateDestinations with scores
///
/// Returns Vec<CandidateDestination> sorted by score (descending)
pub fn discover(
    normalized: NormalizedFileContext,
) -> anyhow::Result<Vec<crate::models::candidate::CandidateDestination>> {
    discover_with(normalized, |query, limit| {
        crate::platform::macos::spotlight::search(query, limit)
    })
}

/// Same as [`discover`] but with an injectable Spotlight search boundary.
/// `search` receives the mdfind query and the result limit.
pub fn discover_with<F>(
    normalized: NormalizedFileContext,
    search: F,
) -> anyhow::Result<Vec<crate::models::candidate::CandidateDestination>>
where
    F: Fn(&str, usize) -> anyhow::Result<Vec<String>>,
{
    // Step 1: Extract search terms from normalized context
    let raw_terms = extract_terms(&normalized);
    let terms = normalize_terms(&raw_terms);

    crate::debug!("raw terms: {:?} -> after normalize: {:?}", raw_terms, terms);

    // Step 2: Search using Spotlight (mdfind) or the provided search boundary
    let target_name = normalized.target.filename.clone();
    let target_ext: Option<String> = normalized.target.extension.clone();
    let target_path = normalized.target.path.clone();
    let target_parent = normalized.parent_directory.clone();
    let similar_files = similarity::find_similar_files_with(
        &terms,
        &target_name,
        target_ext.as_deref(),
        &target_path,
        &target_parent,
        search,
    )?;

    crate::debug!("SimilarFile count: {}", similar_files.len());
    for f in &similar_files {
        crate::debug!(
            "  similar path={} score={:.2} filename={:.2} token={:.2} ext={:.2} matched={:?}",
            f.path.display(),
            f.similarity_score,
            f.filename_similarity,
            f.token_similarity,
            f.extension_match,
            f.matched_terms
        );
    }

    // Step 3: Aggregate similar files by parent directory
    // The target's own parent directory is never a move destination, and a
    // directory the user could not have picked (app store, cache, hidden
    // tree, generated name) is dropped by the aggregation itself — see
    // `destinations` for the rules.
    let mut candidates =
        candidates::aggregate_by_directory_with_exclusions(similar_files, &[target_parent])?;

    // Step 4: Keep the ranked head. Truncation lives here (and not in the
    // search) so the ranking, not the raw hit order, decides the cut.
    let ranked_count = candidates.len();
    candidates.truncate(MAX_CANDIDATE_DESTINATIONS);

    crate::debug!(
        "Candidate destinations: {} (returned {})",
        ranked_count,
        candidates.len()
    );
    for c in &candidates {
        crate::debug!(
            "  {} path={} score={:.1} evidence={}",
            c.id,
            c.path.display(),
            c.score,
            c.evidence_count
        );
    }

    Ok(candidates)
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::models::file_context::NormalizedFileContext;

//     #[test]
//     fn test_discover_empty_terms() {
//         let normalized = NormalizedFileContext {
//             path: std::path::PathBuf::from("/Users/test/file.png"),
//             ...
//         };

//         let candidates = discover(normalized).unwrap();
//         assert!(candidates.is_empty());
//     }

//     #[test]
//     fn test_discover_pipeline_connections() {
//         let normalized = NormalizedFileContext {
//             path: std::path::PathBuf::from("/Users/test/file.png"),
//             ...
//         };

//         let _ = discover(normalized);
//     }
// }

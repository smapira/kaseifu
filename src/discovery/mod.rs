use crate::models::candidate::{CandidateDestination, CandidateOrigin};
use crate::models::{SimilarFile, file_context::NormalizedFileContext};
use crate::platform::user_dirs::{self, UserDirKind};
use std::path::PathBuf;

// Internal functions have placeholder mode disabled for now
mod candidates;
mod destinations;
mod scoring;
mod search_terms;
mod similarity;

// Re-export public API
pub use candidates::{
    aggregate_by_directory, aggregate_by_directory_with_exclusions,
    aggregate_by_directory_with_shape, breakdown_line,
};
pub use destinations::is_excluded_destination;
pub use scoring::{
    DirectoryShape, adjust_for_shape, aggregate_directory_score, depth_below_home, extension_match,
    filename_similarity, normalize_name, score_file, token_jaccard,
};
pub use search_terms::{extract_terms, normalize_terms};
pub use similarity::{
    SimilarFiles, find_similar_files, find_similar_files_with, scan_similar_files_with,
};

/// [`discover`] plus the standard-folder prior (issue #18).
/// The diagnostic `discover` command and the production pipeline both want the
/// same candidate set, and that set now includes the operating-system folder the
/// file's type belongs in. `discover`/`discover_with` themselves stay exactly as
/// they were: their callers pin the ranked list, and a prior whose presence
/// depends on the machine's home layout must not move those numbers.
pub fn discover_with_priors(
    normalized: NormalizedFileContext,
) -> anyhow::Result<Vec<CandidateDestination>> {
    discover_with_priors_and_search(normalized, |query, limit| {
        crate::platform::macos::spotlight::search(query, limit)
    })
}

/// [`discover_with_priors`] with an injectable search boundary.
///
/// `--roots` (issue #19) is a capturing closure over this seam, so the scope of
/// a run never needs a second discovery implementation.
pub fn discover_with_priors_and_search<F>(
    normalized: NormalizedFileContext,
    search: F,
) -> anyhow::Result<Vec<CandidateDestination>>
where
    F: Fn(&str, usize) -> anyhow::Result<Vec<String>>,
{
    let extension = normalized.target.extension.clone();
    let mut found = discover_with(normalized, search)?;
    add_known_dir_priors(&mut found, extension.as_deref());
    Ok(found)
}

/// The path-shape facts for one discovery run: the home directory and the
/// standard folder for the target's type (issues #18 and #19).
///
/// Resolved once per run so the aggregation never re-reads the environment per
/// candidate directory.
fn directory_shape(target_extension: Option<&str>) -> DirectoryShape {
    let known = target_extension
        .and_then(user_dirs::kind_for_extension)
        .and_then(|kind| {
            user_dirs::known_dirs()
                .into_iter()
                .find(|(known_kind, _)| *known_kind == kind)
                .map(|(_, path)| path)
        });
    DirectoryShape {
        home: dirs::home_dir(),
        known_dir: known,
    }
}

/// Append the standard folder for this file type as a separate candidate.
///
/// The prior is not mixed into a Spotlight score: it carries score 0.0, no
/// evidence rows and `origin: prior`, and its reason says which standard folder
/// it is. That is the honest shape for "the index knows nothing about this file,
/// but a PDF belongs in `~/Documents`" (issue #18), and it is what makes an
/// unindexed file negotiable at all on a machine without Spotlight.
///
/// Nothing is added when the type maps to no standard folder, when the folder
/// does not exist, when it is already a candidate, when it is on the
/// never-a-destination list, or when the list is already at its bound.
pub fn add_known_dir_priors(candidates: &mut Vec<CandidateDestination>, extension: Option<&str>) {
    let known = user_dirs::known_dirs();
    add_standard_dir_priors(candidates, extension, &known);
}

/// [`add_known_dir_priors`] over an explicit directory list (tests and callers
/// that already resolved the platform answers).
pub fn add_standard_dir_priors(
    candidates: &mut Vec<CandidateDestination>,
    extension: Option<&str>,
    known: &[(UserDirKind, PathBuf)],
) {
    let Some(kind) = extension.and_then(user_dirs::kind_for_extension) else {
        return;
    };
    if candidates.len() >= MAX_CANDIDATE_DESTINATIONS {
        return;
    }
    let Some((_, directory)) = known.iter().find(|(known_kind, _)| *known_kind == kind) else {
        return;
    };
    let shown = directory.to_string_lossy().to_string();
    if is_excluded_destination(&shown) {
        return;
    }
    if candidates
        .iter()
        .any(|candidate| same_directory(&candidate.path, directory))
    {
        return;
    }
    candidates.push(CandidateDestination {
        id: format!("D{}", candidates.len() + 1),
        path: directory.clone(),
        score: 0.0,
        evidence_count: 0,
        reasons: vec![format!("standard folder for {} (prior)", kind.label())],
        origin: CandidateOrigin::Prior,
    });
}

/// True when two spellings name the same directory.
///
/// Compared by canonical path when both resolve (so a symlinked `~/Documents`
/// and the real folder are one candidate) and by text otherwise.
fn same_directory(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

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

/// Result of one discovery run.
///
/// `candidates` is what the caller ranks and plans with; `target_indexed`
/// says whether Spotlight knew the target file when those candidates were
/// built. They travel together because the second decides how far the first
/// can be trusted: Spotlight indexes asynchronously, so the area a file was
/// just moved into (or out of) is exactly where the index is thinnest.
#[derive(Debug, Clone)]
pub struct DiscoveryOutcome {
    pub candidates: Vec<crate::models::candidate::CandidateDestination>,
    /// See [`SimilarFiles::target_indexed`]: `Some(false)` means the target
    /// had no usable index entry, so the evidence below it is incomplete.
    pub target_indexed: Option<bool>,
}

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
    Ok(discover_with_outcome(normalized, search)?.candidates)
}

/// [`discover_with`] plus the index state of the target
/// ([`DiscoveryOutcome`]).
pub fn discover_with_outcome<F>(
    normalized: NormalizedFileContext,
    search: F,
) -> anyhow::Result<DiscoveryOutcome>
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
    let scan = similarity::scan_similar_files_with(
        &terms,
        &target_name,
        target_ext.as_deref(),
        &target_path,
        &target_parent,
        search,
    )?;
    let similar_files = scan.files;

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
    // tree, generated name) is dropped by the aggregation itself - see
    // `destinations` for the rules. The path-shape facts of this run adjust
    // the directory score (issue #19) and never remove a candidate.
    let shape = directory_shape(target_ext.as_deref());
    let mut candidates =
        candidates::aggregate_by_directory_with_shape(similar_files, &[target_parent], &shape)?;

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

    Ok(DiscoveryOutcome {
        candidates,
        target_indexed: scan.target_indexed,
    })
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

use crate::models::{SimilarFile, file_context::NormalizedFileContext};

// Internal functions have placeholder mode disabled for now
mod candidates;
mod search_terms;
mod similarity;

// Re-export public API
pub use candidates::aggregate_by_directory;
pub use search_terms::{extract_terms, normalize_terms};
pub use similarity::find_similar_files;

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
    // Step 1: Extract search terms from normalized context
    let terms = extract_terms(&normalized);

    // Step 2: Search using Spotlight (mdfind)
    let similar_files = similarity::find_similar_files(&terms)?;

    // Step 3: Aggregate similar files by parent directory
    let candidates = candidates::aggregate_by_directory(similar_files)?;

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

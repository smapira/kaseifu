use std::path::{Path, PathBuf};

use crate::discovery::SimilarFile;
use crate::platform::macos::spotlight;

/// Find files similar to the given search terms using Spotlight
///
/// # Parameters
/// - `terms`: Search terms to query Spotlight
///
/// # Returns
/// - `Ok(Vec<SimilarFile>)`: List of similar files found
/// - `Err(anyhow::Error)`: If Spotlight search fails
pub fn find_similar_files(terms: &[String]) -> anyhow::Result<Vec<SimilarFile>> {
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Combine terms into a single query
    let search_query = terms.join(" ");

    // Search using Spotlight
    let found_paths = spotlight::search(&search_query, 50)?;

    // Deduplicate and process results
    for path_str in found_paths {
        if seen_paths.contains(&path_str) {
            continue;
        }

        // Check if path is safe (not system directory)
        let is_system = is_system_directory(&path_str);
        if is_system {
            seen_paths.insert(path_str.clone());
            continue;
        }

        // Validate path exists and get parent directory
        let path_obj = Path::new(&path_str);
        if path_obj.exists() {
            let parent = if let Some(p) = path_obj.parent() {
                p.to_string_lossy().to_string()
            } else {
                path_str.clone()
            };

            results.push(SimilarFile {
                path: PathBuf::from(path_str.clone()),
                parent_directory: parent,
                matched_terms: vec![path_str.clone()],
                similarity_score: 1.0,
            });

            seen_paths.insert(path_str);
        }
    }

    Ok(results)
}

/// Check if path is a system directory that should be excluded
fn is_system_directory(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    let system_paths = [
        "/system", "/private", "/usr", "/bin", "/sbin", "/var", "/tmp",
    ];

    system_paths.iter().any(|p| path_lower.contains(*p))
}

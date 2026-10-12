use std::path::{Path, PathBuf};

use crate::discovery::SimilarFile;
use crate::discovery::scoring;
use crate::platform::macos::spotlight;

pub fn find_similar_files(
    terms: &[String],
    target_file_name: &str,
    target_ext: Option<&str>,
    target_path: &Path,
    target_parent: &str,
) -> anyhow::Result<Vec<SimilarFile>> {
    find_similar_files_with(
        terms,
        target_file_name,
        target_ext,
        target_path,
        target_parent,
        spotlight::search,
    )
}

pub fn find_similar_files_with<F>(
    terms: &[String],
    target_file_name: &str,
    target_ext: Option<&str>,
    target_path: &Path,
    _target_parent: &str,
    search: F,
) -> anyhow::Result<Vec<SimilarFile>>
where
    F: Fn(&str, usize) -> anyhow::Result<Vec<String>>,
{
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    let search_query = terms.join(" ");

    crate::debug!("mdfind query: '{}'", search_query);

    let found_paths = search(&search_query, 50)?;

    crate::debug!("Spotlight raw results: {}", found_paths.len());

    let target_canonical = canonical_key(target_path);

    for path_str in &found_paths {
        if seen_paths.contains(path_str) {
            continue;
        }
        seen_paths.insert(path_str.clone());

        if is_system_directory(path_str) {
            continue;
        }

        let path_obj = Path::new(path_str.as_str());
        if !path_obj.exists() {
            continue;
        }
        if canonical_key(path_obj) == target_canonical {
            continue;
        }

        let parent = if let Some(p) = path_obj.parent() {
            p.to_string_lossy().to_string()
        } else {
            path_str.clone()
        };

        let scored = scoring::score_file(target_file_name, target_ext, path_str, terms);

        results.push(SimilarFile {
            path: PathBuf::from(path_str.clone()),
            parent_directory: parent,
            matched_terms: scored.matched_terms,
            similarity_score: scored.score,
            filename_similarity: scored.filename_similarity,
            token_similarity: scored.token_similarity,
            extension_match: scored.extension_match,
        });
    }

    crate::debug!(
        "SimilarFile candidates (after dedup + filter): {}",
        results.len()
    );

    Ok(results)
}

fn canonical_key(p: &Path) -> String {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c.to_string_lossy().to_string();
    }
    p.to_string_lossy().to_string()
}

/// Check if path is a system directory that should be excluded
fn is_system_directory(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    let system_paths = [
        "/system", "/private", "/usr", "/bin", "/sbin", "/var", "/tmp",
    ];

    system_paths.iter().any(|p| path_lower.contains(*p))
}

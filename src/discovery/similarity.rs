use std::path::{Path, PathBuf};

use crate::discovery::destinations::is_excluded_destination;
use crate::discovery::{MAX_SPOTLIGHT_HITS, SimilarFile, scoring};
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

    let search_query = build_search_query(terms, target_file_name);

    crate::debug!("mdfind query: '{}'", search_query);

    let found_paths = search(&search_query, MAX_SPOTLIGHT_HITS)?;

    crate::debug!("Spotlight raw results: {}", found_paths.len());

    let target_canonical = canonical_key(target_path);

    let mut skipped_destinations = 0usize;

    for path_str in &found_paths {
        if seen_paths.contains(path_str) {
            continue;
        }
        seen_paths.insert(path_str.clone());

        if is_system_directory(path_str) {
            continue;
        }

        let path_obj = Path::new(path_str.as_str());
        // Decided before the filesystem calls below: a hit inside a
        // destination the user could not have picked (app store, cache,
        // hidden tree, generated name) is not evidence, and rejecting it
        // here skips `exists`/`canonicalize`/`metadata` for it. Measured on
        // a real photo: 2276 of 3526 hits live in such folders.
        let parent = parent_directory(path_obj, path_str);
        if is_excluded_destination(&parent) {
            skipped_destinations += 1;
            continue;
        }

        if !path_obj.exists() {
            continue;
        }
        if canonical_key(path_obj) == target_canonical {
            continue;
        }

        let scored = scoring::score_file(target_file_name, target_ext, path_str, terms);

        results.push(SimilarFile {
            path: PathBuf::from(path_str.clone()),
            parent_directory: parent,
            matched_terms: scored.matched_terms,
            similarity_score: scored.score,
            filename_similarity: scored.filename_similarity,
            token_similarity: scored.token_similarity,
            extension_match: scored.extension_match,
            modified_at: modified_rfc3339(path_obj),
        });
    }

    if skipped_destinations > 0 {
        crate::debug!(
            "Hits skipped (unusable destination directories): {}",
            skipped_destinations
        );
    }

    crate::debug!(
        "SimilarFile candidates (after dedup + filter): {}",
        results.len()
    );

    Ok(results)
}

fn modified_rfc3339(p: &Path) -> Option<String> {
    let mtime = std::fs::metadata(p).ok()?.modified().ok()?;
    let dt: chrono::DateTime<chrono::Utc> = mtime.into();
    Some(dt.to_rfc3339())
}

/// Parent directory of a hit as a path string, or the hit itself when it has
/// no parent component (a direct child of the filesystem root).
fn parent_directory(path: &Path, raw: &str) -> String {
    path.parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_else(|| raw.to_string())
}

fn canonical_key(p: &Path) -> String {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c.to_string_lossy().to_string();
    }
    p.to_string_lossy().to_string()
}

/// mdfind query for one discovery run.
///
/// `mdfind` accepts either a bare word sequence ("sequence of words", AND
/// semantics) or an attribute expression, but never both: mixing them fails
/// with `Failed to create query` (verified against the real CLI). Image
/// targets therefore express their file-name words as attribute conditions
/// and AND them with the image content type tree scope, so the scope is
/// applied while Spotlight searches — a Rust-side filter only ever sees the
/// truncated hit list, in which images never appear.
///
/// Non-image targets keep the historical word query unchanged.
fn build_search_query(terms: &[String], target_file_name: &str) -> String {
    let word_query = || terms.join(" ");
    if !crate::context::image_metadata::is_image_file(Path::new(target_file_name)) {
        return word_query();
    }
    image_scoped_query(terms, target_file_name).unwrap_or_else(word_query)
}

/// Image scope plus file-name conditions, or `None` when no usable word is
/// left (the caller then falls back to the word query).
///
/// The conditions are ORed: the words describe a series ("IMG", "3020"),
/// and requiring all of them returns the target itself only (measured).
/// Words that do not occur in the file name are dropped, which keeps type
/// noise such as `kMDItemKind`'s "image" out of the query — including it only
/// widened the result with system frameworks and app media caches.
fn image_scoped_query(terms: &[String], target_file_name: &str) -> Option<String> {
    let name_terms = filename_terms(terms, target_file_name);
    if name_terms.is_empty() {
        return None;
    }
    let conditions: Vec<String> = name_terms
        .iter()
        .map(|term| format!("kMDItemFSName == '*{term}*'c"))
        .collect();
    Some(format!(
        "kMDItemContentTypeTree == 'public.image' && ({})",
        conditions.join(" || ")
    ))
}

/// Terms that occur in the target file name and are safe to embed in a
/// query, sorted and deduplicated so the query string is deterministic.
fn filename_terms(terms: &[String], target_file_name: &str) -> Vec<String> {
    let name = target_file_name.to_lowercase();
    let mut found: Vec<String> = terms
        .iter()
        .filter(|term| is_query_safe(term))
        .filter(|term| name.contains(&term.to_lowercase()))
        .cloned()
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Query-language safety: quotes, operators and wildcards would either break
/// the query or silently widen the name pattern, so such terms are skipped.
fn is_query_safe(term: &str) -> bool {
    !term.is_empty()
        && term
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | '#' | '+'))
}

/// Check if path is a system directory that should be excluded
fn is_system_directory(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    let system_paths = [
        "/system", "/private", "/usr", "/bin", "/sbin", "/var", "/tmp",
    ];

    system_paths.iter().any(|p| path_lower.contains(*p))
}

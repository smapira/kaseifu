use crate::models::file_context::NormalizedFileContext;

/// Extract search terms from NormalizedFileContext semantic signals
///
/// Extracts terms from M:, K:, F:, X: prefixed signals, and also uses
/// raw terms (without prefixes) as fallback for filename-derived signals
///
/// Example signals:
/// - "M:CBX", "M: カンナビノイド" -> extracts "CBX", "カンナビノイド"
/// - "FB:NATURECAN", "FB:screenshot" -> extracts "NATURECAN"
/// - Untouched raw signals are filtered for keywords
pub fn extract_terms(normalized: &NormalizedFileContext) -> Vec<String> {
    let mut terms: std::collections::HashSet<String> = std::collections::HashSet::new();

    for signal in &normalized.semantic_signals {
        if signal.is_empty() {
            continue;
        }

        let term = if let Some(suffix) = signal.strip_prefix("M:") {
            Some(suffix.trim())
        } else if let Some(suffix) = signal.strip_prefix("K:") {
            Some(suffix.trim())
        } else if let Some(suffix) = signal.strip_prefix("F:") {
            Some(suffix.trim())
        } else if let Some(suffix) = signal.strip_prefix("X:") {
            Some(suffix.trim())
        } else if let Some(suffix) = signal.strip_prefix("FB:") {
            Some(suffix.trim())
        } else {
            // No structured prefix - treat raw signal as a term
            Some(signal.trim())
        };

        if let Some(term) = term {
            if !term.is_empty() {
                terms.insert(term.to_string());
            }
        }
    }

    terms.into_iter().collect()
}

/// Keywords to filter out from search terms (system words, common prefixes, etc.)
const KEYWORD_FILTERS: [&str; 34] = [
    // Common series patterns that indicate preview/thumbnail data
    "screenshot",
    "recents",
    "library",
    "public",
    "desktop",
    "documents",
    "downloads",
    "recent",
    "previews",
    "trash",
    // File extensions
    "html",
    "pdf",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "mp4",
    "mp3",
    "doc",
    "docx",
    "xls",
    "xlsx",
    "ppt",
    "pptx",
    "txt",
    "md",
    "csv",
    // Preview/View types
    "preview",
    "thumbnail",
    // System commands
    "selected",
    "open with",
    // Common file type indicators
    "related",
    "created",
    "modified",
];

/// Normalize and filter terms suitable for mdfind search
///
/// Removes empty terms and system boilerplate
pub fn normalize_terms(terms: &[String]) -> Vec<String> {
    terms
        .iter()
        .filter(|t| !t.is_empty())
        // Convert to lowercase for case-insensitive filtering
        .filter(|t| {
            let lower = t.to_lowercase();
            !KEYWORD_FILTERS.iter().any(|f| lower.contains(f))
        })
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::file_context::DirectorySummary;

    #[test]
    fn test_extract_terms_from_signals() {
        let normalized = NormalizedFileContext {
            semantic_signals: vec![
                "M:CBX".to_string(),
                "M:カンナビノイド".to_string(),
                "M:NATURECAN".to_string(),
            ],
            provenance: Default::default(),
            file_type: None,
            target: Default::default(),
            parent_directory: Default::default(),
            creation_date: Default::default(),
            modification_date: Default::default(),
            size_bytes: 0,
            candidate_destinations: Default::default(),
            directory_summary: DirectorySummary::default(),
        };

        let terms = extract_terms(&normalized);
        assert_eq!(terms.len(), 3);
        assert!(terms.iter().any(|t| t == "CBX"));
        assert!(terms.iter().any(|t| t == "カンナビノイド"));
        assert!(terms.iter().any(|t| t == "NATURECAN"));
    }

    #[test]
    fn test_extract_terms_raw_signals() {
        let normalized = NormalizedFileContext {
            semantic_signals: vec!["fallback".to_string(), "rawterm".to_string()],
            provenance: Default::default(),
            file_type: None,
            target: Default::default(),
            parent_directory: Default::default(),
            creation_date: Default::default(),
            modification_date: Default::default(),
            size_bytes: 0,
            candidate_destinations: Default::default(),
            directory_summary: DirectorySummary::default(),
        };

        let terms = extract_terms(&normalized);
        assert!(terms.contains(&"fallback".to_string()));
        assert!(terms.contains(&"rawterm".to_string()));
    }

    #[test]
    fn test_normalize_terms_filter_screenshot() {
        let terms = vec!["CBX".to_string(), "screenshot".to_string()];

        let normalized = normalize_terms(&terms);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized, vec!["CBX".to_string()]);
    }

    #[test]
    fn test_normalize_terms_basic() {
        let terms = vec!["CBX".to_string(), "palette".to_string()];

        let normalized = normalize_terms(&terms);
        assert_eq!(normalized.len(), 2);
    }
}

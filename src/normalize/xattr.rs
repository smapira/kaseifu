use serde_json::Value;

/// Normalizes xattr evidence using an allowlist approach.
///
/// ### XATTR NORMALIZATION
///
/// - Processes only known-safe attributes (e.g., `com.apple.FinderInfo`)
/// - Unknown/unlisted xattr attributes are silently skipped
/// - Binary attributes (e.g., `com.apple.macl`) are filtered by name, not by value type
/// - Allows future extension to include other safe binary metadata if needed
///
/// Example:
/// ```
/// use kaseifu::models::file_context::{Evidence, EvidenceSource};
/// use kaseifu::normalize::xattr::normalize_xattr;
/// use serde_json::Value;
///
/// let evidence = vec![
///     Evidence {
///         source: EvidenceSource::Xattr,
///         key: "com.apple.FinderInfo".into(),
///         value: Value::String("0081;...;Firefox".into()),
///         attribute: None,
///     },
/// ];
/// let result = normalize_xattr(&evidence);
/// ```
pub fn normalize_xattr(evidence: &[crate::models::file_context::Evidence]) -> XattrResult {
    let mut result = XattrResult::default();

    // Filter to only process allowlisted xattr attributes
    for item in evidence.iter() {
        apply_xattr_item(item, &mut result);
    }

    result
}

/// Single-item pipeline: source filter -> allowlist -> type filter -> parse.
fn apply_xattr_item(item: &crate::models::file_context::Evidence, result: &mut XattrResult) {
    // Only process Xattr source items
    if item.source != crate::models::file_context::EvidenceSource::Xattr {
        return;
    }

    // Allowlist check: only process known-safe attribute names
    // `is_xattr_allowed` returns true only for explicitly whitelisted attrs
    if !is_xattr_allowed(&item.key) {
        // Unknown/forbidden xattr attributes are silently skipped
        // This prevents binary data from malicious attributes from polluting semantic context
        return;
    }

    let Value::String(value_ref) = &item.value else {
        // Skip xattr with non-string values
        return;
    };

    if item.key.as_str() == "com.apple.FinderInfo" {
        apply_finder_info(value_ref, result);
    }
}

/// Parse an allowlisted FinderInfo value (sanity filter + agent/URL extraction).
fn apply_finder_info(value_ref: &str, result: &mut XattrResult) {
    // Basic sanity check to skip obviously corrupted data
    if is_corrupted_finder_value(value_ref) {
        // Invalid content - skip processing
        return;
    }

    // Extract downloader agent
    if let Some(agent) = parse_agent(value_ref) {
        result.downloader = Some(agent);
    }

    // Extract URLs if present
    for url in parse_url(value_ref) {
        if !url.is_empty() {
            result.urls.push(url);
        }
    }
}

/// Obviously corrupted FinderInfo payloads carry no semantic value.
fn is_corrupted_finder_value(value: &str) -> bool {
    value.is_empty() || value.contains("Malformed") || value.contains("invalid")
}

/// Allowlist of safe xattr attributes to process.
/// Unknown/unlisted xattr attributes are silently skipped.
fn is_xattr_allowed(key: &str) -> bool {
    matches!(key, "com.apple.FinderInfo")
}

/// Extracts the downloader agent name from FinderInfo value.
/// Example input: "0081;com.apple.coreduoethylamine;Firefox"
/// Returns the agent name if found.
///
/// @note: UTF-8 parsing only - binary/encoded data is not decoded, only string matching.
fn parse_agent(input: &str) -> Option<String> {
    let agents = ["Firefox", "Chrome", "Safari", "Edge", "Word"];
    for &agent in &agents {
        if input.contains(agent) {
            return Some(agent.to_string());
        }
    }
    None
}

/// Extracts URLs from FinderInfo value if present.
/// Returns a vector of URL strings found.
/// Currently no-op; @see parse_agent
fn parse_url(_input: &str) -> Vec<String> {
    // Placeholder - no actual URL parsing
    vec![]
}

#[derive(Debug, Clone, Default)]
pub struct XattrResult {
    /// Semantic signals from xattr are currently not extracted (see `extract_terms`)
    pub semantic_signals: Vec<String>,
    /// Detected file downloader agent from `com.apple.FinderInfo`
    pub downloader: Option<String>,
    /// Extracted URLs from `com.apple.FinderInfo` (if present)
    pub urls: Vec<String>,
}

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
        // Only process Xattr source items
        if item.source != crate::models::file_context::EvidenceSource::Xattr {
            continue;
        }

        // Allowlist check: only process known-safe attribute names
        // `is_xattr_allowed` returns true only for explicitly whitelisted attrs
        if !is_xattr_allowed(&item.key) {
            // Unknown/forbidden xattr attributes are silently skipped
            // This prevents binary data from malicious attributes from polluting semantic context
            continue;
        }

        // Process allowed xattr attributes
        let key = item.key.as_str();

        let value = match &item.value {
            Value::String(v) => Some(v.as_str()),
            _ => {
                // Skip xattr with non-string values
                continue;
            }
        };
        let value_ref = value.unwrap_or_default();

        if key == "com.apple.FinderInfo" {
            // Parse FinderInfo value
            // Basic sanity check to skip obviously corrupted data
            if value_ref.is_empty()
                || value_ref.contains("Malformed")
                || value_ref.contains("invalid")
            {
                // Invalid content - skip processing
                continue;
            }

            // Extract downloader agent
            if let Some(agent) = parse_agent(&value_ref) {
                result.downloader = Some(agent);
            }

            // Extract URLs if present
            for url in parse_url(&value_ref) {
                if !url.is_empty() {
                    result.urls.push(url);
                }
            }
        }
    }

    result
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

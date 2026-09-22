use serde_json::Value;

pub fn normalize_metadata(evidence: &[crate::models::file_context::Evidence]) -> MetadataResult {
    let mut result = MetadataResult::default();
    for item in evidence {
        let value = &item.value;
        let key = item.key.as_str();

        if matches!(key, "kMDItemContentType") {
            if let Value::String(v) = value {
                result.file_type = Some(v.clone());
            }
        } else if let Some(_v) = key.strip_prefix("kMDItemTitle") {
            if let Value::String(title_value) = value {
                for term in extract_terms(title_value) {
                    if !term.is_empty() {
                        result.semantic_signals.push(format!("M:{}", term));
                    }
                }
            }
        } else if let Some(_v) = key.strip_prefix("kMDItemKind") {
            if let Value::String(kind_value) = value {
                for term in extract_terms(kind_value) {
                    if !term.is_empty() {
                        result.semantic_signals.push(format!("K:{}", term));
                    }
                }
            }
        } else if matches!(key, "kMDItemAuthors") {
            if let Value::String(v) = value {
                result.authors = Some(format!("A:{}", v));
            }
        } else if matches!(key, "kMDItemFSCreationDate" | "kMDItemFSContentChangeDate") {
            if let Value::String(v) = value {
                result.created_date = Some(v.clone());
            }
        }
    }
    result
}

fn remove_snapshot_noise(title: &str) -> &str {
    // Strip "Screenshot YYYY-MM-DD at HH-MM-DD" prefix
    if title.starts_with("Screenshot ") {
        return &title[10..];
    }
    title
}

fn extract_terms(title: &str) -> Vec<String> {
    // Strip Screenshot prefix and date/time before processing
    let after_screenshot = if title.starts_with("Screenshot ") {
        &title[10..]
    } else {
        title
    };

    // Filter out known noise terms
    let known = [
        "recents",
        "library",
        "public",
        "desktop",
        "documents",
        "screenshot",
    ];

    after_screenshot
        .split_whitespace()
        .filter(|s| s.is_empty() && !known.iter().any(|&n| s.to_lowercase().contains(n)))
        .map(|s| s.to_string())
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct MetadataResult {
    pub semantic_signals: Vec<String>,
    pub kind: String,
    pub file_type: Option<String>,
    pub created_date: Option<String>,
    pub authors: Option<String>,
}

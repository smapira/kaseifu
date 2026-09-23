use serde_json::Value;

pub fn normalize_metadata(evidence: &[crate::models::file_context::Evidence]) -> MetadataResult {
    let mut result = MetadataResult::default();

    for item in evidence {
        let key = item.key.as_str();
        let value = &item.value;

        if matches!(key, "kMDItemContentType") {
            if let Value::String(v) = value {
                result.file_type = Some(v.clone());
            }
        } else if let Some(_v) = key.strip_prefix("kMDItemTitle") {
            if let Value::String(title_value) = value {
                for term in extract_terms(&title_value) {
                    if !term.is_empty() {
                        result.semantic_signals.push(format!("M:{}", term));
                    }
                }
            }
        } else if let Some(_v) = key.strip_prefix("kMDItemKind") {
            if let Value::String(kind_value) = value {
                for term in extract_terms(&kind_value) {
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

fn extract_terms(title: &str) -> Vec<String> {
    // Filter out noise filenames entirely
    let lower = title.to_lowercase();
    if lower == ".ds_store" || lower == ".localized" || lower.starts_with("html") {
        return Vec::new();
    }

    // Strip Screenshot prefix and date/time before processing
    let after_screenshot = if title.starts_with("Screenshot ") {
        &title[10..]
    } else {
        title
    };

    // Filter out noise terms - these produce no semantic value
    let known = [
        "recents",
        "library",
        "public",
        "desktop",
        "documents",
        "screenshot",
        "document",
        "desktop file",
    ];

    after_screenshot
        .split_whitespace()
        .filter(|s| !s.is_empty() && !known.iter().any(|&n| s.to_lowercase().contains(n)))
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

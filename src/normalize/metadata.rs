use serde_json::Value;

pub fn normalize_metadata(evidence: &[crate::models::file_context::Evidence]) -> MetadataResult {
    let mut result = MetadataResult::default();

    for item in evidence {
        match classify_metadata_key(item.key.as_str()) {
            MetadataKey::ContentType => apply_content_type(&item.value, &mut result),
            MetadataKey::Title => push_prefixed_terms(&item.value, "M", &mut result),
            MetadataKey::Kind => push_prefixed_terms(&item.value, "K", &mut result),
            MetadataKey::Authors => apply_authors(&item.value, &mut result),
            MetadataKey::CreatedDate => apply_created_date(&item.value, &mut result),
            MetadataKey::Ignored => {}
        }
    }
    result
}

/// Spotlight key classification: dispatch is data-driven, behavior unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataKey {
    ContentType,
    Title,
    Kind,
    Authors,
    CreatedDate,
    Ignored,
}

fn classify_metadata_key(key: &str) -> MetadataKey {
    if key == "kMDItemContentType" {
        MetadataKey::ContentType
    } else if key.starts_with("kMDItemTitle") {
        MetadataKey::Title
    } else if key.starts_with("kMDItemKind") {
        MetadataKey::Kind
    } else if key == "kMDItemAuthors" {
        MetadataKey::Authors
    } else if key == "kMDItemFSCreationDate" || key == "kMDItemFSContentChangeDate" {
        MetadataKey::CreatedDate
    } else {
        MetadataKey::Ignored
    }
}

fn apply_content_type(value: &Value, result: &mut MetadataResult) {
    if let Value::String(v) = value {
        result.file_type = Some(v.clone());
    }
}

fn push_prefixed_terms(value: &Value, prefix: &str, result: &mut MetadataResult) {
    if let Value::String(text) = value {
        for term in extract_terms(text) {
            if !term.is_empty() {
                result.semantic_signals.push(format!("{prefix}:{term}"));
            }
        }
    }
}

fn apply_authors(value: &Value, result: &mut MetadataResult) {
    if let Value::String(v) = value {
        result.authors = Some(format!("A:{v}"));
    }
}

fn apply_created_date(value: &Value, result: &mut MetadataResult) {
    if let Value::String(v) = value {
        result.created_date = Some(v.clone());
    }
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
    #[allow(dead_code)]
    pub kind: String,
    pub file_type: Option<String>,
    pub created_date: Option<String>,
    pub authors: Option<String>,
}

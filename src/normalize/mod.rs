// Normalize raw evidence into clean semantic signals.

mod filename;
mod metadata;
pub mod noise;
pub mod provenance;
pub mod xattr;

use crate::models::file_context::{
    DirectorySummary, FileContext, NormalizedFileContext, Provenance,
};

/// Normalize a file context from Phase 1 to Phase 2 normalized format
pub fn normalize(context: FileContext) -> anyhow::Result<NormalizedFileContext> {
    // Extract semantic signals from metadata (M:, K:, etc. labels)
    let metadata_result = metadata::normalize_metadata(&context.evidence);

    // Optionally combine with xattr provenance signals
    // Optionally add xattr signals (e.g., downloader agent)
    let mut semantic_signals = metadata_result.semantic_signals;
    for item in context.evidence.iter() {
        if item.source == crate::models::file_context::EvidenceSource::Xattr {
            let key = item.key.as_str();
            if let Some(value_str) = item.value.as_str() {
                if key.starts_with("FinderInfo") || value_str.contains("Firefox") {
                    if !value_str.is_empty() {
                        let signaling_output = format!("X:{}", key);
                        semantic_signals.push(signaling_output);
                    }
                }
            }
        }
    }

    // Extract parent_directory from target path
    let parent_directory = context
        .target
        .path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // Create directory summary from context
    let directory_summary = DirectorySummary {
        parent_directory: parent_directory.clone(),
        child_count: context.directory_context.entries.len(),
    };

    // Extract downloader from xattr provenance
    let downloader_opt = context
        .evidence
        .iter()
        .find(|item| {
            item.source == crate::models::file_context::EvidenceSource::Xattr
                && item.key == "com.apple.FinderInfo"
        })
        .and_then(|item| {
            item.value.as_str().and_then(|value| {
                if value.is_empty() {
                    None
                } else {
                    parse_agent(value)
                }
            })
        });

    let downloader = downloader_opt;

    // Create normalized context
    let target = context.target;
    Ok(NormalizedFileContext {
        target: target.clone(),
        semantic_signals,
        provenance: Provenance {
            downloader,
            url: None,
        },
        directory_summary,
        file_type: metadata_result.file_type,
        creation_date: metadata_result.created_date,
        modification_date: target.modified_at.clone(),
        size_bytes: target.size_bytes,
        parent_directory,
        candidate_destinations: vec![],
    })
}

/// Extracts the downloader agent name from FinderInfo value.
/// Example input: "0081;com.apple.coreduoethylamine;Firefox"
/// Returns the agent name if found.
fn parse_agent(input: &str) -> Option<String> {
    let agents = ["Firefox", "Chrome", "Safari", "Edge", "Word"];
    for &agent in &agents {
        if input.contains(agent) {
            return Some(agent.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests;

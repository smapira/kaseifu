// Normalize raw evidence into clean semantic signals.

mod filename;
pub mod image;
mod metadata;
pub mod noise;
pub mod provenance;
pub mod xattr;

use crate::models::file_context::{
    DirectorySummary, FileContext, NormalizedFileContext, Provenance,
};

/// Normalize a file context from Phase 1 to Phase 2 normalized format
pub fn normalize(context: FileContext) -> anyhow::Result<NormalizedFileContext> {
    normalize_with_options(
        context,
        &crate::context::image_metadata::MetadataOptions::default(),
    )
}

/// Normalize with explicit image extraction toggles (v3.2 `[metadata.image]`).
///
/// Default [`normalize`] enables both Spotlight and EXIF fallback; callers
/// with resolved user configuration pass the user's toggles here so
/// `enabled = false` / `spotlight = false` / `exif_fallback = false` are
/// honored without re-reading config inside the pipeline.
pub fn normalize_with_options(
    context: FileContext,
    image_options: &crate::context::image_metadata::MetadataOptions,
) -> anyhow::Result<NormalizedFileContext> {
    // Extract semantic signals from metadata (M:, K:, etc. labels)
    let metadata_result = metadata::normalize_metadata(&context.evidence);

    // Optionally combine with xattr provenance signals
    // Optionally add xattr signals (e.g., downloader agent)
    let mut semantic_signals = metadata_result.semantic_signals;
    for item in context.evidence.iter() {
        if item.source == crate::models::file_context::EvidenceSource::Xattr {
            let key = item.key.as_str();
            if let Some(value_str) = item.value.as_str()
                && (key.starts_with("FinderInfo") || value_str.contains("Firefox"))
                && !value_str.is_empty()
            {
                let signaling_output = format!("X:{}", key);
                semantic_signals.push(signaling_output);
            }
        }
    }

    // Extract filename-based signals (F: prefix)
    // This ensures the filename itself is used as a search term,
    // even when Spotlight metadata lacks a title (kMDItemTitle).
    let stem = context
        .target
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if !stem.is_empty() {
        // Split camelCase and PascalCase: "AGENTS" -> ["AGENTS"], "CamelCase" -> ["Camel", "Case"]
        for term in split_case(&stem) {
            if !term.is_empty() {
                semantic_signals.push(format!("F:{}", term));
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
    // v3.2: image metadata rides along (Spotlight first, EXIF fallback).
    // File mtime is never treated as capture time; zones are never guessed.
    // v0.4.1: image availability markers are NOT semantic signals here —
    // Discovery would consume them as mdfind search terms. The Jev adapter
    // derives them from `image_metadata` instead (single source of truth).
    let image_metadata = crate::context::image_metadata::collect(
        &context.target.path,
        &context.evidence,
        image_options,
    )
    .and_then(crate::normalize::image::validate);
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
        image_metadata,
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

/// Split a string on PascalCase/camelCase boundaries.
/// "CamelCase" -> ["Camel", "Case"], "AGENTS" -> ["AGENTS"], "hello_world" -> ["hello", "world"]
fn split_case(input: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            if !current.is_empty() {
                terms.push(current.clone());
                current.clear();
            }
            continue;
        }

        if ch.is_uppercase() {
            // Check if this starts a new word
            if !current.is_empty() && current.chars().last().is_some_and(|c| c.is_lowercase()) {
                terms.push(current.clone());
                current.clear();
            }
            current.push(ch);
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        terms.push(current);
    }

    terms
}

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

use crate::models::file_context::{Evidence, EvidenceSource};

const ATTRIBUTES: &[&str] = &[
    "kMDItemContentType",
    "kMDItemKind",
    "kMDItemTitle",
    "kMDItemAuthors",
    "kMDItemKeywords",
    "kMDItemWhereFroms",
    "kMDItemFSCreationDate",
    "kMDItemFSContentChangeDate",
];

/// v3.2 P0 image attributes (Spotlight is the first source).
/// Presence depends on file format and index state; absence is normal.
pub const IMAGE_ATTRIBUTES: &[&str] = &[
    "kMDItemEXIFDateTimeOriginal",
    "kMDItemContentCreationDate",
    "kMDItemLatitude",
    "kMDItemLongitude",
    "kMDItemAltitude",
    "kMDItemAcquisitionMake",
    "kMDItemAcquisitionModel",
    "kMDItemPixelWidth",
    "kMDItemPixelHeight",
];

pub fn metadata(path: &Path) -> Result<Vec<Evidence>> {
    let mut names: Vec<&str> = Vec::new();
    names.extend_from_slice(ATTRIBUTES);
    names.extend_from_slice(IMAGE_ATTRIBUTES);
    fetch_attributes(path, &names)
}

/// Fetch image-only attributes (v3.2 first source).
///
/// Returns evidence for attributes that exist; `(null)` / missing values are
/// skipped without fabrication. A failed `mdls` call yields no evidence but
/// is not an error for the caller (classification continues).
pub fn image_metadata(path: &Path) -> Result<Vec<Evidence>> {
    fetch_attributes(path, IMAGE_ATTRIBUTES)
}

/// One `mdls` invocation for all requested attributes.
///
/// A single process call avoids N round-trips for the same file.
fn fetch_attributes(path: &Path, attributes: &[&str]) -> Result<Vec<Evidence>> {
    let mut command = Command::new("mdls");
    for attribute in attributes {
        command.arg("-name").arg(attribute);
    }
    let output = command
        .arg(path)
        .output()
        .with_context(|| format!("failed to execute mdls for {}", path.display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_mdls_output(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse `mdls` multi-attribute output (`key = value` per line).
///
/// `(null)` and empty values are skipped so null / missing / failure stay
/// distinguishable downstream (absent evidence means absent value).
fn parse_mdls_output(text: &str) -> Vec<Evidence> {
    let mut evidence = Vec::new();
    for (key, raw) in split_mdls_lines(text) {
        let value = normalize_mdls_value(&raw);
        if value.is_empty() || value == "(null)" {
            continue;
        }
        evidence.push(Evidence {
            source: EvidenceSource::SpotlightMeta,
            key: key.clone(),
            value: Value::String(value),
            attribute: Some(key),
        });
    }
    evidence
}

/// Split `mdls` output into `(key, raw value)` pairs.
fn split_mdls_lines(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in text.lines() {
        if let Some((key, raw)) = split_mdls_line(line) {
            pairs.push((key, raw));
        }
    }
    pairs
}

/// Split one `key = value` line; returns `None` for continuation lines.
fn split_mdls_line(line: &str) -> Option<(String, String)> {
    let (key, raw) = line.split_once('=')?;
    let key = key.trim().to_string();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    Some((key, raw.trim().to_string()))
}

/// Unquote `mdls` string values (`"public.jpeg"` -> `public.jpeg`).
fn normalize_mdls_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn search(query: &str, limit: usize) -> Result<Vec<String>> {
    let output = Command::new("mdfind")
        .arg(query)
        .output()
        .context("failed to execute mdfind")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(stdout.lines().take(limit).map(ToOwned::to_owned).collect())
}

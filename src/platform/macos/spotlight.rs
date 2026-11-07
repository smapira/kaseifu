use anyhow::{Context, Result};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
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
    search_in(query, limit, &default_roots())
}

/// The roots a search is scoped to when the caller names none.
///
/// The user's home directory: an index also holds `/System`, `/Applications`
/// and every mounted volume, and a file that lives in the home is never best
/// matched by a path outside it (measured: a picture in `~/Desktop` ranked a
/// repository checkout first, issue #19). An empty result here means the home
/// could not be resolved; the search then runs unscoped, exactly as before.
pub fn default_roots() -> Vec<PathBuf> {
    dirs::home_dir().into_iter().collect()
}

/// The `mdfind` argument vector for one query scoped to `roots`.
///
/// `mdfind` takes one `-onlyin` scope per invocation, so a multi-root search
/// runs once per root and merges (see [`search_in`]). The vector is built by a
/// pure function so the scope can be asserted without starting a process.
pub fn search_arguments(query: &str, root: Option<&Path>) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = Vec::new();
    if let Some(root) = root {
        arguments.push(OsString::from("-onlyin"));
        arguments.push(root.as_os_str().to_owned());
    }
    arguments.push(OsString::from(query));
    arguments
}

/// [`search`] scoped to explicit roots.
///
/// One `mdfind` call per root, results concatenated in root order and bounded
/// by `limit` across all of them. An empty `roots` falls back to
/// [`default_roots`]; when even that is empty the query runs unscoped (the
/// historical behavior) rather than returning nothing.
pub fn search_in(query: &str, limit: usize, roots: &[PathBuf]) -> Result<Vec<String>> {
    search_in_with(query, limit, roots, run_mdfind)
}

/// [`search_in`] with an injectable runner (`query`, scope, limit).
///
/// The seam exists so the scope can be asserted without starting `mdfind`: the
/// acceptance for issue #19 is that the *invocation* is scoped, and a test that
/// ran the real command would read the developer's index.
pub fn search_in_with<F>(
    query: &str,
    limit: usize,
    roots: &[PathBuf],
    run: F,
) -> Result<Vec<String>>
where
    F: Fn(&str, Option<&Path>, usize) -> Result<Vec<String>>,
{
    let scopes: Vec<PathBuf> = if roots.is_empty() {
        default_roots()
    } else {
        roots.to_vec()
    };
    if scopes.is_empty() {
        return run(query, None, limit);
    }
    let mut hits: Vec<String> = Vec::new();
    for scope in &scopes {
        if hits.len() >= limit {
            break;
        }
        let found = run(query, Some(scope), limit.saturating_sub(hits.len()))?;
        hits.extend(found);
    }
    Ok(hits)
}

/// One `mdfind` invocation, optionally scoped to one root.
fn run_mdfind(query: &str, root: Option<&Path>, limit: usize) -> Result<Vec<String>> {
    let output = Command::new("mdfind")
        .args(search_arguments(query, root))
        .output()
        .context("failed to execute mdfind")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(stdout.lines().take(limit).map(ToOwned::to_owned).collect())
}

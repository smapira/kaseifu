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

pub fn metadata(path: &Path) -> Result<Vec<Evidence>> {
    let mut evidence = Vec::new();

    for attribute in ATTRIBUTES {
        let output = Command::new("mdls")
            .arg("-raw")
            .arg("-name")
            .arg(attribute)
            .arg(path)
            .output()
            .with_context(|| format!("failed to execute mdls for {}", path.display()))?;

        if !output.status.success() {
            continue;
        }

        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if value.is_empty() || value == "(null)" {
            continue;
        }

        evidence.push(Evidence {
            source: EvidenceSource::SpotlightMeta,
            key: attribute.to_string(),
            value: Value::String(value),
            attribute: Some(attribute.to_string()),
        });
    }

    Ok(evidence)
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

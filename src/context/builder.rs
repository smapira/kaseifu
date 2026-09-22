use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::fs;
use std::path::Path;

use crate::models::file_context::{DirectoryContext, FileContext, TargetFile};

use crate::platform::macos::{filesystem, spotlight, xattr};

pub fn build(path: &Path) -> Result<FileContext> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", path.display()))?;

    let metadata = fs::metadata(&canonical)?;

    let filename = canonical
        .file_name()
        .context("file has no filename")?
        .to_string_lossy()
        .into_owned();

    let extension = canonical
        .extension()
        .map(|v| v.to_string_lossy().into_owned());

    let modified_at = metadata.modified().ok().map(|time| {
        let dt: DateTime<Utc> = time.into();
        dt.to_rfc3339()
    });

    let current = canonical
        .parent()
        .context("target has no parent directory")?
        .to_path_buf();

    let parent = current.parent().map(|p| p.to_path_buf());

    let entries = filesystem::read_directory(&current, 100)?;

    let mut evidence = Vec::new();

    evidence.extend(spotlight::metadata(&canonical).unwrap_or_default());

    evidence.extend(xattr::collect(&canonical).unwrap_or_default());

    Ok(FileContext {
        schema_version: "file-context-v1".into(),

        target: TargetFile {
            path: canonical,
            filename,
            extension,
            size_bytes: metadata.len(),
            modified_at,
        },

        directory_context: DirectoryContext {
            current,
            parent,
            entries,
        },

        evidence,
    })
}

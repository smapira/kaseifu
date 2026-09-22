use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::models::file_context::{DirectoryEntry, EntryKind};

pub fn read_directory(path: &Path, max_entries: usize) -> Result<Vec<DirectoryEntry>> {
    let mut result = Vec::new();

    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .take(max_entries)
    {
        let entry = entry?;
        let file_type = entry.file_type()?;

        let kind = if file_type.is_file() {
            EntryKind::File
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        };

        result.push(DirectoryEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind,
        });
    }

    Ok(result)
}

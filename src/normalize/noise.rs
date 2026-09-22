use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectorySummary {
    pub total_entries: usize,
    pub file_count: usize,
    pub directory_count: usize,
    pub counts_map: std::collections::HashMap<String, usize>,
}

impl Default for DirectorySummary {
    fn default() -> Self {
        Self {
            total_entries: 0,
            file_count: 0,
            directory_count: 0,
            counts_map: std::collections::HashMap::new(),
        }
    }
}

impl DirectorySummary {
    pub fn new(entries: &[crate::models::file_context::DirectoryEntry]) -> DirectorySummary {
        let mut map = std::collections::HashMap::new();
        let mut file_count = 0;
        let mut directory_count = 0;

        for entry in entries {
            match entry.kind {
                crate::models::file_context::EntryKind::File => {
                    file_count += 1;
                    *map.entry("file".to_string()).or_insert(0) += 1;
                }
                crate::models::file_context::EntryKind::Directory => {
                    directory_count += 1;
                    *map.entry("directory".to_string()).or_insert(0) += 1;
                }
                _ => {}
            }
        }

        DirectorySummary {
            total_entries: entries.len(),
            file_count,
            directory_count,
            counts_map: map,
        }
    }
}

pub fn directory_summary(
    entries: &[crate::models::file_context::DirectoryEntry],
) -> DirectorySummary {
    DirectorySummary::new(entries)
}

pub fn is_noise_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    let known_noises = [
        "ds_store",
        ".localized",
        "node_modules",
        "target",
        "thumbs.db",
    ];
    known_noises
        .iter()
        .any(|&n| lower == n || lower.ends_with(&n))
}

pub fn noise_filter(
    entries: &[crate::models::file_context::DirectoryEntry],
) -> Vec<crate::models::file_context::DirectoryEntry> {
    entries
        .iter()
        .filter(|e| !is_noise_file(&e.name))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoiseType {
    System,
    Temporary,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoiseFilter {
    Exclude { noise: NoiseType },
    IncludeAll,
}

impl Default for NoiseFilter {
    fn default() -> Self {
        Self::IncludeAll
    }
}

impl From<NoiseFilter> for bool {
    fn from(_filter: NoiseFilter) -> Self {
        true
    }
}

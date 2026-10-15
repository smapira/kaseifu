use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectorySummary {
    pub total_entries: usize,
    pub file_count: usize,
    pub directory_count: usize,
    pub counts_map: std::collections::HashMap<String, usize>,
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

/// True when `filename` is a macOS or build-tool noise name.
///
/// The comparison is an exact, case-insensitive match against the known list:
/// a partial (`ends_with`) match silently dropped user folders such as
/// `my-target` or `dist-target` from folder evidence and batch renames. The
/// dotted `.DS_Store` form is listed explicitly so the exact match still
/// catches the file the Finder writes.
pub fn is_noise_file(filename: &str) -> bool {
    matches!(
        filename.to_lowercase().as_str(),
        "ds_store" | ".ds_store" | ".localized" | "node_modules" | "target" | "thumbs.db"
    )
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NoiseFilter {
    Exclude {
        noise: NoiseType,
    },
    #[default]
    IncludeAll,
}

impl From<NoiseFilter> for bool {
    fn from(_filter: NoiseFilter) -> Self {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_noise_names_are_excluded() {
        for name in [
            ".DS_Store",
            ".ds_store",
            "ds_store",
            ".localized",
            "node_modules",
            "Node_Modules",
            "target",
            "Target",
            "TARGET",
            "Thumbs.db",
            "THUMBS.DB",
        ] {
            assert!(is_noise_file(name), "{name} must be noise");
        }
    }

    #[test]
    fn partial_name_matches_are_kept() {
        // A partial match used to drop these silently from folder evidence
        // and batch renames; only the exact names above are noise.
        for name in [
            "my-target",
            "dist-target",
            "my-ds_store-backup",
            "mytarget",
            "targets",
            "my-node_modules",
            ".localized-backup",
            "invoice.pdf",
            "note.txt",
            ".git",
        ] {
            assert!(!is_noise_file(name), "{name} must not be noise");
        }
    }
}

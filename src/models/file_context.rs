use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileContext {
    pub schema_version: String,
    pub target: TargetFile,
    pub directory_context: DirectoryContext,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetFile {
    pub path: PathBuf,
    pub filename: String,
    pub extension: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectoryContext {
    pub current: PathBuf,
    pub parent: Option<PathBuf>,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    #[default]
    Other,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source: EvidenceSource,
    pub key: String,
    pub value: serde_json::Value,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceSource {
    Fs,
    Dir,
    SpotlightMeta,
    SpotlightSearch,
    Xattr,
    Content,
    Derived,
}

// ============ PHASE 3: NORMALIZED MODEL ============
// NormalizedFileContext - output of Normalize phase

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NormalizedFileContext {
    pub target: TargetFile,
    pub semantic_signals: Vec<String>,
    pub provenance: Provenance,
    pub directory_summary: DirectorySummary,
    pub file_type: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub size_bytes: u64,
    pub parent_directory: String,
    pub candidate_destinations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Provenance {
    pub downloader: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectorySummary {
    pub parent_directory: String,
    pub child_count: usize,
}

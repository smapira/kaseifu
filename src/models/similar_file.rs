use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Similar file found during discovery phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarFile {
    /// Full path to the similar file
    pub path: PathBuf,
    /// Parent directory that contains this similar file
    pub parent_directory: String,
    /// Terms that matched in this file's metadata/title
    pub matched_terms: Vec<String>,
    /// Similarity score (1.0 = exact match, 0.0 = no match)
    pub similarity_score: f64,
    /// Normalized filename similarity (0..=1). Default 0 for old JSON.
    #[serde(default)]
    pub filename_similarity: f64,
    /// Token-set similarity (0..=1). Default 0 for old JSON.
    #[serde(default)]
    pub token_similarity: f64,
    /// Extension match (1.0 or 0.0). Default 0 for old JSON.
    #[serde(default)]
    pub extension_match: f64,
}

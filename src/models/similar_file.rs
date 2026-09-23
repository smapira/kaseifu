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
}

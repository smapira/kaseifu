use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateDestination {
    pub id: String,
    pub path: PathBuf,
    pub score: f64,
    pub evidence_count: usize,
    pub reasons: Vec<String>,
}

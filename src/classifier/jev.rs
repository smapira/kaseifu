use serde::{Deserialize, Serialize};

use crate::models::candidate::CandidateDestination;
use crate::models::file_context::FileContext;

#[derive(Debug, Serialize)]
pub struct JevRequest<'a> {
    pub target: &'a FileContext,
    pub candidates: &'a [CandidateDestination],
}

#[derive(Debug, Deserialize)]
pub struct JevResponse {
    pub destination_id: Option<String>,
    pub confidence: f64,
    pub probabilities: std::collections::HashMap<String, f64>,
}

// Jev API integration will be implemented after
// FileContext and Candidate Discovery are validated locally.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateDestination {
    pub id: String,
    pub path: PathBuf,
    pub score: f64,
    pub evidence_count: usize,
    pub reasons: Vec<String>,
    /// Where this candidate came from.
    ///
    /// Historical candidates are [`CandidateOrigin::Spotlight`] (the default, so
    /// a report written before the field existed reads as one); a standard
    /// operating-system folder that discovery injected because the file's type
    /// belongs there is [`CandidateOrigin::Prior`] and carries no similarity
    /// score of its own (issue #18). The field is additive in JSON and in the
    /// classifier payload: the `reasons` list already says `prior` in words.
    #[serde(default)]
    pub origin: CandidateOrigin,
}

/// Which evidence produced a candidate destination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateOrigin {
    /// Rows of Spotlight evidence aggregated by directory.
    #[default]
    Spotlight,
    /// An operating-system standard folder for the file's own type.
    Prior,
}

impl CandidateOrigin {
    /// True for the historical origin, so a payload can omit it.
    pub fn is_spotlight(&self) -> bool {
        matches!(self, Self::Spotlight)
    }
}

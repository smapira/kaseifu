pub fn normalized_provenance(_evidence: &[crate::models::file_context::Evidence]) -> Provenance {
    Provenance::default()
}

#[derive(Debug, Clone, Default)]
pub struct Provenance {
    pub tags: Vec<String>,
}

use crate::models::file_context::{Evidence, EvidenceSource};

pub fn derived(key: impl Into<String>, value: serde_json::Value) -> Evidence {
    Evidence {
        source: EvidenceSource::Derived,
        key: key.into(),
        value,
        attribute: None,
    }
}

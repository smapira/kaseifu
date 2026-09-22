use anyhow::Result;
use serde_json::Value;
use std::path::Path;

use crate::models::file_context::{Evidence, EvidenceSource};

pub fn collect(path: &Path) -> Result<Vec<Evidence>> {
    let mut evidence = Vec::new();

    for name in xattr::list(path)? {
        let name_string = name.to_string_lossy().into_owned();

        if let Some(value) = xattr::get(path, &name)? {
            evidence.push(Evidence {
                source: EvidenceSource::Xattr,
                key: name_string,
                value: Value::String(String::from_utf8_lossy(&value).into_owned()),
                attribute: None,
            });
        }
    }

    Ok(evidence)
}

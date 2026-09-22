mod filename;
mod metadata;
mod noise;
pub mod provenance;
pub mod xattr;

pub use filename::normalize_filename;
pub use metadata::normalize_metadata;
pub use noise::{directory_summary, is_noise_file, noise_filter};
pub use provenance::Provenance;
pub use xattr::{XATTR_ALLOWLIST, normalize_xattr};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NormalizedTarget {
    pub filename: String,
    pub file_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NormalizedFileContext {
    pub target: NormalizedTarget,
    pub semantic_signals: Vec<String>,
    pub provenance: Provenance,
    pub directory_summary: noise::DirectorySummary,
}

impl NormalizedFileContext {
    pub fn build(context: &crate::models::file_context::FileContext) -> Result<Self> {
        let mut signals = Vec::new();

        let filename = normalize_filename(&context.target.filename);
        for term in &filename {
            if !term.is_empty() {
                signals.push(term.clone());
            }
        }

        let metadata = normalize_metadata(&context.evidence).unwrap_or_default();
        for term in &metadata.terms {
            if !term.is_empty() {
                signals.push(term.clone());
            }
        }

        signals.extend(metadata.file_type.iter().filter(|t| !t.is_empty()));
        signals.extend(metadata.authors.iter().filter(|t| !t.is_empty()));

        let xattr = normalize_xattr(&context.evidence).unwrap_or_default();
        if let Some(ref agent) = xattr.download_agent {
            signals.push(agent.clone());
        }
        signals.extend(xattr.urls);

        let summary = directory_summary(&context.directory_context.entries);

        Ok(NormalizedFileContext {
            target: NormalizedTarget {
                filename: context.target.filename.clone(),
                file_type: context.target.extension.clone(),
            },
            semantic_signals: signals,
            provenance: Provenance::default(),
            directory_summary: summary,
        })
    }
}

impl From<&crate::models::file_context::FileContext> for NormalizedFileContext {
            target: Self {
                filename: String::new(),
                file_type: None,
            },
            semantic_signals: vec![],
            provenance: Provenance::default(),
            directory_summary: noise::DirectorySummary::default(),
        })
    }
}

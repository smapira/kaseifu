//! Developer smoke for the Foundation Models bridge.
//!
//! ```text
//! cargo run --example fm_bridge_smoke
//! ```
//!
//! It prints which bridge this build linked, the model availability (with its
//! reason), and one full semantic request through the provider. On a machine
//! with Apple Intelligence enabled that request is a real on-device inference;
//! anywhere else it prints the explicit reason instead. Nothing is written to
//! the filesystem and no path is opened: the observation is synthetic.
//!
//! This is a developer tool: `cargo install` does not ship examples.

use std::time::Instant;

use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};
use kaseifu::semantic::{BRIDGE_KIND, FoundationModelsProvider, STUB_REASON, SemanticProvider};

fn synthetic_context() -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: "/tmp/2026-Q4-budget-draft.pdf".into(),
            filename: "2026-Q4-budget-draft.pdf".to_string(),
            extension: Some("pdf".to_string()),
            size_bytes: 84_000,
            modified_at: None,
        },
        parent_directory: "/tmp".to_string(),
        size_bytes: 84_000,
        semantic_signals: vec!["F:budget".to_string(), "F:Q4".to_string()],
        ..NormalizedFileContext::default()
    }
}

#[tokio::main]
async fn main() {
    println!("bridge: {BRIDGE_KIND}");
    if let Some(reason) = STUB_REASON {
        println!("stub reason: {reason} (no Swift bridge was built)");
    }

    let provider = FoundationModelsProvider::production();
    match provider.availability() {
        Ok(availability) => println!("availability: {}", availability.label()),
        Err(error) => {
            println!("availability: error: {error}");
            return;
        }
    }

    let started = Instant::now();
    match provider.analyze(&synthetic_context()).await {
        Ok(info) => println!(
            "semantic: title={:?} category={:?} project={:?} ({} ms)",
            info.title,
            info.category,
            info.project,
            started.elapsed().as_millis()
        ),
        Err(error) => println!(
            "semantic error: {error} ({} ms; skippable: {})",
            started.elapsed().as_millis(),
            error.is_skippable()
        ),
    }
}

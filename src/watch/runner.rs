//! Phase 6 - the watch loop: event coordinator + the existing pipeline.
//!
//! No watcher-specific classification, decision, or move logic lives here:
//! every request goes through `operations::organize`.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use crate::classifier::jev::JevIntegration;
use crate::models::decision::DecisionAction;
use crate::operations::organize::{OrganizeOutcome, organize_file_with};
use crate::operations::plan::PlanError;
use crate::storage::history::OperationHistory;
use crate::watch::coordinator::{
    Disposition, EventCoordinator, ProcessingRequest, RawEvent, SuppressReason, WatchConfig,
};

/// What happened to one processing request (for logging).
#[derive(Debug)]
pub enum ProcessOutcome {
    Moved {
        operation_id: String,
        destination: PathBuf,
    },
    WouldMove {
        destination: PathBuf,
    },
    /// Review / Leave / NONE: nothing was moved.
    Refused {
        policy: DecisionAction,
        candidate_path: Option<PathBuf>,
        confidence: f64,
        margin: f64,
    },
    Unplanable(PlanError),
    /// The file could not be observed/classified; the watcher continues.
    Failed(String),
}

/// Process one request through the existing pipeline.
pub async fn process_request(
    request: &ProcessingRequest,
    coordinator: &mut EventCoordinator,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
) -> ProcessOutcome {
    process_request_with(
        request,
        coordinator,
        jev,
        history,
        execute,
        crate::policy::decision::Thresholds::default(),
    )
    .await
}

/// Process one request with explicit policy thresholds (user TOML overrides).
pub async fn process_request_with(
    request: &ProcessingRequest,
    coordinator: &mut EventCoordinator,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    thresholds: crate::policy::decision::Thresholds,
) -> ProcessOutcome {
    // Deduplication: the path is locked for the duration of processing.
    coordinator.begin_processing(&request.path);

    let outcome = match organize_file_with(&request.path, jev, history, execute, thresholds).await {
        Ok(report) => match report.outcome {
            OrganizeOutcome::Moved {
                operation_id,
                destination,
            } => {
                // Feedback-loop protection: the move we just performed will
                // produce events for both paths; suppress them.
                coordinator.record_self_operation(
                    &[request.path.clone(), destination.clone()],
                    Instant::now(),
                );
                ProcessOutcome::Moved {
                    operation_id,
                    destination,
                }
            }
            OrganizeOutcome::WouldMove { destination } => ProcessOutcome::WouldMove { destination },
            OrganizeOutcome::Refused => ProcessOutcome::Refused {
                policy: report.policy,
                candidate_path: report.candidate_path,
                confidence: report.decision.confidence,
                margin: report.decision.margin,
            },
            OrganizeOutcome::Unplanable(error) => ProcessOutcome::Unplanable(error),
            OrganizeOutcome::MoveFailed(error) => ProcessOutcome::Failed(error),
        },
        Err(error) => ProcessOutcome::Failed(error.to_string()),
    };

    coordinator.finish_processing(&request.path);
    outcome
}

/// Log one outcome (Review/Leave are surfaced in the log; no GUI in Phase 6).
pub fn log_outcome(path: &Path, outcome: &ProcessOutcome) {
    match outcome {
        ProcessOutcome::Moved {
            operation_id,
            destination,
        } => println!(
            "moved   {} -> {}\n        operation: {operation_id}",
            path.display(),
            destination.display()
        ),
        ProcessOutcome::WouldMove { destination } => println!(
            "dry-run {} (would move to {})",
            path.display(),
            destination.display()
        ),
        ProcessOutcome::Refused {
            policy,
            candidate_path,
            confidence,
            margin,
        } => {
            // Phase 6 surfaces Review for the user; no notification system yet.
            println!("review  {}", path.display());
            println!("        policy:     {policy:?}");
            println!(
                "        candidate:  {}",
                candidate_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(none)".to_string())
            );
            println!("        confidence: {confidence:.3}");
            println!("        margin:     {margin:.3}");
        }
        ProcessOutcome::Unplanable(error) => {
            println!("no move {}\n        {error:?}", path.display())
        }
        ProcessOutcome::Failed(error) => println!("error   {}\n        {error}", path.display()),
    }
}

/// Consume an event source until the process is stopped.
///
/// A failing file never stops the loop (errors are logged per file).
pub async fn run_loop(
    events: Receiver<PathBuf>,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    config: WatchConfig,
    thresholds: crate::policy::decision::Thresholds,
) -> Result<()> {
    let mut coordinator = EventCoordinator::new(config);

    loop {
        while let Ok(path) = events.try_recv() {
            let disposition = coordinator.submit(RawEvent { path: path.clone() }, Instant::now());
            if let Disposition::Suppressed(reason) = disposition {
                println!(
                    "ignored {} ({})",
                    path.display(),
                    describe_suppression(&reason)
                );
            }
        }

        // Only stable, debounced, not-yet-processing paths reach the pipeline,
        // so the Jev API is never called for a rejected event.
        for request in coordinator.drain_due(Instant::now()) {
            println!("detected {}", request.path.display());
            let outcome = process_request_with(
                &request,
                &mut coordinator,
                jev,
                history,
                execute,
                thresholds,
            )
            .await;
            log_outcome(&request.path, &outcome);
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Human-readable suppression reason.
pub fn describe_suppression(reason: &SuppressReason) -> &'static str {
    match reason {
        SuppressReason::Directory => "directory",
        SuppressReason::Missing => "missing",
        SuppressReason::Temporary => "temporary",
        SuppressReason::ManagedData => "managed data",
        SuppressReason::DuplicateInFlight => "already processing",
        SuppressReason::SelfGenerated => "self-generated",
    }
}

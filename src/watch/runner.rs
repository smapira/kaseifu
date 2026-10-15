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
use crate::operations::organize::{OrganizeOutcome, organize_file_with_full};
use crate::operations::plan::PlanError;
use crate::presentation::renderers::generic;
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
    process_request_with_full(
        request,
        coordinator,
        jev,
        history,
        execute,
        thresholds,
        &crate::context::image_metadata::MetadataOptions::default(),
    )
    .await
}

/// [`process_request_with`] with explicit image toggles (v3.2).
pub async fn process_request_with_full(
    request: &ProcessingRequest,
    coordinator: &mut EventCoordinator,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    thresholds: crate::policy::decision::Thresholds,
    image_options: &crate::context::image_metadata::MetadataOptions,
) -> ProcessOutcome {
    // Deduplication: the path is locked for the duration of processing.
    coordinator.begin_processing(&request.path);

    let outcome = match organize_file_with_full(
        &request.path,
        jev,
        history,
        execute,
        thresholds,
        image_options,
    )
    .await
    {
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
///
/// With `json` every outcome is one JSON object on its own line (JSON Lines);
/// otherwise the historical text line is printed unchanged.
pub fn log_outcome(path: &Path, outcome: &ProcessOutcome, json: bool) {
    if json {
        println!("{}", outcome_json(path, outcome));
    } else {
        print!("{}", outcome_text(path, outcome));
    }
}

/// The log line(s) for one outcome.
///
/// The mode vocabulary is shared with the other commands (ZC-9): a dry run
/// says `dry run (no files moved)`, never a raw boolean or a backwards
/// `dry-run` tag, and an executed move stays `moved`.
pub fn outcome_text(path: &Path, outcome: &ProcessOutcome) -> String {
    match outcome {
        ProcessOutcome::Moved {
            operation_id,
            destination,
        } => format!(
            "moved   {} -> {}\n        operation: {operation_id}\n",
            path.display(),
            destination.display()
        ),
        ProcessOutcome::WouldMove { destination } => format!(
            "would move {} -> {} ({})\n",
            path.display(),
            destination.display(),
            generic::dry_run_banner(false, false)
        ),
        ProcessOutcome::Refused {
            policy,
            candidate_path,
            confidence,
            margin,
        } => format!(
            "review  {}\n        policy:     {policy:?}\n        candidate:  {}\n        confidence: {confidence:.3}\n        margin:     {margin:.3}\n",
            path.display(),
            candidate_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "(none)".to_string())
        ),
        ProcessOutcome::Unplanable(error) => {
            format!("no move {}\n        {error:?}\n", path.display())
        }
        ProcessOutcome::Failed(error) => {
            format!("error   {}\n        {error}\n", path.display())
        }
    }
}

/// One outcome as one JSON object for `--format json` (see
/// [`generic::watch_start_json`]): the `event` names what happened, `path`
/// is the observed file, and `detail` carries what the text line would show
/// (the destination, the operation id, the refusal numbers).
pub fn outcome_json(path: &Path, outcome: &ProcessOutcome) -> String {
    let path = path.display().to_string();
    let (event, detail) = match outcome {
        ProcessOutcome::Moved {
            operation_id,
            destination,
        } => (
            "moved",
            format!(
                "{} -> {} (operation: {operation_id})",
                path,
                destination.display()
            ),
        ),
        ProcessOutcome::WouldMove { destination } => (
            "would-move",
            format!("{} -> {}", path, destination.display()),
        ),
        ProcessOutcome::Refused {
            policy,
            candidate_path,
            confidence,
            margin,
        } => (
            "refused",
            format!(
                "policy: {policy:?}; candidate: {}; confidence: {confidence:.3}; margin: {margin:.3}",
                candidate_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(none)".to_string())
            ),
        ),
        ProcessOutcome::Unplanable(error) => ("unplanable", format!("{error:?}")),
        ProcessOutcome::Failed(error) => ("failed", error.clone()),
    };
    generic::watch_event_json(event, &path, Some(&detail))
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
    run_loop_with(
        events,
        jev,
        history,
        execute,
        config,
        thresholds,
        WatchRunOptions {
            image_options: &crate::context::image_metadata::MetadataOptions::default(),
            json: false,
        },
    )
    .await
}

/// Options only [`run_loop_with`] needs on top of [`run_loop`]: the image
/// toggles and the report shape (text lines or JSON Lines).
#[derive(Debug, Clone, Copy)]
pub struct WatchRunOptions<'a> {
    /// Image metadata toggles forwarded to the pipeline.
    pub image_options: &'a crate::context::image_metadata::MetadataOptions,
    /// Print one JSON object per line instead of the text lines.
    pub json: bool,
}

/// [`run_loop`] with explicit image toggles (v3.2).
///
/// With `options.json` every loop line is one JSON object (JSON Lines);
/// otherwise the historical text lines are printed unchanged.
pub async fn run_loop_with(
    events: Receiver<PathBuf>,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    config: WatchConfig,
    thresholds: crate::policy::decision::Thresholds,
    options: WatchRunOptions<'_>,
) -> Result<()> {
    let mut coordinator = EventCoordinator::new(config);
    let json = options.json;
    let image_options = options.image_options;

    loop {
        while let Ok(path) = events.try_recv() {
            let disposition = coordinator.submit(RawEvent { path: path.clone() }, Instant::now());
            if let Disposition::Suppressed(reason) = disposition {
                if json {
                    println!(
                        "{}",
                        generic::watch_event_json(
                            "ignored",
                            &path.display().to_string(),
                            Some(describe_suppression(&reason))
                        )
                    );
                } else {
                    println!(
                        "ignored {} ({})",
                        path.display(),
                        describe_suppression(&reason)
                    );
                }
            }
        }

        // Only stable, debounced, not-yet-processing paths reach the pipeline,
        // so the Jev API is never called for a rejected event.
        for request in coordinator.drain_due(Instant::now()) {
            if json {
                println!(
                    "{}",
                    generic::watch_event_json(
                        "detected",
                        &request.path.display().to_string(),
                        None
                    )
                );
            } else {
                println!("detected {}", request.path.display());
            }
            let outcome = process_request_with_full(
                &request,
                &mut coordinator,
                jev,
                history,
                execute,
                thresholds,
                image_options,
            )
            .await;
            log_outcome(&request.path, &outcome, json);
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

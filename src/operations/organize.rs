//! Phase 5/6 - end-to-end organization of a single file.
//!
//! One shared entry point used by the CLI and by the watcher. It only
//! coordinates existing components; it never re-implements them.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::classifier::jev::JevIntegration;
use crate::classifier::pipeline;
use crate::models::decision::{DecisionAction, JevDecision};
use crate::operations::mover::{self, MoveOutcome};
use crate::operations::plan::{self, PlanError};
use crate::policy;
use crate::storage::history::OperationHistory;

/// Everything the caller needs to log or react to.
#[derive(Debug)]
pub struct OrganizeReport {
    pub decision: JevDecision,
    /// The Phase 5 execution policy for this decision.
    pub policy: DecisionAction,
    /// Destination directory of the selected candidate (from discovery).
    pub candidate_path: Option<PathBuf>,
    pub outcome: OrganizeOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrganizeOutcome {
    Moved {
        operation_id: String,
        destination: PathBuf,
    },
    WouldMove {
        destination: PathBuf,
    },
    /// Policy refused execution (Review / Leave / NONE): no mutation.
    Refused,
    /// Policy allowed a move, but no plan could be built.
    Unplanable(PlanError),
    /// The move itself failed; the source is preserved.
    MoveFailed(String),
}

/// Observe -> Normalize -> Discover -> Jev -> Policy -> Plan -> Execute.
pub async fn organize_file(
    file: &Path,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
) -> Result<OrganizeReport> {
    organize_file_with(
        file,
        jev,
        history,
        execute,
        policy::decision::Thresholds::default(),
    )
    .await
}

/// Observe -> Normalize -> Discover -> Jev -> Policy -> Plan -> Execute.
///
/// Uses explicit policy thresholds (user TOML overrides) for both the policy
/// verdict and the move-plan gate, so the two always agree.
pub async fn organize_file_with(
    file: &Path,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    thresholds: policy::decision::Thresholds,
) -> Result<OrganizeReport> {
    // Phase 4: decision (the candidate list is needed to resolve the path).
    let (decision, candidates) = pipeline::classify_with_candidates(file, jev).await?;

    // Phase 5: policy.
    let action = policy::decision::evaluate_with(&decision, thresholds);

    let candidate_path = decision
        .destination_id
        .as_deref()
        .and_then(|id| candidates.iter().find(|candidate| candidate.id == id))
        .map(|candidate| candidate.path.clone());

    // Phase 5: plan + safe execution.
    let outcome = match plan::build_move_plan_with(file, &decision, &candidates, thresholds) {
        Ok(plan) => {
            let planned_destination = plan.destination_path.clone();

            if execute {
                match mover::execute_move(&plan, history) {
                    Ok(MoveOutcome::Moved { operation_id }) => OrganizeOutcome::Moved {
                        operation_id,
                        destination: planned_destination,
                    },
                    Ok(MoveOutcome::WouldMove) => OrganizeOutcome::WouldMove {
                        destination: planned_destination,
                    },
                    Err(error) => OrganizeOutcome::MoveFailed(format!("{error:?}")),
                }
            } else {
                match mover::dry_run(&plan) {
                    Ok(_) => OrganizeOutcome::WouldMove {
                        destination: planned_destination,
                    },
                    Err(error) => OrganizeOutcome::MoveFailed(format!("{error:?}")),
                }
            }
        }
        Err(error) => OrganizeOutcome::Unplanable(error),
    };

    Ok(OrganizeReport {
        decision,
        policy: action,
        candidate_path,
        outcome,
    })
}

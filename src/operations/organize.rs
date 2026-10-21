//! Phase 5/6 - end-to-end organization of a single file.
//!
//! One shared entry point used by the CLI and by the watcher. It only
//! coordinates existing components; it never re-implements them.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::classifier::jev::JevIntegration;
use crate::classifier::keywords::KeywordDelivery;
use crate::classifier::pipeline;
use crate::models::decision::{DecisionAction, JevDecision};
use crate::operations::mover::{self, MoveOutcome};
use crate::operations::plan::{self, PlanError};
use crate::policy;
use crate::repo_policy::{self, RuleMatch};
use crate::storage::history::OperationHistory;

/// Everything the caller needs to log or react to.
#[derive(Debug)]
pub struct OrganizeReport {
    pub decision: JevDecision,
    /// The Phase 5 execution policy for this decision.
    pub policy: DecisionAction,
    /// Destination directory of the selected candidate (from discovery).
    pub candidate_path: Option<PathBuf>,
    /// Whether Spotlight knew the source file when the candidates were
    /// discovered.
    ///
    /// `Some(false)` means the verdict rests on an index that has not caught
    /// up with this file yet (it was created, copied or moved moments ago,
    /// or the path is one `mdfind` never returns), so the candidates may be
    /// incomplete. It is a display fact only: it never changes the policy,
    /// the plan gate or the execution path. `None` (a report built without
    /// a search, such as a display-only caller) claims nothing.
    pub target_indexed: Option<bool>,
    pub outcome: OrganizeOutcome,
    /// The repository policy rule (`.kaseifu.toml`) that decided this file.
    ///
    /// `None` means no policy file applies, or no rule matched, and the
    /// classifier produced `decision`/`policy` as before. `Some` means a rule
    /// hit: Discovery and Jev were not consulted, so `decision` carries no
    /// selection and `policy` is [`DecisionAction::None`].
    pub rule: Option<RuleMatch>,
    /// The `--keywords` input for this run (v112): what the user provided and
    /// what actually reached the Jev request (empty when the flag was not
    /// given, so historical output stays byte-identical).
    pub keywords: KeywordDelivery,
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
    organize_file_with_full(
        file,
        jev,
        history,
        execute,
        thresholds,
        &crate::context::image_metadata::MetadataOptions::default(),
    )
    .await
}

/// [`organize_file_with`] with explicit image toggles (v3.2).
///
/// Honors `[metadata.image]` without re-reading configuration inside the
/// pipeline; callers pass the already-resolved toggles.
pub async fn organize_file_with_full(
    file: &Path,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    thresholds: policy::decision::Thresholds,
    image_options: &crate::context::image_metadata::MetadataOptions,
) -> Result<OrganizeReport> {
    organize_file_with_search(
        file,
        jev,
        history,
        execute,
        thresholds,
        image_options,
        crate::platform::macos::spotlight::search,
    )
    .await
}

/// [`organize_file_with_full`] with an injectable Spotlight search boundary.
///
/// `search` receives the mdfind query and the result limit. Tests use it to
/// run the whole verdict (discovery -> Jev -> policy -> plan -> report)
/// without depending on the machine's index state.
pub async fn organize_file_with_search<F>(
    file: &Path,
    jev: &dyn JevIntegration,
    history: &OperationHistory,
    execute: bool,
    thresholds: policy::decision::Thresholds,
    image_options: &crate::context::image_metadata::MetadataOptions,
    search: F,
) -> Result<OrganizeReport>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    // Repository policy first: an explicit rule is stronger evidence than the
    // classifier, so a hit decides the file without Discovery or a Jev call.
    if let Some(report) = rule_decision(file, execute, history)? {
        return Ok(report);
    }

    // Phase 4: decision (the candidate list is needed to resolve the path).
    let (decision, discovery) =
        pipeline::classify_with_candidates_and_search(file, jev, search, image_options).await?;
    let candidates = discovery.candidates;
    let target_indexed = discovery.target_indexed;

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
        target_indexed,
        outcome,
        rule: None,
        // The CLI attaches `--keywords` after the run; the pipeline itself
        // never sees them (the boundary decorator does).
        keywords: KeywordDelivery::default(),
    })
}

/// The repository policy rule that governs `file`, without touching the
/// journal or the classifier.
///
/// `Ok(None)` means no rule matched; `Err` means the policy exists but is
/// broken (nothing must move). Callers that need the journal only for a move
/// ask this first, so a key-less run that fails later creates no state.
pub fn rule_hit(file: &Path) -> Result<Option<RuleMatch>> {
    repo_policy::evaluate(file, home_directory().as_deref())
}

/// The report of a rule hit: plan the move and perform it only with `execute`.
///
/// The destination is validated by [`plan::build_rule_move_plan`] (workspace
/// boundary, never-a-destination rules, existing directory, no collision), a
/// dry run reports `WouldMove`, `execute` journals the move with the rule
/// label, and a refusal is reported as an unplanable outcome (a collision and
/// a destination outside the workspace render as REVIEW, like every other
/// refusal). A rule hit never falls back to the classifier.
pub fn rule_report(
    file: &Path,
    hit: RuleMatch,
    execute: bool,
    history: &OperationHistory,
) -> Result<OrganizeReport> {
    let planned =
        plan::build_rule_move_plan(file, &hit.destination, &hit.workspace, &hit.journal_id());
    let outcome = match planned {
        Ok(move_plan) => rule_outcome(&move_plan, execute, history),
        Err(error) => OrganizeOutcome::Unplanable(error),
    };
    Ok(OrganizeReport {
        decision: JevDecision::default(),
        policy: DecisionAction::None,
        candidate_path: None,
        // A rule hit consults no index, so nothing is claimed about it.
        target_indexed: None,
        outcome,
        rule: Some(hit),
        keywords: KeywordDelivery::default(),
    })
}

/// [`rule_hit`] plus [`rule_report`], for callers that already hold a journal
/// (the shared entry point used by `organize` and `watch`).
fn rule_decision(
    file: &Path,
    execute: bool,
    history: &OperationHistory,
) -> Result<Option<OrganizeReport>> {
    match rule_hit(file)? {
        Some(hit) => Ok(Some(rule_report(file, hit, execute, history)?)),
        None => Ok(None),
    }
}

/// Plan the move or report it; the filesystem changes only with `execute`.
fn rule_outcome(
    move_plan: &plan::MovePlan,
    execute: bool,
    history: &OperationHistory,
) -> OrganizeOutcome {
    let destination = move_plan.destination_path.clone();
    if !execute {
        return match mover::dry_run(move_plan) {
            Ok(_) => OrganizeOutcome::WouldMove { destination },
            Err(error) => OrganizeOutcome::MoveFailed(format!("{error:?}")),
        };
    }
    match mover::execute_rule_move(move_plan, history) {
        Ok(MoveOutcome::Moved { operation_id }) => OrganizeOutcome::Moved {
            operation_id,
            destination,
        },
        Ok(MoveOutcome::WouldMove) => OrganizeOutcome::WouldMove { destination },
        Err(error) => OrganizeOutcome::MoveFailed(format!("{error:?}")),
    }
}

/// The user's home directory, which bounds the policy lookup.
fn home_directory() -> Option<PathBuf> {
    dirs::home_dir()
}

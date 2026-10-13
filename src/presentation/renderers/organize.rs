//! Organize Rich/plain renderer (display-only).
//!
//! A pure view over [`OrganizeReport`]:
//!
//! - the requested **mode** is stated without claiming that anything moved
//!   (`dry run (no files moved)` / `execute mode`); the result section is
//!   the only place that says what actually happened,
//! - the **Jev selection** (with the discovered directory it resolved to)
//!   is shown separately from the **Rust policy verdict**, and is never
//!   presented as a move permission: even when the policy leaves the file
//!   in place, the destination that was considered stays visible,
//! - `PlanError` variants are rendered as human sentences; the Rust enum
//!   names never reach the user,
//! - `NONE`/no selection shows `no destination` instead of a path,
//! - section order is mode -> decision -> policy -> result, so the mode
//!   line never splits the decision information.

use crate::models::decision::DecisionAction;
use crate::operations::organize::{OrganizeOutcome, OrganizeReport};
use crate::operations::plan::PlanError;
use crate::presentation::components::panel::{self, Section};
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::renderers::generic;
use crate::presentation::theme;
use std::path::Path;

/// Key column width of the decision/result rows (matches classify's
/// `Selected`/`Confidence`/`Margin` alignment).
const KEY_WIDTH: usize = 13;

/// Render the Rich (Cyber Dashboard) panel.
pub fn render_panel(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    color: bool,
) -> String {
    let sections = [
        Section::plain(vec![generic::dry_run_banner(execute_requested, color)]),
        Section::headed("DECISION", decision_lines(report, true, color)),
        Section::headed("POLICY", policy_lines(&report.policy, color)),
        Section::headed(
            result_heading(&report.outcome).0,
            result_lines(report, source, true, color),
        ),
    ];
    panel::frame_with_sections("KASEIFU / ORGANIZE", &sections, color)
}

/// Print the Rich panel.
pub fn print_rich(report: &OrganizeReport, source: &Path, execute_requested: bool, color: bool) {
    print!("{}", render_panel(report, source, execute_requested, color));
}

/// Plain (non-TTY / `--format json`) text: the same sections without ANSI,
/// box chrome, or terminal-width truncation.
pub fn render_text(report: &OrganizeReport, source: &Path, execute_requested: bool) -> String {
    let mut out = String::new();
    push_section(
        &mut out,
        "Mode",
        &[generic::dry_run_banner(execute_requested, false)],
    );
    push_section(&mut out, "Decision", &decision_lines(report, false, false));
    push_section(&mut out, "Policy", &policy_lines(&report.policy, false));
    push_section(
        &mut out,
        result_heading(&report.outcome).1,
        &result_lines(report, source, false, false),
    );
    out
}

fn push_section(out: &mut String, heading: &str, lines: &[String]) {
    out.push_str("== ");
    out.push_str(heading);
    out.push_str(" ==\n");
    for line in lines {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
}

/// `(panel heading, plain heading)` for the outcome.
fn result_heading(outcome: &OrganizeOutcome) -> (&'static str, &'static str) {
    match outcome {
        OrganizeOutcome::Moved { .. } => ("MOVED", "Moved"),
        OrganizeOutcome::WouldMove { .. } => ("WOULD MOVE", "Would move"),
        OrganizeOutcome::MoveFailed(_) => ("MOVE FAILED", "Move failed"),
        // A collision goes to REVIEW (docs/design/05-execute.md).
        OrganizeOutcome::Unplanable(PlanError::Collision(_)) => ("REVIEW", "REVIEW"),
        OrganizeOutcome::Refused | OrganizeOutcome::Unplanable(_) => ("NO MOVE", "No move"),
    }
}

/// DECISION rows: Jev's selection and the discovered directory it resolved
/// to. This is a considered destination, not an execution permission: the
/// policy verdict lives in its own section.
fn decision_lines(report: &OrganizeReport, rich: bool, color: bool) -> Vec<String> {
    let selected = match report.decision.destination_id.as_deref() {
        Some("NONE") => format!(
            "{} - no destination",
            theme::paint("NONE", theme::ANSI_VIOLET, color)
        ),
        None => "no destination".to_string(),
        Some(id) => format!(
            "{} - {}",
            theme::paint(id, theme::ANSI_VIOLET, color),
            report
                .candidate_path
                .as_deref()
                .map(|path| shown_path(path, rich, color))
                .unwrap_or_else(|| "unresolved candidate".to_string())
        ),
    };
    vec![
        row("Selected", &selected),
        row("Confidence", &format!("{:.3}", report.decision.confidence)),
        row("Margin", &format!("{:.3}", report.decision.margin)),
    ]
}

/// POLICY rows: the Rust verdict, with the same wording as `classify`.
fn policy_lines(policy: &DecisionAction, color: bool) -> Vec<String> {
    let (kind, reason) = generic::policy_view(policy);
    vec![generic::decision_line(kind, color), reason]
}

/// Result rows: the only place that says what actually happened.
fn result_lines(report: &OrganizeReport, source: &Path, rich: bool, color: bool) -> Vec<String> {
    match &report.outcome {
        OrganizeOutcome::Moved {
            operation_id,
            destination,
        } => vec![
            row("Destination", &shown_path(destination, rich, color)),
            row("Operation", operation_id),
        ],
        OrganizeOutcome::WouldMove { destination } => vec![
            row("From", &shown_path(source, rich, color)),
            row("Destination", &shown_path(destination, rich, color)),
        ],
        OrganizeOutcome::Refused => vec!["policy refused".to_string()],
        OrganizeOutcome::Unplanable(error) => vec![plan_error_message(error, rich, color)],
        OrganizeOutcome::MoveFailed(error) => vec![error.clone()],
    }
}

/// Human sentence for a failed plan; the Rust enum names never reach the
/// user. `Collision` is rendered under the REVIEW heading (unchanged).
fn plan_error_message(error: &PlanError, rich: bool, color: bool) -> String {
    match error {
        PlanError::NotAutoMove => "policy refused: automatic move not allowed".to_string(),
        PlanError::UnknownCandidate(id) => {
            format!("unknown candidate selected by the decision: {id}")
        }
        PlanError::SourceMissing(path) => {
            format!("source file is missing: {}", shown_path(path, rich, color))
        }
        PlanError::SourceNotAFile(path) => {
            format!(
                "source is not a regular file: {}",
                shown_path(path, rich, color)
            )
        }
        PlanError::DestinationNotADirectory(path) => {
            format!(
                "destination is not a directory: {}",
                shown_path(path, rich, color)
            )
        }
        PlanError::Collision(path) => {
            format!(
                "destination already exists: {}",
                shown_path(path, rich, color)
            )
        }
    }
}

/// Path display shared with `discover`/`classify`: Rich truncates to the
/// terminal budget and emphasizes the last segment ([`paint_path`]), plain
/// keeps the full [`relative_display`] form (stable for pipes and tests).
fn shown_path(path: &Path, rich: bool, color: bool) -> String {
    let text = path.to_string_lossy();
    if rich {
        paint_path(&text, path_budget(), color)
    } else {
        relative_display(&text)
    }
}

/// One `key value` row with the shared key column.
fn row(key: &str, value: &str) -> String {
    format!("{key:<KEY_WIDTH$}{value}")
}

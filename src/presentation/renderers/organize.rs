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
//! - section order is mode -> decision -> policy -> (explanation) -> result,
//!   so the mode line never splits the decision information; the near-miss
//!   line (ZC-5) and the `--explain` section are additions only.

use crate::operations::organize::{OrganizeOutcome, OrganizeReport};
use crate::operations::plan::PlanError;
use crate::policy::decision::Thresholds;
use crate::presentation::components::panel::{self, Section};
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::renderers::explain as explain_view;
use crate::presentation::renderers::generic;
use crate::presentation::theme;
use crate::repo_policy::RuleMatch;
use std::path::Path;

/// Key column width of the decision/result rows (matches classify's
/// `Selected`/`Confidence`/`Margin` alignment).
const KEY_WIDTH: usize = 13;

/// Shown when the index has no entry for the source yet (see
/// [`OrganizeReport::target_indexed`]). One line, no advice: the README's
/// Spotlight section explains waiting for the index or `mdimport`.
const INDEX_LAG_NOTICE: &str = "not indexed yet; candidates may be incomplete";

/// Render the Rich (Cyber Dashboard) panel.
///
/// Existing entry point: default thresholds and no `--explain` section, so
/// callers that only need the historical view keep their exact output.
pub fn render_panel(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    color: bool,
) -> String {
    render_panel_with(
        report,
        source,
        execute_requested,
        color,
        Thresholds::default(),
        false,
    )
}

/// [`render_panel`] with the effective thresholds and the `--explain`
/// section (ZC-5).
pub fn render_panel_with(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    color: bool,
    thresholds: Thresholds,
    explain: bool,
) -> String {
    let sections = sections(
        report,
        source,
        execute_requested,
        true,
        color,
        thresholds,
        explain,
    );
    panel::frame_with_sections("KASEIFU / ORGANIZE", &sections, color)
}

/// Print the Rich panel.
pub fn print_rich(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    color: bool,
    thresholds: Thresholds,
    explain: bool,
) {
    print!(
        "{}",
        render_panel_with(
            report,
            source,
            execute_requested,
            color,
            thresholds,
            explain
        )
    );
}

/// Plain (non-TTY / `--format json`) text: the same sections without ANSI,
/// box chrome, or terminal-width truncation.
///
/// Existing entry point: default thresholds, no `--explain` section.
pub fn render_text(report: &OrganizeReport, source: &Path, execute_requested: bool) -> String {
    render_text_with(
        report,
        source,
        execute_requested,
        Thresholds::default(),
        false,
    )
}

/// [`render_text`] with the effective thresholds and the `--explain`
/// section (ZC-5).
pub fn render_text_with(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    thresholds: Thresholds,
    explain: bool,
) -> String {
    let mut out = String::new();
    push_section(
        &mut out,
        "Mode",
        &[generic::dry_run_banner(execute_requested, false)],
    );
    push_keywords_section(&mut out, report);
    match &report.rule {
        Some(rule) => push_section(&mut out, "Repo policy", &rule_lines(rule, false, false)),
        None => {
            push_section(&mut out, "Decision", &decision_lines(report, false, false));
            push_section(&mut out, "Policy", &policy_lines(report, false, thresholds));
            if explain {
                push_section(
                    &mut out,
                    "Explanation",
                    &explanation_lines(report, false, false, thresholds),
                );
            }
        }
    }
    push_section(
        &mut out,
        result_heading(&report.outcome).1,
        &result_lines(report, source, false, false),
    );
    out
}

/// Shared section list for the Rich panel (mode -> decision -> policy ->
/// explanation -> result); the plain renderer uses the same order.
///
/// A repository rule reports itself in place of the classifier sections: it
/// carries no selection, confidence or margin, so showing empty decision rows
/// would invite reading zeros as evidence. The result section and its order
/// are untouched.
fn sections(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    rich: bool,
    color: bool,
    thresholds: Thresholds,
    explain: bool,
) -> Vec<Section> {
    let mut sections = vec![Section::plain(vec![generic::dry_run_banner(
        execute_requested,
        color,
    )])];
    // `--keywords` (v112): input first, so the reader sees the material
    // before the verdict it produced (or before the rule that replaced it).
    if !report.keywords.is_empty() {
        sections.push(Section::headed("KEYWORDS", report.keywords.rows()));
    }
    match &report.rule {
        Some(rule) => sections.push(Section::headed(
            "REPO POLICY",
            rule_lines(rule, rich, color),
        )),
        None => {
            sections.push(Section::headed(
                "DECISION",
                decision_lines(report, rich, color),
            ));
            sections.push(Section::headed(
                "POLICY",
                policy_lines(report, color, thresholds),
            ));
            if explain {
                sections.push(Section::headed(
                    "EXPLANATION",
                    explanation_lines(report, rich, color, thresholds),
                ));
            }
        }
    }
    sections.push(Section::headed(
        result_heading(&report.outcome).0,
        result_lines(report, source, rich, color),
    ));
    sections
}

/// REPO POLICY rows: which rule matched, its pattern, the destination it
/// resolved to, and the author's reason.
fn rule_lines(rule: &RuleMatch, rich: bool, color: bool) -> Vec<String> {
    let mut lines = vec![
        row("Rule", &rule.rule),
        row("Match", &rule.match_glob),
        row("Destination", &shown_path(&rule.destination, rich, color)),
    ];
    if let Some(reason) = &rule.reason {
        lines.push(row("Reason", reason));
    }
    lines
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

/// The plain-text `Keywords` section, absent when the flag was not given.
fn push_keywords_section(out: &mut String, report: &OrganizeReport) {
    if report.keywords.is_empty() {
        return;
    }
    push_section(out, "Keywords", &report.keywords.rows());
}

/// `(panel heading, plain heading)` for the outcome.
fn result_heading(outcome: &OrganizeOutcome) -> (&'static str, &'static str) {
    match outcome {
        OrganizeOutcome::Moved { .. } => ("MOVED", "Moved"),
        OrganizeOutcome::WouldMove { .. } => ("WOULD MOVE", "Would move"),
        OrganizeOutcome::MoveFailed(_) => ("MOVE FAILED", "Move failed"),
        // A collision goes to REVIEW (docs/design/05-execute.md), and so
        // does a rule destination that leaves its workspace.
        OrganizeOutcome::Unplanable(
            PlanError::Collision(_) | PlanError::DestinationOutsideWorkspace(_),
        ) => ("REVIEW", "REVIEW"),
        OrganizeOutcome::Refused | OrganizeOutcome::Unplanable(_) => ("NO MOVE", "No move"),
    }
}

/// DECISION rows: Jev's selection and the discovered directory it resolved
/// to. This is a considered destination, not an execution permission: the
/// policy verdict lives in its own section.
fn decision_lines(report: &OrganizeReport, rich: bool, color: bool) -> Vec<String> {
    let selected = explain_view::destination_display(
        report.decision.destination_id.as_deref(),
        report.candidate_path.as_deref(),
        rich,
        color,
    );
    let mut lines = vec![
        row("Selected", &selected),
        row("Confidence", &format!("{:.3}", report.decision.confidence)),
        row("Margin", &format!("{:.3}", report.decision.margin)),
    ];
    // The evidence behind the selection may be incomplete (the index has
    // not caught up with the file). Stated next to the selection because
    // that is the claim it qualifies; the policy and result rows below are
    // unaffected.
    if report.target_indexed == Some(false) {
        lines.push(row(
            "Index",
            &theme::paint(INDEX_LAG_NOTICE, theme::ANSI_AMBER, color),
        ));
    }
    lines
}

/// POLICY rows: the Rust verdict, with the same wording as `classify`.
///
/// `thresholds` feeds the ZC-5 near-miss line only; the verdict rows keep
/// their historical (default-threshold) wording.
fn policy_lines(report: &OrganizeReport, color: bool, thresholds: Thresholds) -> Vec<String> {
    let (kind, reason) = generic::policy_view(&report.policy);
    let mut lines = vec![generic::decision_line(kind, color), reason];
    if let Some(near) = explanation_for(report, thresholds).near_line() {
        lines.push(near);
    }
    lines
}

/// EXPLANATION rows (ZC-5): effective thresholds and the resolved
/// destination on one screen.
fn explanation_lines(
    report: &OrganizeReport,
    rich: bool,
    color: bool,
    thresholds: Thresholds,
) -> Vec<String> {
    let explanation = explanation_for(report, thresholds);
    let destination = explain_view::destination_display(
        report.decision.destination_id.as_deref(),
        report.candidate_path.as_deref(),
        rich,
        color,
    );
    explanation.lines(&destination)
}

/// The explanation for one organize report under the effective thresholds.
pub fn explanation_for(
    report: &OrganizeReport,
    thresholds: Thresholds,
) -> explain_view::Explanation {
    explain_view::explain(
        &report.decision,
        &report.policy,
        thresholds,
        report.candidate_path.as_deref(),
    )
}

/// Structured `--format json` report (ZC-7).
///
/// Additive by construction: the plain renderer and every existing JSON key
/// are untouched, and `--explain` only adds the `explanation` object. The
/// outcome carries a machine word (`type`, `kind`) plus the same human
/// sentence the text renderer shows in `reason`, so scripts and people read
/// one source of truth. A repository policy hit adds one `rule` object (name,
/// pattern, destination, reason) and leaves every other key in place; `policy`
/// is then `none` because no classifier verdict exists, and `outcome` stays the
/// authority on what happens to the file. `--keywords` adds one `keywords`
/// object (`provided`, `sent_to_jev`) and nothing else.
pub fn json_report(
    report: &OrganizeReport,
    source: &Path,
    execute_requested: bool,
    explanation: Option<&explain_view::Explanation>,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "source": source.to_string_lossy(),
        "mode": if execute_requested { "execute" } else { "dry_run" },
        "decision": report.decision,
        "policy": report.policy,
        "candidate_path": report
            .candidate_path
            .as_deref()
            .map(|path| path.to_string_lossy().into_owned()),
        "target_indexed": report.target_indexed,
        "outcome": outcome_json(&report.outcome),
    });
    if let Some(rule) = &report.rule {
        value["rule"] = serde_json::json!({
            "name": rule.rule,
            "match_glob": rule.match_glob,
            "destination": rule.destination.to_string_lossy(),
            "reason": rule.reason,
        });
    }
    // Additive (v112): one `keywords` object iff `--keywords` was given;
    // every historical run keeps its exact key set.
    if !report.keywords.is_empty() {
        value["keywords"] = report.keywords.to_json();
    }
    if let Some(explanation) = explanation {
        value["explanation"] = explanation.to_json();
    }
    value
}

/// Machine-shaped outcome object; `type` is always present.
fn outcome_json(outcome: &OrganizeOutcome) -> serde_json::Value {
    match outcome {
        OrganizeOutcome::Moved {
            operation_id,
            destination,
        } => serde_json::json!({
            "type": "moved",
            "destination": destination.to_string_lossy(),
            "operation_id": operation_id,
        }),
        OrganizeOutcome::WouldMove { destination } => serde_json::json!({
            "type": "would_move",
            "destination": destination.to_string_lossy(),
        }),
        OrganizeOutcome::Refused => serde_json::json!({"type": "refused"}),
        OrganizeOutcome::Unplanable(error) => serde_json::json!({
            "type": "unplanable",
            "kind": plan_error_kind(error),
            "reason": plan_error_message_with(error, &raw_path),
        }),
        OrganizeOutcome::MoveFailed(error) => serde_json::json!({
            "type": "move_failed",
            "error": error,
        }),
    }
}

/// Stable machine word for a failed plan (the Rust enum name never leaks).
fn plan_error_kind(error: &PlanError) -> &'static str {
    match error {
        PlanError::NotAutoMove => "not_auto_move",
        PlanError::UnknownCandidate(_) => "unknown_candidate",
        PlanError::SourceMissing(_) => "source_missing",
        PlanError::SourceNotAFile(_) => "source_not_a_file",
        PlanError::DestinationNotADirectory(_) => "destination_not_a_directory",
        PlanError::DestinationExcluded(_) => "destination_excluded",
        PlanError::DestinationOutsideWorkspace(_) => "destination_outside_workspace",
        PlanError::Collision(_) => "collision",
    }
}

/// Raw path rendering for the JSON `reason` (absolute as given).
fn raw_path(path: &Path) -> String {
    path.display().to_string()
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
///
/// Public so the interactive selection (ZC-13) reports a refused destination
/// with exactly the same words as `organize`.
pub fn plan_error_message(error: &PlanError, rich: bool, color: bool) -> String {
    plan_error_message_with(error, &|path| shown_path(path, rich, color))
}

/// [`plan_error_message`] with an explicit path rendering (JSON uses raw
/// paths; the human views use [`shown_path`]).
fn plan_error_message_with(error: &PlanError, shown: &dyn Fn(&Path) -> String) -> String {
    match error {
        PlanError::NotAutoMove => "policy refused: automatic move not allowed".to_string(),
        PlanError::UnknownCandidate(id) => {
            format!("unknown candidate selected by the decision: {id}")
        }
        PlanError::DestinationExcluded(path) => {
            format!("destination is not a usable folder: {}", shown(path))
        }
        PlanError::DestinationOutsideWorkspace(path) => {
            format!(
                "destination is outside the policy workspace: {}",
                shown(path)
            )
        }
        PlanError::SourceMissing(path) => {
            format!("source file is missing: {}", shown(path))
        }
        PlanError::SourceNotAFile(path) => {
            format!("source is not a regular file: {}", shown(path))
        }
        PlanError::DestinationNotADirectory(path) => {
            format!("destination is not a directory: {}", shown(path))
        }
        PlanError::Collision(path) => {
            format!("destination already exists: {}", shown(path))
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

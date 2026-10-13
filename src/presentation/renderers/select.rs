//! Interactive selection renderer (ZC-13, display-only).
//!
//! The menu, the plan and the result are plain ASCII lines; the Rich variant
//! wraps the same lines in the shared panel frame and truncates paths to the
//! terminal budget like the other renderers. Every row is an addition to the
//! existing vocabulary ("dry run (no files moved)" etc.), and the plan is
//! marked `[MANUAL]` so a human decision is never presented as a Jev one.

use crate::models::candidate::CandidateDestination;
use crate::operations::plan::MovePlan;
use crate::presentation::components::panel;
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::renderers::generic;
use std::path::Path;

/// Key column width of the plan/result rows (matches the other views).
const KEY_WIDTH: usize = 13;

/// Render the candidate menu for the interactive selection.
///
/// `target` is the file being organized. Candidates are already ranked by
/// Discovery; the score and the related-file count are the stated basis.
/// Jev is deliberately not consulted, so no suggestion line exists.
pub fn render_menu(
    target: &Path,
    candidates: &[CandidateDestination],
    rich: bool,
    color: bool,
) -> String {
    let mut lines = vec![
        format!("Target: {}", shown_path(target, rich, color)),
        "Candidates are discovered destinations; you choose (Jev is not consulted).".to_string(),
        String::new(),
    ];
    for (index, candidate) in candidates.iter().enumerate() {
        lines.push(menu_row(index + 1, candidate, rich, color));
    }
    lines.push(String::new());
    lines.push("q) cancel (nothing changes)".to_string());
    frame("KASEIFU / SELECT", &lines, rich, color)
}

fn menu_row(number: usize, candidate: &CandidateDestination, rich: bool, color: bool) -> String {
    let noun = if candidate.evidence_count == 1 {
        "related file"
    } else {
        "related files"
    };
    format!(
        "{number:>3}) {}  {}  score {:.2}  ({} {})",
        candidate.id,
        shown_path(&candidate.path, rich, color),
        candidate.score,
        candidate.evidence_count,
        noun
    )
}

/// Render the chosen move plan (dry run or the mode about to run).
pub fn render_plan(
    plan: &MovePlan,
    candidate: &CandidateDestination,
    execute_requested: bool,
    rich: bool,
    color: bool,
) -> String {
    let basis = format!(
        "{} - {} (score {:.2}, {}; you selected it)",
        candidate.id,
        shown_path(&candidate.path, rich, color),
        candidate.score,
        evidence_noun(candidate)
    );
    let lines = vec![
        row("From", &shown_path(&plan.source, rich, color)),
        row(
            "Destination",
            &shown_path(&plan.destination_path, rich, color),
        ),
        row("Basis", &basis),
        row("Mode", &generic::dry_run_banner(execute_requested, color)),
    ];
    frame("KASEIFU / SELECT [MANUAL]", &lines, rich, color)
}

/// Render the completed manual move.
pub fn render_moved(plan: &MovePlan, operation_id: &str, rich: bool, color: bool) -> String {
    let lines = vec![
        row(
            "Destination",
            &shown_path(&plan.destination_path, rich, color),
        ),
        row("Operation", operation_id),
        row("Undo", "kaseifu undo reverses this move"),
    ];
    frame("KASEIFU / SELECT [MANUAL] / MOVED", &lines, rich, color)
}

/// The cancellation line (q, declined confirmation, Ctrl+C equivalent).
pub fn cancelled_line() -> &'static str {
    "cancelled; nothing was changed"
}

/// Rendered when Discovery found nothing to choose from.
pub fn no_candidates_text(rich: bool, color: bool) -> String {
    let lines = vec![
        "No candidate destinations were found for this file.".to_string(),
        "Spotlight may not have indexed similar files yet (about 4 s after a write);".to_string(),
        "run kaseifu doctor if this keeps happening.".to_string(),
    ];
    frame("KASEIFU / SELECT", &lines, rich, color)
}

fn evidence_noun(candidate: &CandidateDestination) -> String {
    if candidate.evidence_count == 1 {
        "1 related file".to_string()
    } else {
        format!("{} related files", candidate.evidence_count)
    }
}

/// Rich wraps the lines in the shared frame; plain prints them as they are
/// (stable for pipes and tests).
fn frame(title: &str, lines: &[String], rich: bool, color: bool) -> String {
    if rich {
        panel::frame(title, lines, color)
    } else {
        let mut out = format!("{title}\n");
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

/// Path display shared with the other views: Rich truncates to the terminal
/// budget, plain keeps the full [`relative_display`] form.
fn shown_path(path: &Path, rich: bool, color: bool) -> String {
    let text = path.to_string_lossy();
    if rich {
        paint_path(&text, path_budget(), color)
    } else {
        relative_display(&text)
    }
}

fn row(key: &str, value: &str) -> String {
    format!("{key:<KEY_WIDTH$}{value}")
}

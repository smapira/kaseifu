//! v3.1 Classify Rich renderer.
//!
//! Read-only view over [`ClassifyReport`]: Jev probabilities are sorted
//! descending for display only (never rescored, never renormalized),
//! candidate IDs are resolved to directories, and the Rust Policy verdict
//! (from `policy::decision::evaluate`) is shown separately from Jev's
//! selection. `MoveTo` is Jev's pick, not a move permission, and is never
//! rendered as one.

use crate::classifier::pipeline::ClassifyReport;
use crate::models::decision::DecisionAction;
use crate::policy::decision::{AUTO_MOVE_CONFIDENCE, AUTO_MOVE_MARGIN, REVIEW_CONFIDENCE};
use crate::presentation::components::display::{MISSING, format_bytes};
use crate::presentation::components::panel::{self, Section};
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::components::score_bar::{percent_label, render_probability_bar_auto};
use crate::presentation::renderers::generic;
use crate::presentation::theme;
use std::collections::HashMap;

/// Default ranking size; `--all` lifts the limit.
pub const RANKING_LIMIT: usize = 5;

/// Sort probabilities descending (display order only; values untouched).
/// Sort is stable, so tied inputs keep Jev's relative order.
pub fn rank_probabilities(probabilities: &[(String, f64)]) -> Vec<(String, f64)> {
    let mut ranked = probabilities.to_vec();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

/// Resolve a candidate ID to its discovered directory path.
pub fn resolve_directory<'a>(id: &str, by_id: &'a HashMap<&str, String>) -> Option<&'a String> {
    by_id.get(id)
}

fn candidate_paths(report: &ClassifyReport) -> HashMap<&str, String> {
    report
        .candidates
        .iter()
        .map(|c| (c.id.as_str(), c.path.to_string_lossy().to_string()))
        .collect()
}

fn target_summary(report: &ClassifyReport) -> String {
    let kind = report
        .target_extension
        .as_deref()
        .map(|ext| ext.to_uppercase())
        .or_else(|| report.target_file_type.clone())
        .unwrap_or_else(|| "unknown type".to_string());
    format!(
        "{} {} {}",
        kind,
        "\u{00b7}",
        format_bytes(report.target_size_bytes)
    )
}

fn policy_view(policy: &DecisionAction) -> (&'static str, String) {
    match policy {
        DecisionAction::AutoMove(_) => (
            "AUTO MOVE",
            format!(
                "meets auto-move thresholds (confidence >= {:.2}, margin >= {:.2})",
                AUTO_MOVE_CONFIDENCE, AUTO_MOVE_MARGIN
            ),
        ),
        DecisionAction::Review(_) => (
            "REVIEW",
            format!(
                "below auto-move thresholds; human review required (review confidence >= {:.2})",
                REVIEW_CONFIDENCE
            ),
        ),
        DecisionAction::Leave { what } => ("LEAVE", what.clone()),
        // `evaluate` only yields the three variants above; anything else is
        // not an execution verdict, so it must read as LEAVE, never as a move.
        _ => ("LEAVE", "policy did not authorize execution".to_string()),
    }
}

fn policy_destination(policy: &DecisionAction) -> Option<&str> {
    match policy {
        DecisionAction::AutoMove(id) | DecisionAction::Review(id) => Some(id.as_str()),
        _ => None,
    }
}

fn body_sections(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> Vec<Section> {
    let by_id = candidate_paths(report);
    let ranked = rank_probabilities(&report.probabilities);
    let selected = report.decision.destination_id.as_deref().unwrap_or(MISSING);

    // TARGET + DISCOVERY (leading block, no divider).
    let found = report.candidates.len();
    let head = vec![
        theme::paint("TARGET", theme::ANSI_TEAL, color),
        report.target_filename.clone(),
        theme::paint(&target_summary(report), theme::ANSI_DIM, color),
        String::new(),
        theme::paint("DISCOVERY", theme::ANSI_TEAL, color),
        if found == 1 {
            "1 destination found".to_string()
        } else {
            format!("{} destinations found", found)
        },
    ];

    // JEV RANKING.
    let mut ranking = Vec::new();
    if ranked.is_empty() {
        ranking.push("No probability distribution (no candidates).".to_string());
    } else {
        let limit = if show_all {
            ranked.len()
        } else {
            ranked.len().min(RANKING_LIMIT)
        };
        let mut shown_ids = std::collections::HashSet::new();
        for (index, (id, prob)) in ranked.iter().take(limit).enumerate() {
            shown_ids.insert(id.as_str());
            ranking.extend(ranking_row(
                index + 1,
                id,
                *prob,
                selected == *id,
                &by_id,
                report,
                verbose,
                color,
            ));
            ranking.push(String::new());
        }
        // The selection is always shown, even outside the top 5.
        if !shown_ids.contains(selected)
            && let Some((pos, (id, prob))) = ranked
                .iter()
                .enumerate()
                .find(|(_, (id, _))| id == selected)
        {
            ranking.push(theme::paint(
                &format!("Selected candidate is ranked #{}:", pos + 1),
                theme::ANSI_AMBER,
                color,
            ));
            ranking.extend(ranking_row(
                pos + 1,
                id,
                *prob,
                true,
                &by_id,
                report,
                verbose,
                color,
            ));
            ranking.push(String::new());
        }
        if !show_all && ranked.len() > limit {
            ranking.push(format!(
                "Showing {} of {} destinations (use --all)",
                limit,
                ranked.len()
            ));
        }
    }

    // JEV DECISION.
    let mut decision = Vec::new();
    let selected_dir = if selected == "NONE" {
        "no destination".to_string()
    } else {
        resolve_directory(selected, &by_id)
            .map(|s| paint_path(s, path_budget(), color))
            .unwrap_or_else(|| "unresolved candidate".to_string())
    };
    decision.push(format!(
        "Selected     {} {} {}",
        theme::paint(selected, theme::ANSI_VIOLET, color),
        theme::paint("\u{00b7}", theme::ANSI_DIM, color),
        selected_dir
    ));
    decision.push(format!("Confidence   {:.2}", report.decision.confidence));
    decision.push(format!("Margin       {:.2}", report.decision.margin));

    // POLICY (Rust verdict; never Jev's `move_to`).
    let mut policy_lines = Vec::new();
    let (kind, reason) = policy_view(policy);
    policy_lines.push(generic::decision_line(kind, color));
    policy_lines.push(reason);
    if let Some(id) = policy_destination(policy) {
        let dir = resolve_directory(id, &by_id)
            .map(|s| paint_path(s, path_budget(), color))
            .unwrap_or_else(|| "unresolved candidate".to_string());
        policy_lines.push(format!(
            "{} {} {}",
            theme::paint(id, theme::ANSI_VIOLET, color),
            theme::paint("\u{00b7}", theme::ANSI_DIM, color),
            dir
        ));
    }

    vec![
        Section::plain(head),
        Section::headed("JEV RANKING", ranking),
        Section::headed("JEV DECISION", decision),
        Section::headed("POLICY", policy_lines),
    ]
}

#[allow(clippy::too_many_arguments)]
fn ranking_row(
    rank: usize,
    id: &str,
    probability: f64,
    is_selected: bool,
    by_id: &HashMap<&str, String>,
    report: &ClassifyReport,
    verbose: bool,
    color: bool,
) -> Vec<String> {
    let dir = if id == "NONE" {
        MISSING.to_string()
    } else {
        match resolve_directory(id, by_id) {
            Some(path) if verbose => relative_display(path),
            Some(path) => paint_path(path, path_budget(), color),
            None => "unresolved candidate".to_string(),
        }
    };
    let marker = if is_selected { "*" } else { " " };
    let id_painted = theme::paint(id, theme::ANSI_VIOLET, color);
    let mut rows = vec![format!(
        "{}{:<2} {}  {}  {}",
        marker,
        rank,
        id_painted,
        dir,
        percent_label(probability)
    )];
    rows.push(format!(
        "     {}",
        render_probability_bar_auto(probability, color)
    ));
    if verbose && let Some(candidate) = report.candidates.iter().find(|c| c.id == id) {
        rows.push(format!(
            "     {}",
            theme::paint(
                &format!(
                    "evidence={} reasons=[{}]",
                    candidate.evidence_count,
                    candidate.reasons.join("; ")
                ),
                theme::ANSI_DIM,
                color
            )
        ));
    }
    rows
}

/// Render the classify panel (Rich output).
pub fn render_panel(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> String {
    panel::frame_with_sections(
        "KASEIFU / CLASSIFY",
        &body_sections(report, policy, show_all, verbose, color),
        color,
    )
}

pub fn print_rich(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
) {
    print!("{}", render_panel(report, policy, show_all, verbose, color));
}

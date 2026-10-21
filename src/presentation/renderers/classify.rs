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
use crate::policy::decision::Thresholds;
use crate::presentation::components::display::{MISSING, format_bytes};
use crate::presentation::components::panel::{self, Section};
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::components::score_bar::{percent_label, render_probability_bar_auto};
use crate::presentation::renderers::explain::{self, Explanation};
use crate::presentation::renderers::generic;
use crate::presentation::theme;
use std::collections::HashMap;
use std::path::PathBuf;

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
    format!("{} - {}", kind, format_bytes(report.target_size_bytes))
}

fn policy_destination(policy: &DecisionAction) -> Option<&str> {
    match policy {
        DecisionAction::AutoMove(id) | DecisionAction::Review(id) => Some(id.as_str()),
        _ => None,
    }
}

fn target_head(report: &ClassifyReport, color: bool) -> Vec<String> {
    let found = report.candidates.len();
    vec![
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
    ]
}

fn ranking_limit(ranked_len: usize, show_all: bool) -> usize {
    if show_all {
        ranked_len
    } else {
        ranked_len.min(RANKING_LIMIT)
    }
}

/// Shared rendering context for the JEV ranking block.
struct RankingContext<'a, 'b> {
    by_id: &'a HashMap<&'b str, String>,
    report: &'a ClassifyReport,
    selected: &'a str,
    verbose: bool,
    color: bool,
}

fn push_visible_ranking_rows(
    ranking: &mut Vec<String>,
    ranked: &[(String, f64)],
    limit: usize,
    ctx: &RankingContext<'_, '_>,
    shown_ids: &mut std::collections::HashSet<String>,
) {
    for (index, (id, prob)) in ranked.iter().take(limit).enumerate() {
        shown_ids.insert(id.clone());
        ranking.extend(ranking_row(
            index + 1,
            id,
            *prob,
            ctx.selected == *id,
            ctx.by_id,
            ctx.report,
            ctx.verbose,
            ctx.color,
        ));
        ranking.push(String::new());
    }
}

/// The selection is always shown, even outside the top 5.
fn push_selected_outside_top(
    ranking: &mut Vec<String>,
    ranked: &[(String, f64)],
    shown_ids: &std::collections::HashSet<String>,
    ctx: &RankingContext<'_, '_>,
) {
    if shown_ids.contains(ctx.selected) {
        return;
    }
    let Some((pos, (id, prob))) = ranked
        .iter()
        .enumerate()
        .find(|(_, (id, _))| id == ctx.selected)
    else {
        return;
    };
    ranking.push(theme::paint(
        &format!("Selected candidate is ranked #{}:", pos + 1),
        theme::ANSI_AMBER,
        ctx.color,
    ));
    ranking.extend(ranking_row(
        pos + 1,
        id,
        *prob,
        true,
        ctx.by_id,
        ctx.report,
        ctx.verbose,
        ctx.color,
    ));
    ranking.push(String::new());
}

fn push_ranking_footer(
    ranking: &mut Vec<String>,
    ranked: &[(String, f64)],
    limit: usize,
    show_all: bool,
) {
    if show_all || ranked.len() <= limit {
        return;
    }
    // The ranked distribution includes NONE, which is not a
    // destination: say so to avoid confusion with the DISCOVERY
    // count above.
    let scope = if ranked.iter().any(|(id, _)| id == "NONE") {
        format!("{} (incl. NONE)", ranked.len())
    } else {
        ranked.len().to_string()
    };
    ranking.push(format!(
        "Showing {} of {} destinations (use --all)",
        limit, scope
    ));
}

fn ranking_lines(
    report: &ClassifyReport,
    ranked: &[(String, f64)],
    by_id: &HashMap<&str, String>,
    selected: &str,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> Vec<String> {
    let mut ranking = Vec::new();
    if ranked.is_empty() {
        ranking.push("No probability distribution (no candidates).".to_string());
        return ranking;
    }
    let limit = ranking_limit(ranked.len(), show_all);
    let mut shown_ids = std::collections::HashSet::new();
    let ctx = RankingContext {
        by_id,
        report,
        selected,
        verbose,
        color,
    };
    push_visible_ranking_rows(&mut ranking, ranked, limit, &ctx, &mut shown_ids);
    push_selected_outside_top(&mut ranking, ranked, &shown_ids, &ctx);
    push_ranking_footer(&mut ranking, ranked, limit, show_all);
    ranking
}

fn decision_lines(
    report: &ClassifyReport,
    by_id: &HashMap<&str, String>,
    selected: &str,
    color: bool,
) -> Vec<String> {
    let mut decision = Vec::new();
    decision.push(format!(
        "Selected     {} - {}",
        theme::paint(selected, theme::ANSI_VIOLET, color),
        selected_directory_label(selected, by_id, color)
    ));
    decision.push(format!("Confidence   {:.2}", report.decision.confidence));
    decision.push(format!("Margin       {:.2}", report.decision.margin));
    decision
}

fn selected_directory_label(selected: &str, by_id: &HashMap<&str, String>, color: bool) -> String {
    if selected == "NONE" {
        "no destination".to_string()
    } else {
        resolve_directory(selected, by_id)
            .map(|s| paint_path(s, path_budget(), color))
            .unwrap_or_else(|| "unresolved candidate".to_string())
    }
}

/// POLICY (Rust verdict; never Jev's `move_to`).
///
/// `near` (ZC-5) is the near-miss line that applies below the auto-move
/// thresholds; the existing rows keep their wording and order.
fn policy_lines(
    policy: &DecisionAction,
    by_id: &HashMap<&str, String>,
    color: bool,
    near: Option<String>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let (kind, reason) = generic::policy_view(policy);
    lines.push(generic::decision_line(kind, color));
    lines.push(reason);
    if let Some(near) = near {
        lines.push(near);
    }
    if let Some(id) = policy_destination(policy) {
        let dir = resolve_directory(id, by_id)
            .map(|s| paint_path(s, path_budget(), color))
            .unwrap_or_else(|| "unresolved candidate".to_string());
        lines.push(format!(
            "{} - {}",
            theme::paint(id, theme::ANSI_VIOLET, color),
            dir
        ));
    }
    lines
}

/// The explanation for one classification, using the effective thresholds.
pub fn decision_explanation(
    report: &ClassifyReport,
    policy: &DecisionAction,
    thresholds: Thresholds,
) -> Explanation {
    let path: Option<PathBuf> = report
        .candidates
        .iter()
        .find(|candidate| Some(&candidate.id) == report.decision.destination_id.as_ref())
        .map(|candidate| candidate.path.clone());
    explain::explain(&report.decision, policy, thresholds, path.as_deref())
}

fn body_sections(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
    thresholds: Thresholds,
    explain: bool,
) -> Vec<Section> {
    let by_id = candidate_paths(report);
    let ranked = rank_probabilities(&report.probabilities);
    let selected = report.decision.destination_id.as_deref().unwrap_or(MISSING);
    let explanation = decision_explanation(report, policy, thresholds);
    let near = explanation.near_line();

    let mut sections = vec![Section::plain(target_head(report, color))];
    // `--keywords` (v112): shown before the ranking, next to the target they
    // qualify; absent when the flag was not given.
    if !report.keywords.is_empty() {
        sections.push(Section::headed("KEYWORDS", report.keywords.rows()));
    }
    sections.push(Section::headed(
        "JEV RANKING",
        ranking_lines(report, &ranked, &by_id, selected, show_all, verbose, color),
    ));
    sections.push(Section::headed(
        "JEV DECISION",
        decision_lines(report, &by_id, selected, color),
    ));
    sections.push(Section::headed(
        "POLICY",
        policy_lines(policy, &by_id, color, near),
    ));
    if explain {
        let destination = explain::destination_display(
            report.decision.destination_id.as_deref(),
            explanation
                .destination_path
                .as_deref()
                .map(std::path::Path::new),
            true,
            color,
        );
        sections.push(Section::headed(
            "EXPLANATION",
            explanation.lines(&destination),
        ));
    }
    sections
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
///
/// Existing entry point: default thresholds and no `--explain` section, so
/// callers that only need the historical view keep their exact output.
pub fn render_panel(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> String {
    render_panel_with(
        report,
        policy,
        show_all,
        verbose,
        color,
        Thresholds::default(),
        false,
    )
}

/// [`render_panel`] with the effective thresholds and the `--explain`
/// section (ZC-5). The near-miss line is part of the POLICY section; the
/// EXPLANATION section is appended only when requested.
pub fn render_panel_with(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
    thresholds: Thresholds,
    explain: bool,
) -> String {
    panel::frame_with_sections(
        "KASEIFU / CLASSIFY",
        &body_sections(
            report, policy, show_all, verbose, color, thresholds, explain,
        ),
        color,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn print_rich(
    report: &ClassifyReport,
    policy: &DecisionAction,
    show_all: bool,
    verbose: bool,
    color: bool,
    thresholds: Thresholds,
    explain: bool,
) {
    print!(
        "{}",
        render_panel_with(
            report, policy, show_all, verbose, color, thresholds, explain
        )
    );
}

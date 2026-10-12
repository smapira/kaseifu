use crate::models::candidate::CandidateDestination;
use crate::presentation::components::panel;
use crate::presentation::components::path::paint_path;
use crate::presentation::components::score_bar::render_bar_auto;
use crate::presentation::theme;

/// Path display budget: wide enough that the differing middle of long
/// Spotlight paths survives; overflow is shortened head-first so the
/// last (most identifying) segment is never cut in the middle.
pub const PATH_WIDTH: usize = 64;

pub struct DiscoverRow {
    pub id: String,
    pub short: String,
    pub score: f64,
    pub bar: String,
    pub reason: String,
    pub breakdown: Option<String>,
}

pub fn build_rows(
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
    color: bool,
) -> (Vec<DiscoverRow>, f64) {
    let max = candidates
        .iter()
        .map(|c| c.score)
        .filter(|s| s.is_finite() && *s >= 0.0)
        .fold(0.0, f64::max);
    let limit = if show_all {
        candidates.len()
    } else {
        candidates.len().min(5)
    };
    let mut rows = Vec::new();
    for c in candidates.iter().take(limit) {
        let reason = c
            .reasons
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(" \u{00b7} ");
        let breakdown = if verbose {
            Some(format!(
                "score={:.3} evidence={} reasons=[{}]",
                c.score,
                c.evidence_count,
                c.reasons.join("; ")
            ))
        } else {
            None
        };
        rows.push(DiscoverRow {
            id: c.id.clone(),
            short: paint_path(&c.path.to_string_lossy(), PATH_WIDTH, color),
            score: c.score,
            bar: render_bar_auto(c.score, max, color),
            reason,
            breakdown,
        });
    }
    (rows, max)
}

fn body_lines(
    target_name: &str,
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
    color: bool,
) -> Vec<String> {
    let (rows, _) = build_rows(candidates, show_all, verbose, color);
    let mut lines = Vec::new();
    lines.push(format!(
        "Target  {}",
        theme::paint(target_name, theme::ANSI_DIM, color)
    ));
    if rows.is_empty() {
        lines.push("No candidates found.".to_string());
        return lines;
    }
    lines.push(String::new());
    for r in &rows {
        let id = theme::paint(&r.id, theme::ANSI_VIOLET, color);
        lines.push(format!("{}  {}  {:.2}", id, r.short, r.score));
        lines.push(format!("    {}", r.bar));
        if !r.reason.is_empty() {
            lines.push(format!(
                "    {}",
                theme::paint(&r.reason, theme::ANSI_TEAL, color)
            ));
        }
        if let Some(b) = &r.breakdown {
            lines.push(format!("    {}", theme::paint(b, theme::ANSI_DIM, color)));
        }
    }
    if !show_all && candidates.len() > rows.len() {
        lines.push(format!(
            "Showing {} of {} (use --all)",
            rows.len(),
            candidates.len()
        ));
    }
    lines.push("Bars show relative score within these candidates (not confidence).".to_string());
    lines
}

pub fn render_text(
    target_name: &str,
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
    color: bool,
) -> String {
    let title = theme::paint("KASEIFU / DISCOVER", theme::ANSI_CYAN, color);
    let mut out = format!("{} \u{2500} {}\n", title, target_name);
    for line in body_lines(target_name, candidates, show_all, verbose, color)
        .iter()
        .skip(1)
    {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Rich dashboard: the same rows inside a self-drawn box.
///
/// The box is drawn with ANSI-aware widths, so borders stay aligned
/// whether colors are on or off.
pub fn render_panel(
    target_name: &str,
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
    color: bool,
) -> String {
    panel::frame(
        "KASEIFU / DISCOVER",
        &body_lines(target_name, candidates, show_all, verbose, color),
        color,
    )
}

pub fn print_rich(
    target_name: &str,
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
    color: bool,
) {
    print!(
        "{}",
        render_panel(target_name, candidates, show_all, verbose, color)
    );
}

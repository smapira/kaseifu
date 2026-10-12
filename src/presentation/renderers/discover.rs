use crate::models::candidate::CandidateDestination;
use crate::presentation::components::path::short_path;
use crate::presentation::components::score_bar::render_bar_auto;
use crate::presentation::theme;

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
            short: short_path(&c.path.to_string_lossy(), 42),
            score: c.score,
            bar: render_bar_auto(c.score, max, color),
            reason,
            breakdown,
        });
    }
    (rows, max)
}

pub fn render_text(
    target_name: &str,
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
    color: bool,
) -> String {
    let (rows, _) = build_rows(candidates, show_all, verbose, color);
    let mut out = String::new();
    let title = theme::paint("KASEIFU / DISCOVER", theme::ANSI_CYAN, color);
    out.push_str(&format!("{} \u{2500} {}\n", title, target_name));
    if rows.is_empty() {
        out.push_str("No candidates found.\n");
        return out;
    }
    for r in &rows {
        let id = theme::paint(&r.id, theme::ANSI_VIOLET, color);
        out.push_str(&format!("{}  {}  {:.2}\n", id, r.short, r.score));
        out.push_str(&format!("    {}\n", r.bar));
        if !r.reason.is_empty() {
            out.push_str(&format!(
                "    {}\n",
                theme::paint(&r.reason, theme::ANSI_TEAL, color)
            ));
        }
        if let Some(b) = &r.breakdown {
            out.push_str(&format!(
                "    {}\n",
                theme::paint(b, theme::ANSI_DIM, color)
            ));
        }
    }
    if !show_all && candidates.len() > rows.len() {
        out.push_str(&format!(
            "Showing {} of {} (use --all)\n",
            rows.len(),
            candidates.len()
        ));
    }
    out.push_str("Bars show relative score within these candidates (not confidence).\n");
    out
}

pub fn print_rich(
    target_name: &str,
    candidates: &[CandidateDestination],
    show_all: bool,
    verbose: bool,
) {
    use rich_rs::{Console, Panel, Table, Text};
    let plain = render_text(target_name, candidates, show_all, verbose, true);
    let mut console = Console::new();
    let text = Text::plain(&plain);
    let panel = Panel::new(Box::new(text)).with_title("KASEIFU / DISCOVER");
    let _ = console.print(&panel, None, None, None, false, "\n");
    if verbose {
        let mut table = Table::new();
        table.add_column_str("ID");
        table.add_column_str("score");
        table.add_column_str("evidence");
        for c in candidates
            .iter()
            .take(if show_all { candidates.len() } else { 5 })
        {
            table.add_row_strs(&[
                &c.id,
                &format!("{:.3}", c.score),
                &c.evidence_count.to_string(),
            ]);
        }
        let _ = console.print(&table, None, None, None, false, "\n");
    }
}

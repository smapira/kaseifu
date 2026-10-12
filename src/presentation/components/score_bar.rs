use crate::presentation::theme;

pub const DEFAULT_WIDTH: usize = 30;

pub fn term_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(80)
}

pub fn bar_width() -> usize {
    let w = term_width();
    if w < 50 {
        10
    } else if w < 80 {
        20
    } else {
        DEFAULT_WIDTH
    }
}

pub fn relative_percent(score: f64, max_score: f64) -> Option<f64> {
    if !score.is_finite() || !max_score.is_finite() {
        return None;
    }
    if score < 0.0 || max_score <= 0.0 {
        return None;
    }
    Some((score / max_score * 100.0).clamp(0.0, 100.0))
}

pub fn render_bar(score: f64, max_score: f64, width: usize, color_enabled: bool) -> String {
    let Some(pct) = relative_percent(score, max_score) else {
        return "N/A".to_string();
    };
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    let body = format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty));
    let label = format!("{:>5.1}%", pct);
    if color_enabled {
        format!(
            "{}{}{} {}",
            theme::ANSI_MINT,
            body,
            theme::ANSI_RESET,
            label
        )
    } else {
        format!("{} {}", body, label)
    }
}

pub fn render_bar_auto(score: f64, max_score: f64, color_enabled: bool) -> String {
    render_bar(score, max_score, bar_width(), color_enabled)
}

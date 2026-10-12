use crate::presentation::theme;

pub const DEFAULT_WIDTH: usize = 30;

/// Terminal width in columns: `$COLUMNS` first, then ioctl on stdout
/// (shells rarely export `COLUMNS` to children), defaulting to 80.
pub fn term_width() -> usize {
    if let Ok(v) = std::env::var("COLUMNS")
        && let Ok(n) = v.trim().parse::<usize>()
        && n > 0
    {
        return n;
    }
    #[cfg(unix)]
    if let Some(n) = ioctl_width() {
        return n;
    }
    80
}

#[cfg(unix)]
fn ioctl_width() -> Option<usize> {
    // SAFETY: ioctl with TIOCGWINSZ only writes the winsize struct.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return Some(ws.ws_col as usize);
        }
    }
    None
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

use crate::presentation::components::path::visible_width;
use crate::presentation::theme;

/// Draw a box around pre-rendered content lines.
///
/// Widths are measured with [`visible_width`], so embedded ANSI colors
/// never misalign the borders (unlike measuring raw string length).
/// Every emitted line has the same visible width.
pub fn frame(title: &str, lines: &[String], color: bool) -> String {
    let title_text = format!("\u{25c8} {}", title);
    let title_w = visible_width(&title_text);
    let body_max = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0);
    let inner = body_max.max(title_w + 4);

    let paint_border = |s: &str| theme::paint(s, theme::ANSI_DIM, color);
    let title_painted = if color {
        format!(
            "{}{}{}{}",
            theme::ANSI_BOLD,
            theme::ANSI_CYAN,
            title_text,
            theme::ANSI_RESET,
        )
    } else {
        title_text.clone()
    };

    let mut out = String::new();
    // Top: ╭─ ◈ TITLE ───╮ (visible width inner + 4, like body rows)
    let dashes = inner.saturating_sub(title_w + 1);
    out.push_str(&paint_border("\u{256d}\u{2500} "));
    out.push_str(&title_painted);
    out.push(' ');
    out.push_str(&paint_border(&"\u{2500}".repeat(dashes)));
    out.push_str(&paint_border("\u{256e}"));
    out.push('\n');
    // Body.
    for line in lines {
        let pad = inner.saturating_sub(visible_width(line));
        out.push_str(&paint_border("\u{2502} "));
        out.push_str(line);
        out.push_str(&" ".repeat(pad));
        out.push_str(&paint_border(" \u{2502}"));
        out.push('\n');
    }
    // Bottom: ╰─────╯
    out.push_str(&paint_border("\u{2570}"));
    out.push_str(&paint_border(&"\u{2500}".repeat(inner + 2)));
    out.push_str(&paint_border("\u{256f}"));
    out.push('\n');
    out
}

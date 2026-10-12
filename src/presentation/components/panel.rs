use crate::presentation::components::path::visible_width;
use crate::presentation::theme;

/// Draw a box around pre-rendered content lines.
///
/// Widths are measured with [`visible_width`], so embedded ANSI colors
/// never misalign the borders (unlike measuring raw string length).
/// Every emitted line has the same visible width.
///
/// Chrome is pure ASCII (`+`, `-`, `|`): box-drawing and block glyphs
/// render double-width on CJK-font terminals, which scatters the right
/// border. Colors still carry the Cyber Dashboard theme.
pub fn frame(title: &str, lines: &[String], color: bool) -> String {
    let title_text = format!("* {}", title);
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
    // Top: +-- * TITLE ---+ (visible width inner + 4, like body rows)
    let dashes = inner.saturating_sub(title_w + 2);
    out.push_str(&paint_border("+-- "));
    out.push_str(&title_painted);
    out.push(' ');
    out.push_str(&paint_border(&"-".repeat(dashes)));
    out.push_str(&paint_border("+"));
    out.push('\n');
    // Body.
    for line in lines {
        let pad = inner.saturating_sub(visible_width(line));
        out.push_str(&paint_border("| "));
        out.push_str(line);
        out.push_str(&" ".repeat(pad));
        out.push_str(&paint_border(" |"));
        out.push('\n');
    }
    // Bottom: +-------+
    out.push_str(&paint_border("+"));
    out.push_str(&paint_border(&"-".repeat(inner + 2)));
    out.push_str(&paint_border("+"));
    out.push('\n');
    out
}

/// One headed block inside a [`frame_with_sections`] panel.
///
/// `heading` is `None` for the leading block (no divider above it).
/// A `Some` heading renders an `|-- SECTION --|` divider row above
/// `lines`, spanning the same visible width as every other row.
#[derive(Debug, Clone, Default)]
pub struct Section {
    pub heading: Option<String>,
    pub lines: Vec<String>,
}

impl Section {
    pub fn plain(lines: Vec<String>) -> Self {
        Self {
            heading: None,
            lines,
        }
    }

    pub fn headed(heading: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            heading: Some(heading.into()),
            lines,
        }
    }
}

/// Draw a box around headed content sections.
///
/// Same geometry and ANSI-aware measuring as [`frame`]: every emitted
/// row (top, body, dividers, bottom) shares one visible width, so borders
/// stay aligned with colors on or off, including narrow (80-column)
/// terminals. [`frame`] is the single-section special case.
pub fn frame_with_sections(title: &str, sections: &[Section], color: bool) -> String {
    let title_text = format!("* {}", title);
    let title_w = visible_width(&title_text);
    let body_max = sections
        .iter()
        .flat_map(|s| s.lines.iter().chain(s.heading.iter()))
        .map(|line| visible_width(line))
        .max()
        .unwrap_or(0);
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
    // Top: +-- * TITLE ---+ (visible width inner + 4, like body rows)
    let dashes = inner.saturating_sub(title_w + 2);
    out.push_str(&paint_border("+-- "));
    out.push_str(&title_painted);
    out.push(' ');
    out.push_str(&paint_border(&"-".repeat(dashes)));
    out.push_str(&paint_border("+"));
    out.push('\n');
    // Sections.
    for section in sections {
        if let Some(heading) = &section.heading {
            out.push_str(&paint_border(&divider_row(heading, inner)));
            out.push('\n');
        }
        for line in &section.lines {
            let pad = inner.saturating_sub(visible_width(line));
            out.push_str(&paint_border("| "));
            out.push_str(line);
            out.push_str(&" ".repeat(pad));
            out.push_str(&paint_border(" |"));
            out.push('\n');
        }
    }
    // Bottom: +-------+
    out.push_str(&paint_border("+"));
    out.push_str(&paint_border(&"-".repeat(inner + 2)));
    out.push_str(&paint_border("+"));
    out.push('\n');
    out
}

/// An `|-- HEADING --|` row exactly `inner + 4` columns wide
/// (visible width; the heading itself is never padded, only the dashes).
/// Over-long headings are shortened head-first so the frame never breaks.
fn divider_row(heading: &str, inner: usize) -> String {
    let total = inner + 4;
    // `|-- ` + heading + ` ` + dashes + `|`: fixed chrome is 6 columns.
    let max_heading = total.saturating_sub(6).max(1);
    let shown = if visible_width(heading) > max_heading {
        let keep = max_heading.saturating_sub(3).max(1);
        let tail: String = heading
            .chars()
            .rev()
            .take(keep)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("...{}", tail)
    } else {
        heading.to_string()
    };
    let dashes = total.saturating_sub(6 + visible_width(&shown));
    format!("|-- {} {}|", shown, "-".repeat(dashes))
}

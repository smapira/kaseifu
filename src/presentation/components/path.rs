pub fn short_path(path: &str, max_len: usize) -> String {
    paint_path(path, max_len, false)
}

/// Strip ANSI CSI escape sequences (e.g. `\x1b[1m`, `\x1b[38;2;..m`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Visible terminal width: ANSI escapes count as 0.
pub fn visible_width(s: &str) -> usize {
    display_width(&strip_ansi(s))
}

/// Display budget for paths: fill the terminal width minus the row
/// chrome (`D16  `, `  0.00`, panel borders), floored so narrow
/// terminals still show a useful tail. Over-long paths are shortened
/// head-first (see [`paint_path`]).
pub fn path_budget() -> usize {
    super::score_bar::term_width().saturating_sub(16).max(48)
}

/// Display a path relative to the current directory when possible,
/// otherwise relative to `$HOME` (`~`), otherwise as-is.
pub fn relative_display(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let src = if trimmed.is_empty() { path } else { trimmed };
    if let Ok(cwd) = std::env::current_dir() {
        let cwd = cwd.to_string_lossy();
        if src == cwd {
            return ".".to_string();
        }
        if let Some(rest) = src.strip_prefix(cwd.as_ref())
            && rest.starts_with('/')
        {
            return format!(".{}", rest);
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        if src == home {
            return "~".to_string();
        }
        if let Some(rest) = src.strip_prefix(&home)
            && rest.starts_with('/')
        {
            return format!("~{}", rest);
        }
    }
    src.to_string()
}

/// Format a path for candidate rows.
///
/// - Shows [`relative_display`] instead of absolute paths.
/// - Never cuts the last segment in the middle: over-long paths are
///   shortened head-first (`first/…/parent/last`, then `…/parent/last`,
///   then `…` + tail of the last segment).
/// - With `color`, the last segment is bold (readable on both dark and
///   light themes) and the leading part is dimmed.
pub fn paint_path(path: &str, max_len: usize, color: bool) -> String {
    let rel = relative_display(path);
    let max_len = max_len.max(8);
    let shortened = shorten_head_first(&rel, max_len);
    if !color {
        return shortened;
    }
    // Emphasize the last segment; dim the leading part.
    match shortened.rfind('/') {
        Some(idx) => {
            let (head, last) = shortened.split_at(idx + 1);
            format!(
                "{}{}{}{}{}{}",
                crate::presentation::theme::ANSI_DIM,
                head,
                crate::presentation::theme::ANSI_RESET,
                crate::presentation::theme::ANSI_BOLD,
                last,
                crate::presentation::theme::ANSI_RESET,
            )
        }
        None => format!(
            "{}{}{}",
            crate::presentation::theme::ANSI_BOLD,
            shortened,
            crate::presentation::theme::ANSI_RESET,
        ),
    }
}

fn shorten_head_first(rel: &str, max_len: usize) -> String {
    if display_width(rel) <= max_len {
        return rel.to_string();
    }
    let segs: Vec<&str> = rel.split('/').collect();
    if segs.len() >= 3 {
        // Keep the first segment plus the last two: `~/…/parent/last`.
        let first = segs[0];
        let tail = segs[segs.len() - 2..].join("/");
        let candidate = format!("{}/\u{2026}/{}", first, tail);
        if display_width(&candidate) <= max_len {
            return candidate;
        }
    }
    if segs.len() >= 2 {
        // Keep the last two segments: `…/parent/last`.
        let tail = segs[segs.len() - 2..].join("/");
        let candidate = format!("\u{2026}/{}", tail);
        if display_width(&candidate) <= max_len {
            return candidate;
        }
        // Keep only the last segment.
        let last = segs[segs.len() - 1];
        let candidate = format!("\u{2026}/{}", last);
        if display_width(&candidate) <= max_len {
            return candidate;
        }
    }
    // Last resort: left-truncate, keeping the tail intact.
    let chars: Vec<char> = rel.chars().collect();
    let keep = max_len.saturating_sub(1).max(1);
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("\u{2026}{}", tail)
}

pub fn display_width(s: &str) -> usize {
    let mut w = 0;
    for c in s.chars() {
        w += if c.is_ascii() {
            1
        } else if c.width_fallback() {
            2
        } else {
            1
        };
    }
    w
}

trait WidthFallback {
    fn width_fallback(&self) -> bool;
}

impl WidthFallback for char {
    fn width_fallback(&self) -> bool {
        let n = *self as u32;
        (0x1100..=0x115F).contains(&n)
            || (0x2E80..=0xA4CF).contains(&n)
            || (0xAC00..=0xD7A3).contains(&n)
            || (0xF900..=0xFAFF).contains(&n)
            || (0xFE30..=0xFE4F).contains(&n)
            || (0xFF00..=0xFF60).contains(&n)
            || (0xFFE0..=0xFFE6).contains(&n)
    }
}

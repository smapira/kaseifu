pub fn short_path(path: &str, max_len: usize) -> String {
    if path.chars().count() <= max_len {
        return path.to_string();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut display = if !home.is_empty() && path.starts_with(&home) {
        path.replacen(&home, "~", 1)
    } else {
        path.to_string()
    };
    if display.chars().count() <= max_len {
        return display;
    }
    let chars: Vec<char> = display.chars().collect();
    if max_len <= 1 {
        return chars[..max_len].iter().collect();
    }
    let keep = max_len - 1;
    let tail: String = chars[chars.len() - keep..].iter().collect();
    display = format!("\u{2026}{}", tail);
    display
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

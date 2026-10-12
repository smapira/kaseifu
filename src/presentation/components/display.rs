//! Shared human-friendly formatting helpers (v3.1).
//!
//! Byte sizes, datetimes and directory-entry markers used by the classify
//! and inspect Rich renderers. Everything here is width-safe (no emoji:
//! emoji break the [`crate::presentation::components::path::visible_width`]
//! accounting on narrow terminals) and never crashes on missing data.

use crate::models::file_context::EntryKind;

/// Placeholder shown when a field could not be obtained.
pub const MISSING: &str = "\u{2014}";

/// `126597` -> `"123.6 KiB"`. Binary units, one decimal above bytes.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.1} {}", value, UNITS[unit])
}

/// RFC 3339 (`2022-06-07T08:56:00+00:00`) -> `"2022-06-07 08:56"`.
///
/// `None` renders as [`MISSING`]; unparseable input is shown verbatim so
/// no information is lost and nothing crashes.
pub fn format_datetime(raw: Option<&str>) -> String {
    match raw {
        None => MISSING.to_string(),
        Some(text) => chrono::DateTime::parse_from_rfc3339(text)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|_| text.to_string()),
    }
}

/// `ls -p` style marker: directories get `/`, symlinks `@`, files none.
/// Width-safe by construction (pure ASCII).
pub fn entry_suffix(kind: &EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "/",
        EntryKind::Symlink => "@",
        _ => "",
    }
}

/// Split a semantic signal at its first `:` (`"K:PDF"` -> `("K", "PDF")`).
/// Signals without a prefix render whole under the `"?"` group.
pub fn split_signal(signal: &str) -> (&str, &str) {
    match signal.split_once(':') {
        Some((prefix, rest)) => (prefix, rest),
        None => ("?", signal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_boundaries() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(126597), "123.6 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn datetime_rfc3339_shortens() {
        assert_eq!(
            format_datetime(Some("2022-06-07T08:56:00+00:00")),
            "2022-06-07 08:56"
        );
        assert_eq!(format_datetime(None), MISSING);
        assert_eq!(format_datetime(Some("not-a-date")), "not-a-date");
    }

    #[test]
    fn suffixes_are_ascii() {
        assert_eq!(entry_suffix(&EntryKind::Directory), "/");
        assert_eq!(entry_suffix(&EntryKind::File), "");
        assert_eq!(entry_suffix(&EntryKind::Symlink), "@");
    }
}

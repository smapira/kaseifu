//! Bounded document input (original spec section 5.1).
//!
//! The whole file is never read: at most [`MAX_DOCUMENT_BYTES`] from the
//! start are inspected, and at most [`MAX_TEXT_SIGNALS`] text excerpts travel
//! to the model as `T:` semantic signals (inside the existing contract
//! bounds, so neither the contract nor Swift changes). Unreadable parts are
//! treated as empty, never fabricated.

use std::io::Read;
use std::path::Path;

/// How many bytes from the start of the file are inspected.
pub const MAX_DOCUMENT_BYTES: usize = 32_768;

/// How many text excerpts become `T:` signals.
pub const MAX_TEXT_SIGNALS: usize = 12;

/// Character budget of one `T:` signal (matches the contract bound).
pub const SIGNAL_CHARS: usize = 120;

/// Document kinds Phase 4 accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// Plain text and Markdown.
    Text,
    /// HTML treated as text (tags blanked, never rendered).
    Html,
    /// PDF scanned for printable fragments (no decryption, no inflation).
    Pdf,
}

/// Why a file is not a Phase 4 document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    /// No extension, or an extension outside txt/md/pdf/html.
    UnsupportedType(String),
    /// The file could not be read.
    Unreadable(String),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedType(detail) => write!(f, "{detail}"),
            Self::Unreadable(detail) => write!(f, "cannot read document: {detail}"),
        }
    }
}

/// Classify a path by extension (case-insensitive).
///
/// Images, video, folders and extension-less files are refused with the
/// phase that owns them, so the caller never guesses.
pub fn classify_document(path: &Path) -> Result<DocumentKind, DocumentError> {
    let raw = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let extension = raw.to_ascii_lowercase();
    match extension.as_str() {
        "txt" | "md" | "markdown" => Ok(DocumentKind::Text),
        "html" | "htm" => Ok(DocumentKind::Html),
        "pdf" => Ok(DocumentKind::Pdf),
        "" => Err(DocumentError::UnsupportedType(format!(
            "{} has no extension; rename needs txt, md, pdf or html",
            path.display()
        ))),
        "jpg" | "jpeg" | "png" | "heic" | "heif" | "hif" | "tif" | "tiff" | "webp" => {
            Err(DocumentError::UnsupportedType(format!(
                ".{raw} is an image; image rename arrives with Phase 5"
            )))
        }
        "mp4" | "mov" | "m4v" => Err(DocumentError::UnsupportedType(format!(
            ".{raw} is a video; video rename arrives with Phase 6"
        ))),
        _ => Err(DocumentError::UnsupportedType(format!(
            ".{raw} is not a Phase 4 document (txt, md, pdf, html)"
        ))),
    }
}

/// Read at most `limit` bytes from the start of the file.
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, DocumentError> {
    let file =
        std::fs::File::open(path).map_err(|error| DocumentError::Unreadable(error.to_string()))?;
    let mut bounded = file.take(limit as u64);
    let mut bytes = Vec::new();
    bounded
        .read_to_end(&mut bytes)
        .map_err(|error| DocumentError::Unreadable(error.to_string()))?;
    Ok(bytes)
}

/// Extract `T:` signals from bounded bytes.
///
/// Text and HTML are decoded lossy (HTML tags become blanks); PDF binaries
/// contribute only their printable runs. Signals never carry instructions:
/// they are values the Swift prompt already treats as data.
pub fn extract_signals(kind: DocumentKind, bytes: &[u8]) -> Vec<String> {
    let text = match kind {
        DocumentKind::Text => String::from_utf8_lossy(bytes).into_owned(),
        DocumentKind::Html => blank_tags(&String::from_utf8_lossy(bytes)),
        DocumentKind::Pdf => printable_runs(bytes),
    };
    chunk_signals(&text)
}

/// Replace `<...>` tags with blanks (single pass, no rendering).
fn blank_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut inside = false;
    for ch in text.chars() {
        if ch == '<' {
            inside = true;
        }
        if inside {
            if ch == '>' {
                inside = false;
                output.push(' ');
            }
            continue;
        }
        output.push(ch);
    }
    output
}

/// Collect printable ASCII runs (length 3 or more) from binary bytes.
fn printable_runs(bytes: &[u8]) -> String {
    let mut runs: Vec<&str> = Vec::new();
    let mut start: Option<usize> = None;
    for (index, byte) in bytes.iter().enumerate() {
        let printable = (0x20..=0x7e).contains(byte) || *byte == b'\n' || *byte == b'\t';
        if printable {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(begin) = start.take() {
            push_run(bytes, begin, index, &mut runs);
        }
    }
    if let Some(begin) = start {
        push_run(bytes, begin, bytes.len(), &mut runs);
    }
    runs.join(" ")
}

/// Push one byte range when it holds a long enough printable run.
fn push_run<'a>(bytes: &'a [u8], begin: usize, end: usize, runs: &mut Vec<&'a str>) {
    if end.saturating_sub(begin) < 3 {
        return;
    }
    if let Ok(fragment) = std::str::from_utf8(&bytes[begin..end]) {
        let trimmed = fragment.trim();
        if trimmed.len() >= 3 {
            runs.push(trimmed);
        }
    }
}

/// Pack whitespace-separated words into at most [`MAX_TEXT_SIGNALS`]
/// `T:` signals of at most [`SIGNAL_CHARS`] characters.
fn chunk_signals(text: &str) -> Vec<String> {
    let mut signals = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let extra = word.len() + usize::from(!current.is_empty());
        if current.len() + extra > SIGNAL_CHARS {
            push_signal(&mut signals, &mut current);
            if signals.len() >= MAX_TEXT_SIGNALS {
                return signals;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    push_signal(&mut signals, &mut current);
    signals
}

/// Move a full chunk into the signal list.
fn push_signal(signals: &mut Vec<String>, current: &mut String) {
    if current.is_empty() {
        return;
    }
    let mut chunk = String::new();
    std::mem::swap(&mut chunk, current);
    // Never split a character: the chunk was built from whole words, but a
    // single word may exceed the budget on its own.
    let mut bounded = String::new();
    for ch in chunk.chars() {
        if bounded.len() + ch.len_utf8() > SIGNAL_CHARS {
            break;
        }
        bounded.push(ch);
    }
    if !bounded.is_empty() && signals.len() < MAX_TEXT_SIGNALS {
        signals.push(format!("T:{bounded}"));
    }
}

/// A document date scan result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentDate {
    /// No clear date found.
    None,
    /// Exactly one distinct clear date.
    One(i32, u32, u32),
    /// Several distinct dates: ambiguous, must not be used.
    Ambiguous,
}

/// Scan text for an unambiguous calendar date.
///
/// Only explicit forms count (`YYYY-MM-DD`, `YYYY/MM/DD`, `YYYYMMDD`).
/// Relative dates ("tomorrow") never match. Two distinct dates mean
/// [`DocumentDate::Ambiguous`]: the caller falls through to the next date
/// source instead of asking the model to pick one.
pub fn scan_document_date(text: &str) -> DocumentDate {
    let mut found: Option<(i32, u32, u32)> = None;
    let mut ambiguous = false;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if let Some((ymd, consumed)) = match_date_at(bytes, index) {
            match found {
                current if current == Some(ymd) => {}
                None => found = Some(ymd),
                _ => ambiguous = true,
            }
            index += consumed;
        } else {
            index += 1;
        }
    }
    if ambiguous {
        return DocumentDate::Ambiguous;
    }
    match found {
        Some((year, month, day)) => DocumentDate::One(year, month, day),
        Option::None => DocumentDate::None,
    }
}

/// Try to match a date at a byte offset (ASCII digits only).
fn match_date_at(bytes: &[u8], index: usize) -> Option<((i32, u32, u32), usize)> {
    if let Some(ymd) = match_separated(bytes, index) {
        return Some((ymd, 10));
    }
    if let Some(ymd) = match_compact(bytes, index) {
        return Some((ymd, 8));
    }
    Option::None
}

/// Match `YYYY-MM-DD` or `YYYY/MM/DD` (separators must agree).
fn match_separated(bytes: &[u8], index: usize) -> Option<(i32, u32, u32)> {
    if index + 10 > bytes.len() {
        return Option::None;
    }
    let year = digits_value(bytes, index, 4)?;
    let first = bytes[index + 4];
    if first != b'-' && first != b'/' {
        return Option::None;
    }
    if bytes[index + 7] != first {
        return Option::None;
    }
    let month = digits_value(bytes, index + 5, 2)?;
    let day = digits_value(bytes, index + 8, 2)?;
    check_ymd(year, month, day)
}

/// Match `YYYYMMDD`.
fn match_compact(bytes: &[u8], index: usize) -> Option<(i32, u32, u32)> {
    if index + 8 > bytes.len() {
        return Option::None;
    }
    // A compact match must not sit inside a longer digit run.
    if index > 0 && bytes[index - 1].is_ascii_digit() {
        return Option::None;
    }
    if index + 8 < bytes.len() && bytes[index + 8].is_ascii_digit() {
        return Option::None;
    }
    let year = digits_value(bytes, index, 4)?;
    let month = digits_value(bytes, index + 4, 2)?;
    let day = digits_value(bytes, index + 6, 2)?;
    check_ymd(year, month, day)
}

/// Read `len` ASCII digits as a number.
fn digits_value(bytes: &[u8], start: usize, len: usize) -> Option<i32> {
    if start + len > bytes.len() {
        return Option::None;
    }
    let mut value: i32 = 0;
    for byte in &bytes[start..start + len] {
        if !byte.is_ascii_digit() {
            return Option::None;
        }
        value = value * 10 + i32::from(*byte - b'0');
    }
    Some(value)
}

/// Accept plausible calendar values only.
fn check_ymd(year: i32, month: i32, day: i32) -> Option<(i32, u32, u32)> {
    if !(1900..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Option::None;
    }
    Some((year, month as u32, day as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extensions_classify_case_insensitively() {
        assert_eq!(
            classify_document(&PathBuf::from("a.txt")),
            Ok(DocumentKind::Text)
        );
        assert_eq!(
            classify_document(&PathBuf::from("a.MD")),
            Ok(DocumentKind::Text)
        );
        assert_eq!(
            classify_document(&PathBuf::from("a.Markdown")),
            Ok(DocumentKind::Text)
        );
        assert_eq!(
            classify_document(&PathBuf::from("a.pdf")),
            Ok(DocumentKind::Pdf)
        );
        assert_eq!(
            classify_document(&PathBuf::from("a.HTML")),
            Ok(DocumentKind::Html)
        );
        assert!(matches!(
            classify_document(&PathBuf::from("a.jpg")),
            Err(DocumentError::UnsupportedType(_))
        ));
        assert!(matches!(
            classify_document(&PathBuf::from("a.mp4")),
            Err(DocumentError::UnsupportedType(_))
        ));
        assert!(matches!(
            classify_document(&PathBuf::from("noext")),
            Err(DocumentError::UnsupportedType(_))
        ));
    }

    #[test]
    fn reads_stop_at_the_byte_budget() {
        let dir = std::env::temp_dir().join(format!("kaseifu-rename-doc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.txt");
        std::fs::write(&path, vec![b'x'; MAX_DOCUMENT_BYTES + 1024]).expect("fixture");
        let bytes = read_bounded(&path, MAX_DOCUMENT_BYTES).expect("bounded read");
        assert_eq!(bytes.len(), MAX_DOCUMENT_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signals_stay_within_the_budgets() {
        let text = "word ".repeat(5000);
        let signals = extract_signals(DocumentKind::Text, text.as_bytes());
        assert!(!signals.is_empty());
        assert!(signals.len() <= MAX_TEXT_SIGNALS);
        for signal in &signals {
            assert!(signal.starts_with("T:"));
            assert!(signal.len() <= "T:".len() + SIGNAL_CHARS);
        }
    }

    #[test]
    fn html_tags_become_blanks() {
        let signals = extract_signals(DocumentKind::Html, b"<h1>Hello</h1><p>World</p>");
        let joined = signals.join(" ");
        assert!(joined.contains("Hello"));
        assert!(joined.contains("World"));
        assert!(!joined.contains("<h1>"));
    }

    #[test]
    fn pdf_printable_runs_are_scanned() {
        let mut bytes = vec![0u8, 1, 2];
        bytes.extend_from_slice(b"Meeting notes for October");
        bytes.extend_from_slice(&[0xff, 0xfe]);
        let signals = extract_signals(DocumentKind::Pdf, &bytes);
        assert!(signals.join(" ").contains("Meeting"));
    }

    #[test]
    fn document_dates_need_one_clear_value() {
        assert_eq!(scan_document_date("no dates here"), DocumentDate::None);
        assert_eq!(
            scan_document_date("dated 2026-10-14 ok"),
            DocumentDate::One(2026, 10, 14)
        );
        assert_eq!(
            scan_document_date("dated 2026/10/14 ok"),
            DocumentDate::One(2026, 10, 14)
        );
        assert_eq!(
            scan_document_date("dated 20261014 ok"),
            DocumentDate::One(2026, 10, 14)
        );
        assert_eq!(
            scan_document_date("2026-10-14 and 2026-10-15"),
            DocumentDate::Ambiguous
        );
        assert_eq!(
            scan_document_date("2026-10-14 and again 2026-10-14"),
            DocumentDate::One(2026, 10, 14)
        );
        assert_eq!(scan_document_date("tomorrow"), DocumentDate::None);
    }
}

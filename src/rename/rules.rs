//! Naming Rule Engine (original spec section 6).
//!
//! Applies the user-selected naming rule in Rust: builtin presets P01-P12,
//! template variables, date formats and the date source priority. A template
//! whose required variable has no value is held back, never fabricated
//! (original spec 6.5).

use chrono::{DateTime, Datelike, Local, NaiveDateTime, Timelike};

/// One builtin preset (original spec section 6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetDef {
    /// `P01` .. `P12`.
    pub id: &'static str,
    /// CLI slug (`ai`, `date-title`, ...).
    pub slug: &'static str,
    /// Template with `{variable}` placeholders.
    pub template: &'static str,
    /// ASCII description for `--patterns`.
    pub description: &'static str,
}

/// The twelve builtin presets (code constants, never seeded into SQLite).
///
/// P12 (`custom`) has no fixed template: the caller must pass `--template`.
/// Device names come from image EXIF, so P11 is held back for documents.
pub const PRESETS: [PresetDef; 12] = [
    PresetDef {
        id: "P01",
        slug: "ai",
        template: "{title}",
        description: "AI title",
    },
    PresetDef {
        id: "P02",
        slug: "date-title",
        template: "{date}_{title}",
        description: "date prefix",
    },
    PresetDef {
        id: "P03",
        slug: "title-date",
        template: "{title}_{date}",
        description: "date suffix",
    },
    PresetDef {
        id: "P04",
        slug: "prefix-title",
        template: "{prefix}_{title}",
        description: "fixed prefix",
    },
    PresetDef {
        id: "P05",
        slug: "title-suffix",
        template: "{title}_{suffix}",
        description: "fixed suffix",
    },
    PresetDef {
        id: "P06",
        slug: "category-title",
        template: "{category}_{title}",
        description: "category plus title",
    },
    PresetDef {
        id: "P07",
        slug: "date-category-title",
        template: "{date}_{category}_{title}",
        description: "date plus category",
    },
    PresetDef {
        id: "P08",
        slug: "seq-title",
        template: "{seq}_{title}",
        description: "sequence number",
    },
    PresetDef {
        id: "P09",
        slug: "date-seq-title",
        template: "{date}_{seq}_{title}",
        description: "date plus sequence",
    },
    PresetDef {
        id: "P10",
        slug: "project-title",
        template: "{project}_{title}",
        description: "project plus title",
    },
    PresetDef {
        id: "P11",
        slug: "meta",
        template: "{date}_{device}_{title}",
        description: "capture metadata",
    },
    PresetDef {
        id: "P12",
        slug: "custom",
        template: "",
        description: "user template via --template",
    },
];

/// The shipped default rule (used when no default was stored).
pub const DEFAULT_RULE_ID: &str = "P02";

/// Supported date formats (original spec section 6.4).
pub const DATE_FORMATS: [&str; 5] = [
    "YYYY-MM-DD",
    "YYYYMMDD",
    "YYYY-MM",
    "YYYY",
    "YYYY\u{5e74}MM\u{6708}DD\u{65e5}",
];

/// Find a preset by id (`P02`, case-insensitive) or slug (`date-title`).
pub fn find_preset(spec: &str) -> Option<PresetDef> {
    let trimmed = spec.trim();
    PRESETS
        .iter()
        .find(|preset| preset.id.eq_ignore_ascii_case(trimmed) || preset.slug == trimmed)
        .copied()
}

/// True for a supported date format name.
pub fn is_date_format(name: &str) -> bool {
    DATE_FORMATS.contains(&name)
}

/// Values available to template variables.
#[derive(Debug, Clone, Default)]
pub struct RuleInputs {
    pub title: String,
    pub category: String,
    pub project: String,
    pub prefix: String,
    pub suffix: String,
    pub date: ResolvedDate,
    pub seq: u32,
    pub parent: String,
    pub device: String,
    pub file_type: String,
}

/// A resolved calendar date with its provenance.
#[derive(Debug, Clone)]
pub struct ResolvedDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// `created`, `modified`, `exif`, `document` or `now`.
    pub source: &'static str,
}

impl Default for ResolvedDate {
    fn default() -> Self {
        let now = Local::now();
        Self {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            second: now.second(),
            source: "now",
        }
    }
}

impl ResolvedDate {
    /// Format the date with one of [`DATE_FORMATS`].
    pub fn format(&self, format: &str) -> Option<String> {
        match format {
            "YYYY-MM-DD" => Some(format!("{}-{:02}-{:02}", self.year, self.month, self.day)),
            "YYYYMMDD" => {
                Some(format!("{} {:02}{:02}", self.year, self.month, self.day).replace(' ', ""))
            }
            "YYYY-MM" => Some(format!("{}-{:02}", self.year, self.month)),
            "YYYY" => Some(format!("{}", self.year)),
            "YYYY\u{5e74}MM\u{6708}DD\u{65e5}" => Some(format!(
                "{}年{:02}月{:02}日",
                self.year, self.month, self.day
            )),
            _ => None,
        }
    }

    /// The time of day as `HH-MM-SS` (colons are avoided for file names).
    pub fn time_label(&self) -> String {
        format!("{:02}-{:02}-{:02}", self.hour, self.minute, self.second)
    }
}

/// Where a resolved date came from, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePreference {
    Created,
    Modified,
    Exif,
    Document,
    Now,
}

/// Resolve the naming date (original spec section 6.4 priority).
///
/// Each source is an RFC 3339 timestamp, except `document_ymd` which is an
/// unambiguous calendar date found in the document text. Unparseable values
/// are skipped; when every source is missing the current time is used.
pub fn resolve_date(
    created: Option<&str>,
    modified: Option<&str>,
    exif: Option<&str>,
    document_ymd: Option<(i32, u32, u32)>,
) -> (ResolvedDate, DatePreference) {
    if let Some(date) = parse_datetime(created, "created") {
        return (date, DatePreference::Created);
    }
    if let Some(date) = parse_datetime(modified, "modified") {
        return (date, DatePreference::Modified);
    }
    if let Some(date) = parse_datetime(exif, "exif") {
        return (date, DatePreference::Exif);
    }
    if let Some((year, month, day)) = document_ymd {
        return (
            ResolvedDate {
                year,
                month,
                day,
                hour: 0,
                minute: 0,
                second: 0,
                source: "document",
            },
            DatePreference::Document,
        );
    }
    (ResolvedDate::default(), DatePreference::Now)
}

/// Parse one timestamp into a [`ResolvedDate`].
///
/// RFC 3339 first, then zone-less wall-clock forms (`YYYY-MM-DDTHH:MM:SS`
/// and `YYYY-MM-DD HH:MM:SS`, the EXIF normalized shape). Zones are never
/// guessed: wall-clock fields are used as-is, no offset is invented.
fn parse_datetime(value: Option<&str>, source: &'static str) -> Option<ResolvedDate> {
    let text = value?.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(resolved_from(parsed, source));
    }
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    Some(ResolvedDate {
        year: naive.date().year(),
        month: naive.date().month(),
        day: naive.date().day(),
        hour: naive.time().hour(),
        minute: naive.time().minute(),
        second: naive.time().second(),
        source,
    })
}

/// Build a [`ResolvedDate`] from an offset-aware timestamp.
fn resolved_from<Tz>(parsed: DateTime<Tz>, source: &'static str) -> ResolvedDate
where
    Tz: chrono::TimeZone,
{
    ResolvedDate {
        year: parsed.year(),
        month: parsed.month(),
        day: parsed.day(),
        hour: parsed.hour(),
        minute: parsed.minute(),
        second: parsed.second(),
        source,
    }
}

/// Resolve the naming date for an image target (original spec section 5.2).
///
/// EXIF capture comes first: filesystem dates of photos are transfer times,
/// not capture times. Then creation, modification, now. No document date:
/// images have no text scan, so the caller always passes no document value.
/// Unparseable values are skipped; when every source is missing the current
/// time is used.
pub fn resolve_image_date(
    created: Option<&str>,
    modified: Option<&str>,
    exif: Option<&str>,
) -> (ResolvedDate, DatePreference) {
    if let Some(date) = parse_datetime(exif, "exif") {
        return (date, DatePreference::Exif);
    }
    if let Some(date) = parse_datetime(created, "created") {
        return (date, DatePreference::Created);
    }
    if let Some(date) = parse_datetime(modified, "modified") {
        return (date, DatePreference::Modified);
    }
    (ResolvedDate::default(), DatePreference::Now)
}

/// Why a template could not produce a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The template names a variable that does not exist.
    UnknownVariable(String),
    /// A required variable has no value: the proposal is held back.
    MissingValue(String),
    /// The date format name is not supported.
    BadDateFormat(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariable(name) => write!(f, "unknown variable {{{name}}}"),
            Self::MissingValue(name) => write!(f, "missing: {name}"),
            Self::BadDateFormat(name) => write!(f, "unknown date format: {name}"),
        }
    }
}

/// Apply a template to the inputs.
///
/// Every `{variable}` used must resolve to a non-empty value; an empty value
/// holds the proposal back ([`ResolveError::MissingValue`]) instead of being
/// fabricated. Unknown variables are an error, never silently empty.
pub fn resolve_template(
    template: &str,
    inputs: &RuleInputs,
    date_format: &str,
) -> Result<String, ResolveError> {
    let mut output = String::with_capacity(template.len() + 32);
    let mut rest = template;
    loop {
        let Some(open) = rest.find('{') else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            output.push_str(&rest[open..]);
            break;
        };
        let name = &after[..close];
        let value = variable_value(name, inputs, date_format)?;
        if value.is_empty() {
            return Err(ResolveError::MissingValue(name.to_string()));
        }
        output.push_str(&value);
        rest = &after[close + 1..];
    }
    Ok(output)
}

/// Maximum stem length (the extension is extra): model titles longer than
/// this are cut back to a word boundary instead of becoming sentences.
pub const MAX_STEM_CHARS: usize = 80;

/// Bound a resolved stem to [`MAX_STEM_CHARS`] characters.
///
/// Over-long titles are cut at the last space inside the budget so words are
/// never split; a single word longer than the budget is hard-cut (still a
/// valid, bounded name). Short stems pass through untouched.
pub fn bound_stem(stem: &str) -> String {
    if stem.chars().count() <= MAX_STEM_CHARS {
        return stem.to_string();
    }
    let head: String = stem.chars().take(MAX_STEM_CHARS).collect();
    match head.rfind(' ') {
        Some(index) => head[..index].to_string(),
        Option::None => head,
    }
}

/// Replace whitespace runs with single underscores.
///
/// Spaces break shell tab-completion, so model titles never keep them:
/// `Budget review` becomes `Budget_review`. Leading, trailing and repeated
/// whitespace (including tabs and newlines) collapse away.
pub fn underscore_whitespace(stem: &str) -> String {
    stem.split_whitespace().collect::<Vec<_>>().join("_")
}

/// Drop a trailing `.{extension}` the model echoed back into the title.
///
/// The extension is owned by the rule engine (`finish_plan` appends it), so a
/// case-insensitive trailing match is the model's echo, not content: without
/// this, a `note.txt` title becomes `note.txt.txt`.
pub fn strip_redundant_extension(stem: &str, extension: &str) -> String {
    if extension.is_empty() {
        return stem.to_string();
    }
    match stem.rsplit_once('.') {
        Some((head, tail)) if tail.eq_ignore_ascii_case(extension) => head.to_string(),
        _ => stem.to_string(),
    }
}

/// Check a template without producing a name (used when saving presets).
///
/// Unknown variables and bad date formats are rejected; missing values are
/// allowed here because the preset may be used with richer inputs later.
pub fn validate_template_shape(template: &str, date_format: &str) -> Result<(), ResolveError> {
    if template.trim().is_empty() {
        return Err(ResolveError::MissingValue("template".to_string()));
    }
    if !is_date_format(date_format) {
        return Err(ResolveError::BadDateFormat(date_format.to_string()));
    }
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let name = &after[..close];
        if !variable_known(name) {
            return Err(ResolveError::UnknownVariable(name.to_string()));
        }
        rest = &after[close + 1..];
    }
    Ok(())
}

/// True when the name is a known template variable.
fn variable_known(name: &str) -> bool {
    matches!(
        name,
        "title"
            | "category"
            | "project"
            | "prefix"
            | "suffix"
            | "parent"
            | "device"
            | "type"
            | "seq"
            | "date"
            | "time"
            | "year"
            | "month"
    )
}

/// Resolve one variable against the inputs.
fn variable_value(
    name: &str,
    inputs: &RuleInputs,
    date_format: &str,
) -> Result<String, ResolveError> {
    match name {
        "title" => Ok(inputs.title.clone()),
        "category" => Ok(inputs.category.clone()),
        "project" => Ok(inputs.project.clone()),
        "prefix" => Ok(inputs.prefix.clone()),
        "suffix" => Ok(inputs.suffix.clone()),
        "parent" => Ok(inputs.parent.clone()),
        "device" => Ok(inputs.device.clone()),
        "type" => Ok(inputs.file_type.clone()),
        "seq" => Ok(format!("{:03}", inputs.seq)),
        _ => date_variable(name, inputs, date_format),
    }
}

/// Resolve the date-derived variables (`date`, `time`, `year`, `month`).
fn date_variable(
    name: &str,
    inputs: &RuleInputs,
    date_format: &str,
) -> Result<String, ResolveError> {
    match name {
        "date" => inputs
            .date
            .format(date_format)
            .ok_or_else(|| ResolveError::BadDateFormat(date_format.to_string())),
        "time" => Ok(inputs.date.time_label()),
        "year" => Ok(format!("{}", inputs.date.year)),
        "month" => Ok(format!("{:02}", inputs.date.month)),
        _ => Err(ResolveError::UnknownVariable(name.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_inputs() -> RuleInputs {
        RuleInputs {
            title: "title".to_string(),
            category: "category".to_string(),
            project: "project".to_string(),
            prefix: "prefix".to_string(),
            suffix: "suffix".to_string(),
            date: ResolvedDate {
                year: 2026,
                month: 10,
                day: 14,
                hour: 9,
                minute: 30,
                second: 5,
                source: "modified",
            },
            seq: 3,
            parent: "Documents".to_string(),
            device: "Camera".to_string(),
            file_type: "pdf".to_string(),
        }
    }

    #[test]
    fn builtin_presets_resolve_to_the_documented_names() {
        let inputs = sample_inputs();
        let cases = [
            ("P01", "title"),
            ("P02", "2026-10-14_title"),
            ("P03", "title_2026-10-14"),
            ("P04", "prefix_title"),
            ("P05", "title_suffix"),
            ("P06", "category_title"),
            ("P07", "2026-10-14_category_title"),
            ("P08", "003_title"),
            ("P09", "2026-10-14_003_title"),
            ("P10", "project_title"),
            ("P11", "2026-10-14_Camera_title"),
        ];
        for (id, expected) in cases {
            let preset = find_preset(id).expect("builtin preset exists");
            assert_eq!(
                resolve_template(preset.template, &inputs, "YYYY-MM-DD"),
                Ok(expected.to_string()),
                "{id}"
            );
        }
    }

    #[test]
    fn patterns_accept_ids_and_slugs() {
        assert_eq!(find_preset("P02").map(|preset| preset.id), Some("P02"));
        assert_eq!(find_preset("p02").map(|preset| preset.id), Some("P02"));
        assert_eq!(
            find_preset("date-title").map(|preset| preset.id),
            Some("P02")
        );
        assert_eq!(find_preset("ai").map(|preset| preset.id), Some("P01"));
        assert!(find_preset("P13").is_none());
        assert!(find_preset("nope").is_none());
    }

    #[test]
    fn date_formats_render() {
        let date = sample_inputs().date;
        assert_eq!(date.format("YYYY-MM-DD").as_deref(), Some("2026-10-14"));
        assert_eq!(date.format("YYYYMMDD").as_deref(), Some("20261014"));
        assert_eq!(date.format("YYYY-MM").as_deref(), Some("2026-10"));
        assert_eq!(date.format("YYYY").as_deref(), Some("2026"));
        assert!(
            date.format("YYYY\u{5e74}MM\u{6708}DD\u{65e5}")
                .unwrap()
                .contains("2026")
        );
        assert!(date.format("DD-MM-YYYY").is_none());
        assert_eq!(date.time_label(), "09-30-05");
    }

    #[test]
    fn missing_values_hold_the_proposal_back() {
        let mut inputs = sample_inputs();
        inputs.project.clear();
        assert_eq!(
            resolve_template("{project}_{title}", &inputs, "YYYY-MM-DD"),
            Err(ResolveError::MissingValue("project".to_string()))
        );
        inputs.title.clear();
        assert_eq!(
            resolve_template("{title}", &inputs, "YYYY-MM-DD"),
            Err(ResolveError::MissingValue("title".to_string()))
        );
    }

    #[test]
    fn unknown_variables_are_an_error() {
        let inputs = sample_inputs();
        assert_eq!(
            resolve_template("{unknown}_{title}", &inputs, "YYYY-MM-DD"),
            Err(ResolveError::UnknownVariable("unknown".to_string()))
        );
    }

    #[test]
    fn date_resolution_follows_the_priority_order() {
        let created = Some("2026-10-12T08:00:00+00:00");
        let modified = Some("2026-10-13T08:00:00+00:00");
        let (date, preference) = resolve_date(created, modified, Option::None, Option::None);
        assert!(matches!(preference, DatePreference::Created));
        assert_eq!((date.year, date.month, date.day), (2026, 10, 12));
        let (date, preference) = resolve_date(Option::None, modified, Option::None, Option::None);
        assert!(matches!(preference, DatePreference::Modified));
        assert_eq!((date.year, date.month, date.day), (2026, 10, 13));
        let (date, preference) =
            resolve_date(Option::None, Option::None, Option::None, Some((2025, 5, 6)));
        assert!(matches!(preference, DatePreference::Document));
        assert_eq!((date.year, date.month, date.day), (2025, 5, 6));
        let (date, preference) =
            resolve_date(Option::None, Option::None, Option::None, Option::None);
        assert!(matches!(preference, DatePreference::Now));
        assert_eq!(date.source, "now");
    }

    #[test]
    fn unparseable_timestamps_fall_through() {
        let (date, preference) = resolve_date(
            Some("not-a-date"),
            Some("2026-10-13T08:00:00+00:00"),
            Option::None,
            Option::None,
        );
        assert!(matches!(preference, DatePreference::Modified));
        assert_eq!(date.source, "modified");
    }

    #[test]
    fn an_echoed_extension_is_stripped_once() {
        assert_eq!(strip_redundant_extension("note.txt", "txt"), "note");
        assert_eq!(strip_redundant_extension("NOTE.TXT", "txt"), "NOTE");
        assert_eq!(
            strip_redundant_extension("archive.tar", "txt"),
            "archive.tar"
        );
        assert_eq!(strip_redundant_extension("plain", "txt"), "plain");
        assert_eq!(strip_redundant_extension("plain", ""), "plain");
        assert_eq!(strip_redundant_extension(".txt", "txt"), "");
    }

    #[test]
    fn whitespace_runs_become_single_underscores() {
        assert_eq!(underscore_whitespace("Budget review"), "Budget_review");
        assert_eq!(underscore_whitespace("a  b\tc\nd"), "a_b_c_d");
        assert_eq!(underscore_whitespace("  padded  "), "padded");
        assert_eq!(underscore_whitespace("no-spaces"), "no-spaces");
        assert_eq!(underscore_whitespace(""), "");
    }

    #[test]
    fn long_stems_are_cut_at_a_word_boundary() {
        assert_eq!(bound_stem("short"), "short");
        let long = "word ".repeat(30);
        let bounded = bound_stem(&long);
        assert!(bounded.chars().count() <= MAX_STEM_CHARS);
        assert!(!bounded.ends_with(' '));
        assert!(long.starts_with(&bounded));
        let word = "x".repeat(100);
        assert_eq!(bound_stem(&word).len(), MAX_STEM_CHARS);
        assert_eq!(bound_stem(""), "");
    }
}

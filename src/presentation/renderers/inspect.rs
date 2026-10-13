//! v3.1 Inspect Rich renderer.
//!
//! Three display layers over one observation pass (`FileContext` plus its
//! normalization): L1 always (file basics + semantic signals), L2 summary
//! (parent + nearby counts + representative entries), L3 (`--verbose`/JSON:
//! full paths, all entries, raw metadata, schema version). Read-only.

use crate::models::file_context::{FileContext, NormalizedFileContext};
use crate::presentation::components::display::{
    MISSING, entry_suffix, format_bytes, format_datetime, split_signal,
};
use crate::presentation::components::panel::{self, Section};
use crate::presentation::components::path::{paint_path, path_budget, relative_display};
use crate::presentation::theme;

/// Default nearby-entry count; `--all` lifts the limit.
pub const NEARBY_LIMIT: usize = 5;
/// Evidence rows before truncation without `--all`.
pub const EVIDENCE_LIMIT: usize = 10;

fn label(key: &str, value: &str) -> String {
    format!("{:<8}  {}", key, value)
}

fn file_type_of(ctx: &FileContext, normalized: &NormalizedFileContext) -> String {
    if let Some(kind) = normalized.file_type.as_deref() {
        return kind.to_string();
    }
    match ctx.target.extension.as_deref() {
        Some(ext) => format!("{} file", ext.to_uppercase()),
        None => MISSING.to_string(),
    }
}

fn file_head(ctx: &FileContext, normalized: &NormalizedFileContext, color: bool) -> Vec<String> {
    vec![
        theme::paint("FILE", theme::ANSI_TEAL, color),
        ctx.target.filename.clone(),
        String::new(),
        label("Type", &file_type_of(ctx, normalized)),
        label("Size", &format_bytes(ctx.target.size_bytes)),
        label(
            "Modified",
            &format_datetime(ctx.target.modified_at.as_deref()),
        ),
        label(
            "Location",
            &paint_path(
                &ctx.directory_context.current.to_string_lossy(),
                path_budget(),
                color,
            ),
        ),
    ]
}

fn signals_lines(normalized: &NormalizedFileContext, color: bool) -> Vec<String> {
    let mut signals = Vec::new();
    if normalized.semantic_signals.is_empty() {
        signals.push(MISSING.to_string());
    } else {
        for signal in &normalized.semantic_signals {
            let (prefix, rest) = split_signal(signal);
            signals.push(format!(
                "{}  {}",
                theme::paint(prefix, theme::ANSI_AMBER, color),
                rest
            ));
        }
    }
    signals
}

fn directory_lines(ctx: &FileContext, show_all: bool, verbose: bool, color: bool) -> Vec<String> {
    let mut dir = Vec::new();
    dir.push(label(
        "Current",
        &paint_path(
            &ctx.directory_context.current.to_string_lossy(),
            path_budget(),
            color,
        ),
    ));
    dir.push(label(
        "Parent",
        &ctx.directory_context
            .parent
            .as_ref()
            .map(|p| paint_path(&p.to_string_lossy(), path_budget(), color))
            .unwrap_or_else(|| MISSING.to_string()),
    ));
    dir.push(String::new());
    let total = ctx.directory_context.entries.len();
    let shown = nearby_shown(total, show_all);
    dir.push(nearby_title(total, shown, show_all));
    for entry in ctx.directory_context.entries.iter().take(shown) {
        dir.push(format!(
            "  {}",
            nearby_entry_label(ctx, entry, verbose, color)
        ));
    }
    if !show_all && total > shown {
        dir.push(format!("Showing {} of {} (use --all)", shown, total));
    }
    dir
}

fn nearby_shown(total: usize, show_all: bool) -> usize {
    if show_all {
        total
    } else {
        total.min(NEARBY_LIMIT)
    }
}

fn nearby_title(total: usize, shown: usize, show_all: bool) -> String {
    if show_all || total <= NEARBY_LIMIT {
        format!("Nearby entries ({})", total)
    } else {
        format!("Nearby entries ({} of {})", shown, total)
    }
}

fn nearby_entry_label(
    ctx: &FileContext,
    entry: &crate::models::file_context::DirectoryEntry,
    verbose: bool,
    color: bool,
) -> String {
    if verbose {
        format!(
            "{}{}",
            relative_display(
                &ctx.directory_context
                    .current
                    .join(&entry.name)
                    .to_string_lossy()
            ),
            entry_suffix(&entry.kind)
        )
    } else {
        format!(
            "{}{}",
            paint_path(&entry.name, path_budget(), color),
            entry_suffix(&entry.kind)
        )
    }
}

fn details_lines(ctx: &FileContext, show_all: bool, color: bool) -> Vec<String> {
    let mut details = Vec::new();
    details.push(label("Schema", &ctx.schema_version));
    details.push(label("Target", &ctx.target.path.to_string_lossy()));
    let evidence_total = ctx.evidence.len();
    let evidence_shown = if show_all {
        evidence_total
    } else {
        evidence_total.min(EVIDENCE_LIMIT)
    };
    details.push(label("Evidence", &format!("{} entries", evidence_total)));
    for item in ctx.evidence.iter().take(evidence_shown) {
        details.push(evidence_line(item, show_all, color));
    }
    if !show_all && evidence_total > evidence_shown {
        details.push(format!(
            "Showing {} of {} evidence entries (use --all)",
            evidence_shown, evidence_total
        ));
    }
    details
}

fn evidence_line(
    item: &crate::models::file_context::Evidence,
    show_all: bool,
    color: bool,
) -> String {
    let value = item.value.to_string();
    let short = if show_all || value.len() <= 160 {
        value
    } else {
        format!("{}...", &value[..160])
    };
    let source = format!("{:?}", item.source);
    format!(
        "  [{}] {} = {}",
        source,
        item.key,
        theme::paint(&short, theme::ANSI_DIM, color)
    )
}

/// v3.2 capture metadata rows (ASCII only; missing renders as `-`).
fn capture_lines(normalized: &NormalizedFileContext) -> Vec<String> {
    let meta = normalized.image_metadata.as_ref();
    vec![
        label("Captured", &capture_display(meta)),
        label("Camera", &camera_display(meta)),
        label("Resol.", &resolution_display(meta)),
        label("Source", &capture_source(meta)),
    ]
}

fn capture_display(meta: Option<&crate::models::file_context::ImageMetadata>) -> String {
    let Some(meta) = meta else {
        return MISSING.to_string();
    };
    let Some(capture) = meta.capture_time.as_ref() else {
        return MISSING.to_string();
    };
    let mut shown = capture.value.replace('T', " ");
    if shown.len() > 16 {
        shown.truncate(16);
    }
    match &capture.timezone {
        Some(zone) if capture.timezone_known => format!("{shown} {zone}"),
        _ => shown,
    }
}

fn camera_display(meta: Option<&crate::models::file_context::ImageMetadata>) -> String {
    let Some(meta) = meta else {
        return MISSING.to_string();
    };
    match (&meta.camera.make, &meta.camera.model) {
        (Some(make), Some(model)) if make != model => format!("{make} {model}"),
        (Some(make), _) => make.clone(),
        (None, Some(model)) => model.clone(),
        (None, None) => MISSING.to_string(),
    }
}

fn resolution_display(meta: Option<&crate::models::file_context::ImageMetadata>) -> String {
    let dims = meta.and_then(|meta| meta.dimensions.as_ref());
    match dims {
        Some(dims) => format!("{} x {}", dims.width, dims.height),
        None => MISSING.to_string(),
    }
}

fn capture_source(meta: Option<&crate::models::file_context::ImageMetadata>) -> String {
    let source = meta
        .and_then(|meta| meta.capture_time.as_ref())
        .map(|capture| capture.source.as_str());
    match source {
        Some(source) => source.to_string(),
        None => MISSING.to_string(),
    }
}

/// v3.2 location rows (ASCII only; coordinates never fabricated).
fn location_lines(normalized: &NormalizedFileContext) -> Vec<String> {
    let location = normalized
        .image_metadata
        .as_ref()
        .and_then(|meta| meta.location.as_ref());
    vec![
        label("Lat.", &coord_display(location.map(|loc| loc.latitude))),
        label("Lon.", &coord_display(location.map(|loc| loc.longitude))),
        label(
            "Alt.",
            &altitude_display(location.and_then(|loc| loc.altitude_m)),
        ),
    ]
}

fn coord_display(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.6}"),
        None => MISSING.to_string(),
    }
}

fn altitude_display(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.1} m"),
        None => MISSING.to_string(),
    }
}

fn body_sections(
    ctx: &FileContext,
    normalized: &NormalizedFileContext,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> Vec<Section> {
    let mut sections = vec![
        Section::plain(file_head(ctx, normalized, color)),
        Section::headed("SEMANTIC SIGNALS", signals_lines(normalized, color)),
        Section::headed("CAPTURE METADATA", capture_lines(normalized)),
        Section::headed("LOCATION", location_lines(normalized)),
        Section::headed(
            "DIRECTORY CONTEXT",
            directory_lines(ctx, show_all, verbose, color),
        ),
    ];

    // DETAILS: third layer, verbose only (JSON always carries everything).
    if verbose {
        sections.push(Section::headed(
            "DETAILS",
            details_lines(ctx, show_all, color),
        ));
    }

    sections
}

/// Render the inspect panel (Rich output).
pub fn render_panel(
    ctx: &FileContext,
    normalized: &NormalizedFileContext,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> String {
    panel::frame_with_sections(
        "KASEIFU / INSPECT",
        &body_sections(ctx, normalized, show_all, verbose, color),
        color,
    )
}

pub fn print_rich(
    ctx: &FileContext,
    normalized: &NormalizedFileContext,
    show_all: bool,
    verbose: bool,
    color: bool,
) {
    print!(
        "{}",
        render_panel(ctx, normalized, show_all, verbose, color)
    );
}

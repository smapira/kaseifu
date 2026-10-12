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

fn body_sections(
    ctx: &FileContext,
    normalized: &NormalizedFileContext,
    show_all: bool,
    verbose: bool,
    color: bool,
) -> Vec<Section> {
    // FILE (leading block, no divider).
    let head = vec![
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
    ];

    // SEMANTIC SIGNALS.
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

    // DIRECTORY CONTEXT.
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
    let shown = if show_all {
        total
    } else {
        total.min(NEARBY_LIMIT)
    };
    dir.push(if show_all || total <= NEARBY_LIMIT {
        format!("Nearby entries ({})", total)
    } else {
        format!("Nearby entries ({} of {})", shown, total)
    });
    for entry in ctx.directory_context.entries.iter().take(shown) {
        let name = if verbose {
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
        };
        dir.push(format!("  {}", name));
    }
    if !show_all && total > shown {
        dir.push(format!("Showing {} of {} (use --all)", shown, total));
    }

    let mut sections = vec![
        Section::plain(head),
        Section::headed("SEMANTIC SIGNALS", signals),
        Section::headed("DIRECTORY CONTEXT", dir),
    ];

    // DETAILS: third layer, verbose only (JSON always carries everything).
    if verbose {
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
            let value = item.value.to_string();
            let short = if show_all || value.len() <= 160 {
                value
            } else {
                format!("{}...", &value[..160])
            };
            let source = format!("{:?}", item.source);
            details.push(format!(
                "  [{}] {} = {}",
                source,
                item.key,
                theme::paint(&short, theme::ANSI_DIM, color)
            ));
        }
        if !show_all && evidence_total > evidence_shown {
            details.push(format!(
                "Showing {} of {} evidence entries (use --all)",
                evidence_shown, evidence_total
            ));
        }
        sections.push(Section::headed("DETAILS", details));
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

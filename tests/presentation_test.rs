use kaseifu::models::candidate::CandidateDestination;
use kaseifu::presentation::components::path::{
    paint_path, relative_display, strip_ansi, visible_width,
};
use kaseifu::presentation::components::score_bar::{relative_percent, render_bar};
use kaseifu::presentation::format::{OutputFormat, resolve_mode_for_test};
use kaseifu::presentation::renderers::discover::{render_panel, render_text};
use std::path::PathBuf;

fn cand(id: &str, score: f64) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: PathBuf::from(format!("/data/{}", id.to_lowercase())),
        score,
        evidence_count: 1,
        reasons: vec!["exact normalized filename".to_string()],
    }
}

#[test]
fn bar_relative_math() {
    assert_eq!(relative_percent(1.0, 1.0), Some(100.0));
    assert_eq!(relative_percent(0.9, 1.0), Some(90.0));
    let bar = render_bar(0.9, 1.0, 30, false);
    assert!(bar.contains("90.0%"));
    assert_eq!(bar.chars().filter(|c| *c == '\u{2588}').count(), 27);
}

#[test]
fn bar_edge_cases_na() {
    assert_eq!(render_bar(1.0, 0.0, 30, false), "N/A");
    assert_eq!(render_bar(f64::NAN, 1.0, 30, false), "N/A");
    assert_eq!(render_bar(f64::INFINITY, 1.0, 30, false), "N/A");
    assert_eq!(render_bar(-0.5, 1.0, 30, false), "N/A");
}

#[test]
fn bar_narrow_terminal() {
    let full = render_bar(1.0, 1.0, 30, false);
    let narrow = render_bar(1.0, 1.0, 10, false);
    assert!(narrow.len() < full.len());
}

#[test]
fn mode_matrix() {
    let m = resolve_mode_for_test(OutputFormat::Auto, true, false, false);
    assert!(m.rich && m.color);
    let m = resolve_mode_for_test(OutputFormat::Auto, false, false, false);
    assert!(!m.rich && !m.color);
    let m = resolve_mode_for_test(OutputFormat::Json, true, false, false);
    assert!(!m.rich);
    let m = resolve_mode_for_test(OutputFormat::Rich, true, true, false);
    assert!(m.rich && !m.color);
    let m = resolve_mode_for_test(OutputFormat::Rich, true, false, true);
    assert!(m.rich && !m.color);
}

#[test]
fn discover_top5_all_verbose() {
    let c: Vec<CandidateDestination> = (1..=7)
        .map(|i| cand(&format!("D{}", i), 1.0 - i as f64 * 0.05))
        .collect();
    let top = render_text("target.pdf", &c, false, false, false);
    assert!(top.contains("Showing 5 of 7"));
    assert!(!top.contains("D6"));
    let all = render_text("target.pdf", &c, true, false, false);
    assert!(all.contains("D6") && all.contains("D7"));
    let verbose = render_text("target.pdf", &c, true, true, false);
    assert!(verbose.contains("evidence="));
    assert!(verbose.contains("relative score"));
}

#[test]
fn discover_empty() {
    let out = render_text("target.pdf", &[], false, false, false);
    assert!(out.contains("No candidates"));
}

#[test]
fn path_relative_to_home() {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return;
    }
    let got = relative_display(&format!("{}/Desktop/work", home));
    assert_eq!(got, "~/Desktop/work");
}

#[test]
fn path_relative_to_cwd() {
    let cwd = std::env::current_dir().unwrap();
    let got = relative_display(&cwd.join("some/dir").to_string_lossy());
    assert_eq!(got, "./some/dir");
}

#[test]
fn path_truncation_keeps_last_segment() {
    let long = "/data/a-very-long-prefix-segment/middle/State Of Mind Labs";
    let got = paint_path(long, 30, false);
    assert!(
        got.ends_with("State Of Mind Labs"),
        "last segment must survive: {}",
        got
    );
    assert!(got.starts_with("\u{2026}") || got.contains("\u{2026}"));
}

#[test]
fn path_last_segment_emphasized_with_color() {
    let colored = paint_path("/data/sinks/State Of Mind Labs", 64, true);
    assert!(colored.contains("\x1b[1mState Of Mind Labs\x1b[0m"));
    assert_eq!(strip_ansi(&colored), "/data/sinks/State Of Mind Labs");
    let plain = paint_path("/data/sinks/State Of Mind Labs", 64, false);
    assert!(!plain.contains('\x1b'));
}

#[test]
fn visible_width_ignores_ansi() {
    assert_eq!(visible_width("a\x1b[1mb\x1b[0m"), 2);
    assert_eq!(visible_width("\u{2588}\u{2588}"), 2);
}

#[test]
fn panel_lines_share_visible_width() {
    let c: Vec<CandidateDestination> = (1..=3)
        .map(|i| cand(&format!("D{}", i), 1.0 - i as f64 * 0.1))
        .collect();
    for color in [false, true] {
        let out = render_panel("target.pdf", &c, false, false, color);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() > 3);
        assert!(strip_ansi(lines[0]).starts_with("\u{256d}"));
        assert!(strip_ansi(lines[lines.len() - 1]).starts_with("\u{2570}"));
        let first = visible_width(lines[0]);
        for l in &lines {
            assert_eq!(visible_width(l), first, "misaligned: {}", l);
        }
        assert!(out.contains("KASEIFU / DISCOVER"));
    }
}

#[test]
fn panel_empty_is_aligned() {
    let out = render_panel("target.pdf", &[], false, false, false);
    assert!(out.contains("No candidates"));
    let widths: Vec<usize> = out.lines().map(visible_width).collect();
    assert!(widths.iter().all(|w| *w == widths[0]));
}

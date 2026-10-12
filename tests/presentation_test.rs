use kaseifu::models::candidate::CandidateDestination;
use kaseifu::presentation::components::score_bar::{relative_percent, render_bar};
use kaseifu::presentation::format::{OutputFormat, resolve_mode_for_test};
use kaseifu::presentation::renderers::discover::render_text;
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

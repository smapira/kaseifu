//! v3.1 Classify/Inspect Rich output tests.

use kaseifu::classifier::pipeline::ClassifyReport;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::decision::JevDecision;
use kaseifu::models::file_context::{
    DirectoryContext, DirectoryEntry, EntryKind, FileContext, NormalizedFileContext, TargetFile,
};
use kaseifu::policy;
use kaseifu::presentation::components::display::{format_bytes, format_datetime};
use kaseifu::presentation::components::panel::{Section, frame_with_sections};
use kaseifu::presentation::components::path::{strip_ansi, visible_width};
use kaseifu::presentation::components::score_bar::{percent_label, render_probability_bar};
use kaseifu::presentation::renderers::classify::{self, rank_probabilities};
use kaseifu::presentation::renderers::inspect;
use std::path::PathBuf;

fn cand(id: &str) -> CandidateDestination {
    CandidateDestination {
        id: id.to_string(),
        path: PathBuf::from(format!("/data/sinks/{}", id.to_lowercase())),
        score: 1.0,
        evidence_count: 2,
        reasons: vec!["exact normalized filename".to_string()],
    }
}

fn report(probs: &[(&str, f64)], selected: &str, n_cands: usize) -> ClassifyReport {
    let probabilities: Vec<(String, f64)> = probs
        .iter()
        .map(|(id, p)| ((*id).to_string(), *p))
        .collect();
    let decision = JevDecision::from_selection(selected, &probabilities);
    let candidates: Vec<CandidateDestination> =
        (1..=n_cands).map(|i| cand(&format!("D{}", i))).collect();
    ClassifyReport {
        decision,
        probabilities,
        candidates,
        target_filename: "THCO-COA-Non-Detect Updated.pdf".to_string(),
        target_extension: Some("pdf".to_string()),
        target_file_type: None,
        target_size_bytes: 126597,
        semantic_signals: vec!["K:PDF".to_string(), "F:THCO".to_string()],
        keywords: Default::default(),
    }
}

fn ctx_with_entries(n: usize) -> (FileContext, NormalizedFileContext) {
    let entries = (0..n)
        .map(|i| DirectoryEntry {
            name: if i % 3 == 2 {
                format!("dir{}", i)
            } else {
                format!("file{}.txt", i)
            },
            kind: if i % 3 == 2 {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
        })
        .collect();
    let ctx = FileContext {
        schema_version: "file-context-v1".to_string(),
        target: TargetFile {
            path: PathBuf::from("/Users/test/Desktop/report.pdf"),
            filename: "report.pdf".to_string(),
            extension: Some("pdf".to_string()),
            size_bytes: 126597,
            modified_at: Some("2022-06-07T08:56:00+00:00".to_string()),
        },
        directory_context: DirectoryContext {
            current: PathBuf::from("/Users/test/Desktop"),
            parent: Some(PathBuf::from("/Users/test")),
            entries,
        },
        evidence: vec![],
    };
    let normalized = NormalizedFileContext {
        target: ctx.target.clone(),
        semantic_signals: vec!["K:PDF".to_string()],
        size_bytes: 126597,
        ..Default::default()
    };
    (ctx, normalized)
}

// --- shared parts ---

#[test]
fn probability_bar_uses_raw_value() {
    let bar = render_probability_bar(0.29, 30, false);
    assert!(bar.contains("29.0%"), "got: {}", bar);
    assert_eq!(bar.chars().filter(|c| *c == '#').count(), 9);
    // values are never renormalized: 0.29 of a sub-1.0 total stays 29%
    assert_eq!(percent_label(0.29), "29%");
    assert_eq!(render_probability_bar(f64::NAN, 30, false), "N/A");
    assert_eq!(render_probability_bar(1.5, 30, false), "N/A");
}

#[test]
fn bytes_and_datetime() {
    assert_eq!(format_bytes(126597), "123.6 KiB");
    assert_eq!(
        format_datetime(Some("2022-06-07T08:56:00+00:00")),
        "2022-06-07 08:56"
    );
    assert_eq!(format_datetime(Some("garbage")), "garbage");
}

#[test]
fn sections_share_visible_width() {
    for color in [false, true] {
        let out = frame_with_sections(
            "KASEIFU / CLASSIFY",
            &[
                Section::plain(vec!["TARGET".to_string(), "name.pdf".to_string()]),
                Section::headed("JEV RANKING", vec!["1  D1  x  29%".to_string()]),
                Section::headed("POLICY", vec!["LEAVE".to_string()]),
            ],
            color,
        );
        let plain = strip_ansi(&out);
        assert!(plain.contains("|-- JEV RANKING"), "got:\n{}", plain);
        let widths: Vec<usize> = out.lines().map(visible_width).collect();
        assert!(
            widths.iter().all(|w| *w == widths[0]),
            "misaligned:\n{}",
            plain
        );
        assert!(widths[0] <= 80, "too wide: {}", widths[0]);
    }
}

// --- classify ---

#[test]
fn ranking_is_descending_and_top5() {
    let probs = vec![
        ("D2".to_string(), 0.23),
        ("D1".to_string(), 0.29),
        ("D3".to_string(), 0.18),
    ];
    let ranked = rank_probabilities(&probs);
    assert_eq!(ranked[0].0, "D1");
    assert_eq!(ranked[1].0, "D2");
    assert_eq!(ranked[2].0, "D3");

    let r = report(
        &[
            ("D1", 0.29),
            ("D2", 0.23),
            ("D3", 0.18),
            ("D4", 0.16),
            ("D5", 0.06),
            ("D6", 0.04),
            ("D7", 0.02),
        ],
        "D1",
        7,
    );
    let policy = policy::decision::evaluate(&r.decision);
    let out = classify::render_panel(&r, &policy, false, false, false);
    assert!(out.contains("Showing 5 of 7 destinations"), "got:\n{}", out);
    assert!(!out.contains("D6"), "top-5 must hide D6:\n{}", out);
    let all = classify::render_panel(&r, &policy, true, false, false);
    assert!(all.contains("D6") && all.contains("D7"));
}

#[test]
fn selection_outside_top5_is_shown() {
    let r = report(
        &[
            ("D1", 0.30),
            ("D2", 0.20),
            ("D3", 0.15),
            ("D4", 0.10),
            ("D5", 0.09),
            ("D6", 0.08),
        ],
        "D6",
        6,
    );
    let policy = policy::decision::evaluate(&r.decision);
    let out = classify::render_panel(&r, &policy, false, false, false);
    assert!(out.contains("ranked #6"), "got:\n{}", out);
    assert!(out.contains("D6"), "selected D6 must appear:\n{}", out);
}

#[test]
fn none_row_and_confidence_margin() {
    let r = report(&[("D1", 0.29), ("D2", 0.23), ("NONE", 0.12)], "D1", 2);
    let policy = policy::decision::evaluate(&r.decision);
    let out = classify::render_panel(&r, &policy, true, false, false);
    assert!(out.contains("NONE"), "got:\n{}", out);
    assert!(out.contains("Confidence   0.29"), "got:\n{}", out);
    assert!(out.contains("Margin       0.06"), "got:\n{}", out);
}

#[test]
fn policy_is_real_verdict_and_no_move_to() {
    // 0.29 confidence is below every threshold -> LEAVE, never a move.
    let r = report(&[("D1", 0.29), ("D2", 0.23)], "D1", 2);
    let policy = policy::decision::evaluate(&r.decision);
    let out = classify::render_panel(&r, &policy, false, false, false);
    assert!(out.contains("LEAVE"), "got:\n{}", out);
    assert!(
        !out.contains("MoveTo") && !out.contains("move_to"),
        "got:\n{}",
        out
    );

    // high confidence + clear margin -> AUTO MOVE is the real verdict.
    let r2 = report(&[("D1", 0.95), ("D2", 0.03)], "D1", 2);
    let policy2 = policy::decision::evaluate(&r2.decision);
    let out2 = classify::render_panel(&r2, &policy2, false, false, false);
    assert!(out2.contains("AUTO MOVE"), "got:\n{}", out2);
}

#[test]
fn panels_are_ascii_only() {
    // Same rule as discover: no box-drawing, blocks, or wide punctuation
    // in emitted chrome, so CJK-font terminals cannot misalign borders.
    let r = report(&[("D1", 0.29), ("D2", 0.23), ("NONE", 0.12)], "D1", 2);
    let policy = policy::decision::evaluate(&r.decision);
    let (ctx, normalized) = ctx_with_entries(7);
    for out in [
        classify::render_panel(&r, &policy, true, true, true),
        inspect::render_panel(&ctx, &normalized, true, true, true),
    ] {
        let plain = strip_ansi(&out);
        assert!(plain.is_ascii(), "non-ASCII chrome:\n{}", plain);
    }
}

#[test]
fn ranked_footer_mentions_none() {
    let r = report(&[("D1", 0.29), ("D2", 0.23), ("NONE", 0.12)], "D1", 2);
    let policy = policy::decision::evaluate(&r.decision);
    // more ranked entries than shown, NONE among them
    let r_many = report(
        &[
            ("D1", 0.29),
            ("D2", 0.23),
            ("D3", 0.10),
            ("D4", 0.09),
            ("D5", 0.08),
            ("D6", 0.07),
            ("NONE", 0.05),
        ],
        "D1",
        6,
    );
    let out = classify::render_panel(&r_many, &policy, false, false, false);
    assert!(out.contains("(incl. NONE)"), "got:\n{}", out);
}

// --- inspect ---

#[test]
fn inspect_layers_and_limits() {
    let (ctx, normalized) = ctx_with_entries(7);
    let out = inspect::render_panel(&ctx, &normalized, false, false, false);
    // L1 first: filename before anything else meaningful.
    let name_pos = out.find("report.pdf").expect("filename missing");
    let entries_pos = out.find("Nearby entries").expect("entries missing");
    assert!(name_pos < entries_pos);
    assert!(out.contains("123.6 KiB"), "got:\n{}", out);
    assert!(out.contains("2022-06-07 08:56"), "got:\n{}", out);
    assert!(out.contains("K  PDF"), "signals missing:\n{}", out);
    assert!(out.contains("Nearby entries (5 of 7)"), "got:\n{}", out);
    assert!(!out.contains("DETAILS"), "verbose layer must stay hidden");
    // directories carry a suffix, files do not.
    assert!(out.contains("dir2/"), "got:\n{}", out);

    let all = inspect::render_panel(&ctx, &normalized, true, false, false);
    assert!(all.contains("Nearby entries (7)"));

    let verbose = inspect::render_panel(&ctx, &normalized, false, true, false);
    assert!(verbose.contains("DETAILS"), "got:\n{}", verbose);
    assert!(verbose.contains("file-context-v1"), "got:\n{}", verbose);
}

#[test]
fn inspect_missing_fields_do_not_crash() {
    let (mut ctx, mut normalized) = ctx_with_entries(1);
    ctx.target.modified_at = None;
    ctx.directory_context.parent = None;
    normalized.semantic_signals.clear();
    normalized.file_type = None;
    ctx.target.extension = None;
    let out = inspect::render_panel(&ctx, &normalized, false, false, false);
    assert!(out.contains("report.pdf"));
    assert!(
        out.contains("Type      -") || out.contains("| - "),
        "missing marker expected:\n{}",
        out
    );
}

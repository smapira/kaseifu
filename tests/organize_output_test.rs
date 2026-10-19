//! Organize output tests (display only).
//!
//! The renderer is a pure view over `OrganizeReport`: these tests fix the
//! section order (mode -> decision -> policy -> result), the destination
//! path display, the mode wording, the human-readable plan errors, and the
//! ASCII/plain rules. No Jev call and no filesystem access.

use kaseifu::models::decision::{DecisionAction, JevDecision};
use kaseifu::operations::organize::{OrganizeOutcome, OrganizeReport};
use kaseifu::operations::plan::PlanError;
use kaseifu::policy;
use kaseifu::presentation::components::path::{strip_ansi, visible_width};
use kaseifu::presentation::renderers::generic;
use kaseifu::presentation::renderers::organize as organize_view;
use std::path::{Path, PathBuf};

/// Fixture paths outside cwd/$HOME, so `relative_display` returns them
/// unchanged and the expectations are environment-independent.
const SINK_DIR: &str = "/data/sinks/State Of Mind Labs";
const SOURCE: &str = "/data/inbox/THCO-COA-Non-Detect Updated.pdf";

fn decision(destination: Option<&str>, confidence: f64, margin: f64) -> JevDecision {
    JevDecision {
        destination_id: destination.map(str::to_string),
        confidence,
        margin,
        ..JevDecision::default()
    }
}

fn report(
    decision: JevDecision,
    candidate_path: Option<&str>,
    outcome: OrganizeOutcome,
) -> OrganizeReport {
    let policy = policy::decision::evaluate(&decision);
    OrganizeReport {
        decision,
        policy,
        candidate_path: candidate_path.map(PathBuf::from),
        target_indexed: None,
        outcome,
        rule: None,
    }
}

/// The real observed case: Jev selected D1, the policy left the file alone.
fn leave_report() -> OrganizeReport {
    report(
        decision(Some("D1"), 0.36, 0.13),
        Some(SINK_DIR),
        OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
    )
}

fn panel(report: &OrganizeReport, execute_requested: bool, color: bool) -> String {
    organize_view::render_panel(report, Path::new(SOURCE), execute_requested, color)
}

fn text(report: &OrganizeReport, execute_requested: bool) -> String {
    organize_view::render_text(report, Path::new(SOURCE), execute_requested)
}

// ---------------------------------------------------------------------------
// Decision: the selected candidate is resolved to its directory
// ---------------------------------------------------------------------------

#[test]
fn decision_shows_the_selected_directory_before_policy_and_result() {
    let plain = strip_ansi(&panel(&leave_report(), false, false));

    let mode = plain
        .find("dry run (no files moved)")
        .expect("mode line missing");
    let decision = plain.find("|-- DECISION").expect("decision missing");
    let selected = plain
        .find("Selected     D1 - /data/sinks/State Of Mind Labs")
        .expect("selected row with the directory missing");
    let policy = plain.find("|-- POLICY").expect("policy missing");
    let result = plain.find("|-- NO MOVE").expect("result missing");

    // The mode line never splits the decision information from the verdict.
    assert!(
        mode < decision && decision < selected && selected < policy && policy < result,
        "wrong order:\n{plain}"
    );
    assert!(plain.contains("[LEAVE] LEAVE"), "got:\n{plain}");
    assert!(
        plain.contains("confidence below review threshold"),
        "got:\n{plain}"
    );
    assert!(
        plain.contains("policy refused: automatic move not allowed"),
        "got:\n{plain}"
    );
    for forbidden in ["NotAutoMove", "Some(\"D1\")", "Leave {", "executed"] {
        assert!(!plain.contains(forbidden), "{forbidden} leaked:\n{plain}");
    }
}

#[test]
fn none_selection_shows_no_destination_without_a_path() {
    for (selected, expected) in [
        (Some("NONE"), "Selected     NONE - no destination"),
        (None, "Selected     no destination"),
    ] {
        let report = report(
            decision(selected, 0.36, 0.13),
            Some(SINK_DIR),
            OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
        );
        let out = text(&report, false);
        assert!(out.contains(expected), "got:\n{out}");
        assert!(!out.contains(SINK_DIR), "no path for NONE:\n{out}");
    }
}

#[test]
fn unresolved_selection_is_labelled_and_never_invents_a_path() {
    let report = report(
        decision(Some("D99"), 0.36, 0.13),
        None,
        OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
    );
    let out = strip_ansi(&panel(&report, false, false));
    assert!(
        out.contains("Selected     D99 - unresolved candidate"),
        "got:\n{out}"
    );
}

#[test]
fn rich_emphasizes_the_last_path_segment_and_plain_keeps_the_full_path() {
    let report = leave_report();

    let colored = panel(&report, false, true);
    assert!(
        colored.contains("\x1b[1mState Of Mind Labs\x1b[0m"),
        "last segment must be emphasized:\n{colored}"
    );

    let plain = text(&report, false);
    assert!(plain.contains(SINK_DIR), "got:\n{plain}");
}

#[test]
fn rich_truncates_long_paths_while_plain_keeps_them_full() {
    let prev = std::env::var("COLUMNS").ok();
    // 80 columns is the non-TTY default; pinning it keeps the expectation
    // deterministic without altering the other tests' budget.
    unsafe { std::env::set_var("COLUMNS", "80") };

    let long = "/data/a-very-long-prefix-segment/middle-layers/deeper/State Of Mind Labs";
    let report = report(
        decision(Some("D1"), 0.36, 0.13),
        Some(long),
        OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
    );
    let rich = strip_ansi(&panel(&report, false, false));
    let plain = text(&report, false);

    match prev {
        Some(v) => unsafe { std::env::set_var("COLUMNS", v) },
        None => unsafe { std::env::remove_var("COLUMNS") },
    }

    assert!(plain.contains(long), "plain keeps the path:\n{plain}");
    assert!(
        !rich.contains(long) && rich.contains("State Of Mind Labs"),
        "rich shortens head-first but keeps the tail:\n{rich}"
    );
}

#[test]
fn home_relative_paths_are_shown_with_a_tilde() {
    let Some(home) = std::env::var("HOME").ok().filter(|h| !h.is_empty()) else {
        return;
    };
    let directory = PathBuf::from(&home).join("Documents/Sinks");
    let report = report(
        decision(Some("D1"), 0.36, 0.13),
        Some(&directory.to_string_lossy()),
        OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
    );
    assert!(
        text(&report, false).contains("~/Documents/Sinks"),
        "got:\n{}",
        text(&report, false)
    );
}

// ---------------------------------------------------------------------------
// Mode: the banner describes the mode, never a completed move
// ---------------------------------------------------------------------------

#[test]
fn execute_mode_is_a_mode_not_a_moved_claim() {
    let plain = strip_ansi(&panel(&leave_report(), true, false));
    assert!(plain.contains("execute mode"), "got:\n{plain}");
    assert!(!plain.contains("executed"), "got:\n{plain}");
    assert!(
        !plain.contains("MOVED"),
        "mode must not claim a move:\n{plain}"
    );
    assert!(
        plain.contains("policy refused: automatic move not allowed"),
        "the result still states what happened:\n{plain}"
    );
}

#[test]
fn dry_run_banner_wording_is_shared_with_watch() {
    assert_eq!(
        generic::dry_run_banner(false, false),
        "dry run (no files moved)"
    );
    assert_eq!(generic::dry_run_banner(true, false), "execute mode");
    assert_eq!(
        strip_ansi(&generic::dry_run_banner(true, true)),
        "execute mode"
    );

    let watch = strip_ansi(&generic::watch_line(
        "created",
        "/data/inbox/x.pdf",
        true,
        false,
    ));
    assert!(
        watch.contains("dry run (no files moved)"),
        "watch must use the same wording: {watch}"
    );
}

// ---------------------------------------------------------------------------
// Result: human sentences for every outcome
// ---------------------------------------------------------------------------

#[test]
fn moved_reports_destination_and_operation_id() {
    let destination = format!("{SINK_DIR}/report.pdf");
    let report = report(
        decision(Some("D1"), 0.90, 0.50),
        Some(SINK_DIR),
        OrganizeOutcome::Moved {
            operation_id: "op-123".to_string(),
            destination: PathBuf::from(&destination),
        },
    );

    let rich = strip_ansi(&panel(&report, true, false));
    assert!(rich.contains("|-- MOVED"), "got:\n{rich}");
    assert!(rich.contains(&destination), "got:\n{rich}");
    assert!(rich.contains("op-123"), "got:\n{rich}");

    let plain = text(&report, true);
    assert!(plain.contains("== Moved =="), "got:\n{plain}");
    assert!(plain.contains(&format!("Destination  {destination}")));
    assert!(plain.contains("Operation    op-123"));
    // The policy verdict (AUTO MOVE) is separate from the result.
    assert!(plain.contains("AUTO MOVE"), "got:\n{plain}");
}

#[test]
fn would_move_shows_source_and_destination_paths() {
    let destination = format!("{SINK_DIR}/THCO-COA-Non-Detect Updated.pdf");
    let report = report(
        decision(Some("D1"), 0.90, 0.50),
        Some(SINK_DIR),
        OrganizeOutcome::WouldMove {
            destination: PathBuf::from(&destination),
        },
    );
    let plain = text(&report, false);
    assert!(plain.contains("== Would move =="), "got:\n{plain}");
    assert!(
        plain.contains(&format!("From         {SOURCE}")),
        "got:\n{plain}"
    );
    assert!(
        plain.contains(&format!("Destination  {destination}")),
        "got:\n{plain}"
    );
    assert!(!plain.contains("executed"), "got:\n{plain}");
}

#[test]
fn refused_and_unplanable_share_the_policy_refused_wording() {
    let refused = report(
        decision(Some("D1"), 0.70, 0.10),
        Some(SINK_DIR),
        OrganizeOutcome::Refused,
    );
    let out = text(&refused, false);
    assert!(out.contains("== No move =="), "got:\n{out}");
    assert!(out.contains("  policy refused\n"), "got:\n{out}");

    for (confidence, margin) in [(0.70, 0.10), (0.20, 0.05)] {
        let report = report(
            decision(Some("D1"), confidence, margin),
            Some(SINK_DIR),
            OrganizeOutcome::Unplanable(PlanError::NotAutoMove),
        );
        let out = text(&report, false);
        assert!(
            out.contains("policy refused: automatic move not allowed"),
            "got:\n{out}"
        );
    }
}

#[test]
fn collision_keeps_the_review_result() {
    let existing = format!("{SINK_DIR}/report.pdf");
    let report = report(
        decision(Some("D1"), 0.90, 0.50),
        Some(SINK_DIR),
        OrganizeOutcome::Unplanable(PlanError::Collision(PathBuf::from(&existing))),
    );
    let plain = text(&report, false);
    assert!(plain.contains("== REVIEW =="), "got:\n{plain}");
    assert!(
        plain.contains(&format!("destination already exists: {existing}")),
        "got:\n{plain}"
    );

    let rich = strip_ansi(&panel(&report, false, false));
    assert!(rich.contains("|-- REVIEW"), "got:\n{rich}");
}

#[test]
fn plan_errors_are_human_readable_sentences() {
    let cases: [(PlanError, &str); 6] = [
        (
            PlanError::NotAutoMove,
            "policy refused: automatic move not allowed",
        ),
        (
            PlanError::UnknownCandidate("D99".to_string()),
            "unknown candidate selected by the decision: D99",
        ),
        (
            PlanError::SourceMissing(PathBuf::from("/data/inbox/missing.pdf")),
            "source file is missing: /data/inbox/missing.pdf",
        ),
        (
            PlanError::SourceNotAFile(PathBuf::from("/data/inbox")),
            "source is not a regular file: /data/inbox",
        ),
        (
            PlanError::DestinationNotADirectory(PathBuf::from("/data/sinks/gone")),
            "destination is not a directory: /data/sinks/gone",
        ),
        (
            PlanError::Collision(PathBuf::from("/data/sinks/coa.pdf")),
            "destination already exists: /data/sinks/coa.pdf",
        ),
    ];

    for (error, expected) in cases {
        let report = report(
            decision(Some("D1"), 0.36, 0.13),
            Some(SINK_DIR),
            OrganizeOutcome::Unplanable(error),
        );
        let out = text(&report, false);
        assert!(out.contains(expected), "missing {expected}:\n{out}");
        for name in [
            "NotAutoMove",
            "UnknownCandidate",
            "SourceMissing",
            "SourceNotAFile",
            "DestinationNotADirectory",
            "Collision",
        ] {
            assert!(!out.contains(name), "{name} leaked:\n{out}");
        }
    }
}

#[test]
fn move_failure_keeps_the_reported_detail() {
    let report = report(
        decision(Some("D1"), 0.90, 0.50),
        Some(SINK_DIR),
        OrganizeOutcome::MoveFailed("permission denied".to_string()),
    );
    let plain = text(&report, true);
    assert!(plain.contains("== Move failed =="), "got:\n{plain}");
    assert!(plain.contains("permission denied"), "got:\n{plain}");
}

// ---------------------------------------------------------------------------
// Layout: plain text and panel rules
// ---------------------------------------------------------------------------

#[test]
fn plain_text_is_ordered_ascii_and_ansi_free() {
    let out = text(&leave_report(), false);
    let mut cursor = 0;
    for heading in [
        "== Mode ==",
        "== Decision ==",
        "== Policy ==",
        "== No move ==",
    ] {
        let position = out.find(heading).unwrap_or_else(|| {
            panic!("missing {heading}:\n{out}");
        });
        assert!(position >= cursor, "{heading} out of order:\n{out}");
        cursor = position;
    }
    assert!(
        !out.contains('\x1b'),
        "plain output carries no ANSI:\n{out}"
    );
    assert!(out.is_ascii(), "plain output is ASCII:\n{out}");
    assert!(
        out.ends_with("  policy refused: automatic move not allowed\n"),
        "got:\n{out}"
    );
}

#[test]
fn panels_are_ascii_only_and_share_one_visible_width() {
    for execute_requested in [false, true] {
        for color in [false, true] {
            let out = panel(&leave_report(), execute_requested, color);
            let plain = strip_ansi(&out);
            assert!(plain.is_ascii(), "non-ASCII chrome:\n{plain}");
            let widths: Vec<usize> = out.lines().map(visible_width).collect();
            assert!(
                widths.iter().all(|w| *w == widths[0]),
                "misaligned:\n{plain}"
            );
        }
    }
}

#[test]
fn policy_section_reuses_the_classify_wording() {
    let report = report(
        decision(Some("D1"), 0.90, 0.50),
        Some(SINK_DIR),
        OrganizeOutcome::WouldMove {
            destination: PathBuf::from(format!("{SINK_DIR}/report.pdf")),
        },
    );
    assert!(matches!(report.policy, DecisionAction::AutoMove(_)));
    let out = strip_ansi(&panel(&report, false, false));
    assert!(out.contains("[AUTO MOVE] AUTO MOVE"), "got:\n{out}");
    assert!(
        out.contains("meets auto-move thresholds (confidence >= 0.85, margin >= 0.40)"),
        "got:\n{out}"
    );
}

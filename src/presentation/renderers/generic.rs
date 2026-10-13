use crate::models::decision::DecisionAction;
use crate::policy::decision::{AUTO_MOVE_CONFIDENCE, AUTO_MOVE_MARGIN, REVIEW_CONFIDENCE};
use crate::presentation::theme;

pub fn header(title: &str, color: bool) -> String {
    theme::paint(title, theme::ANSI_CYAN, color)
}

pub fn status_line(label: &str, value: &str, color: bool) -> String {
    format!(
        "{}: {}",
        theme::paint(label, theme::ANSI_TEAL, color),
        value
    )
}

pub fn decision_line(kind: &str, color: bool) -> String {
    let (ansi, tag) = match kind {
        "AUTO MOVE" => (theme::ANSI_MINT, "[AUTO MOVE]"),
        "REVIEW" => (theme::ANSI_AMBER, "[REVIEW]"),
        "LEAVE" | "NONE" => (theme::ANSI_TEAL, "[LEAVE]"),
        s if s.starts_with("ERROR") || s == "FAILED" => (theme::ANSI_CORAL, "[ERROR]"),
        _ => (theme::ANSI_VIOLET, "[INFO]"),
    };
    format!("{} {}", theme::paint(tag, ansi, color), kind)
}

/// Human wording for the mode a command was started in.
///
/// `executed` is the *requested mode* flag (e.g. the CLI `--execute`),
/// never a claim that a file moved: the words describe the mode only.
/// Commands state what actually happened in their own result rows.
pub fn dry_run_banner(executed: bool, color: bool) -> String {
    if executed {
        theme::paint("execute mode", theme::ANSI_CORAL, color)
    } else {
        theme::paint("dry run (no files moved)", theme::ANSI_AMBER, color)
    }
}

/// Human-readable policy verdict shared by the classify and organize views.
///
/// Returns `(kind, reason)`: `kind` feeds [`decision_line`], `reason` is a
/// sentence. `policy::decision::evaluate` only yields AutoMove/Review/Leave;
/// anything else is not an execution verdict, so it must read as LEAVE,
/// never as a move.
pub fn policy_view(policy: &DecisionAction) -> (&'static str, String) {
    match policy {
        DecisionAction::AutoMove(_) => (
            "AUTO MOVE",
            format!(
                "meets auto-move thresholds (confidence >= {:.2}, margin >= {:.2})",
                AUTO_MOVE_CONFIDENCE, AUTO_MOVE_MARGIN
            ),
        ),
        DecisionAction::Review(_) => (
            "REVIEW",
            format!(
                "below auto-move thresholds; human review required (review confidence >= {:.2})",
                REVIEW_CONFIDENCE
            ),
        ),
        DecisionAction::Leave { what } => ("LEAVE", what.clone()),
        // `evaluate` only yields the three variants above; anything else is
        // not an execution verdict, so it must read as LEAVE, never as a move.
        _ => ("LEAVE", "policy did not authorize execution".to_string()),
    }
}

pub fn watch_line(event: &str, path: &str, dry_run: bool, color: bool) -> String {
    format!(
        "{} {} {}",
        theme::paint("watch", theme::ANSI_TEAL, color),
        path,
        if dry_run {
            format!("{} ({})", event, dry_run_banner(false, color))
        } else {
            event.to_string()
        }
    )
}

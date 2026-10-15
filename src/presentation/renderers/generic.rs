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

/// The `watch` mode line shared by the start banner and every event row.
///
/// The mode is always stated with the same vocabulary as the other commands
/// (`dry run (no files moved)` / `execute mode`, ZC-9): the plain output
/// never shows a raw Rust boolean or a backwards `dry-run` tag.
pub fn watch_line(event: &str, path: &str, dry_run: bool, color: bool) -> String {
    format!(
        "{} {} {} ({})",
        theme::paint("watch", theme::ANSI_TEAL, color),
        path,
        event,
        dry_run_banner(!dry_run, color)
    )
}

/// The `watch` start line as one JSON object (`--format json` is JSON Lines:
/// every line the watcher prints is exactly one newline-terminated object,
/// never pretty-printed, because the stream has no end to wrap in an array).
///
/// The keys mirror [`watch_line`]: the same roots and the same mode words
/// (`dry-run` / `execute`).
pub fn watch_start_json(roots: &[String], dry_run: bool) -> String {
    serde_json::json!({
        "watch": {
            "event": "start",
            "roots": roots,
            "mode": if dry_run { "dry-run" } else { "execute" },
        }
    })
    .to_string()
}

/// One `watch` event line as one JSON object (see [`watch_start_json`]).
///
/// `detail` carries the human sentence the text line would show (the
/// suppression reason, the destination of a move); `None` leaves the key out.
pub fn watch_event_json(event: &str, path: &str, detail: Option<&str>) -> String {
    let mut inner = serde_json::Map::new();
    inner.insert(
        "event".to_string(),
        serde_json::Value::String(event.to_string()),
    );
    inner.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    if let Some(detail) = detail {
        inner.insert(
            "detail".to_string(),
            serde_json::Value::String(detail.to_string()),
        );
    }
    serde_json::Value::Object({
        let mut outer = serde_json::Map::new();
        outer.insert("watch".to_string(), serde_json::Value::Object(inner));
        outer
    })
    .to_string()
}

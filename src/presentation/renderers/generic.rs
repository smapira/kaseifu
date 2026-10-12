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

pub fn dry_run_banner(executed: bool, color: bool) -> String {
    if executed {
        theme::paint("executed", theme::ANSI_CORAL, color)
    } else {
        theme::paint("dry-run (no files moved)", theme::ANSI_AMBER, color)
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

pub fn print_panel(title: &str, body: &str) {
    use rich_rs::{Console, Panel, Text};
    let mut console = Console::new();
    let combined = format!("{}\n{}", title, body);
    let panel = Panel::new(Box::new(Text::plain(&combined))).with_title(title);
    let _ = console.print(&panel, None, None, None, false, "\n");
}

//! v0.8.0 finish-batch tests: ZC-9 (watch wording) and ZC-10 (README quick
//! start).
//!
//! ZC-9 is pinned through the functions the watcher actually uses
//! (`generic::watch_line`, `runner::outcome_text`), because `watch` itself
//! loops forever; the CLI start line is covered by a short-lived real run in
//! the batch's measurement record. ZC-10 reads README.md from the repository
//! root: the quick start must come first, keep the old install / first-run
//! information, and resolve every in-page link.

use std::path::{Path, PathBuf};

use kaseifu::presentation::renderers::generic;
use kaseifu::watch::runner::{ProcessOutcome, outcome_text};

// ---------------------------------------------------------------------------
// ZC-9: watch wording uses the shared mode vocabulary
// ---------------------------------------------------------------------------

#[test]
fn watch_start_line_states_the_mode_with_the_shared_vocabulary() {
    let dry = generic::watch_line("start", "/tmp/inbox", true, false);
    assert_eq!(dry, "watch /tmp/inbox start (dry run (no files moved))");
    let execute = generic::watch_line("start", "/tmp/inbox", false, false);
    assert_eq!(execute, "watch /tmp/inbox start (execute mode)");
    for line in [&dry, &execute] {
        assert!(!line.contains("dry-run"), "{line}");
        assert!(
            !line.contains(": true") && !line.contains(": false"),
            "no raw boolean: {line}"
        );
        assert!(line.is_ascii(), "{line}");
    }
}

#[test]
fn watch_move_log_line_speaks_the_shared_dry_run_vocabulary() {
    let text = outcome_text(
        Path::new("/inbox/note.txt"),
        &ProcessOutcome::WouldMove {
            destination: PathBuf::from("/archive/note.txt"),
        },
    );
    assert_eq!(
        text,
        "would move /inbox/note.txt -> /archive/note.txt (dry run (no files moved))\n"
    );
    assert!(!text.contains("dry-run"), "{text}");
}

#[test]
fn watch_executed_move_log_line_is_unchanged() {
    let text = outcome_text(
        Path::new("/inbox/note.txt"),
        &ProcessOutcome::Moved {
            operation_id: "op-1".to_string(),
            destination: PathBuf::from("/archive/note.txt"),
        },
    );
    assert!(
        text.starts_with("moved   /inbox/note.txt -> /archive/note.txt"),
        "{text}"
    );
    assert!(text.contains("operation: op-1"), "{text}");
}

// ---------------------------------------------------------------------------
// ZC-10: README quick start
// ---------------------------------------------------------------------------

fn read_readme() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md readable")
}

#[test]
fn readme_quick_start_comes_first_and_covers_the_one_command_flow() {
    let readme = read_readme();
    let quick = readme.find("## Quick Start").expect("Quick Start heading");
    let how = readme
        .find("## How it works")
        .expect("How it works heading");
    assert!(
        quick < how,
        "Quick Start must come before the design section"
    );
    assert!(
        quick < readme.len() / 4,
        "the quick start must be near the top of the README"
    );
    let section = &readme[quick..how];
    for needle in [
        "cargo install kaseifu",
        "kaseifu classify ~/Desktop/example.pdf",
        "kaseifu doctor",
        "kaseifu setup",
        "kaseifu organize",
        "KASEIFU_KEY_SOURCE",
        "kaseifu inspect",
        "cargo install --path .",
        "git clone https://github.com/smapira/kaseifu.git",
    ] {
        assert!(section.contains(needle), "quick start lost: {needle}");
    }
}

#[test]
fn readme_keeps_the_detail_sections_and_resolves_its_links() {
    let readme = read_readme();
    for heading in [
        "### Inspect a file",
        "### Classify (dry run)",
        "### Organize (dry run by default)",
        "### Choose the destination yourself",
        "### Undo",
        "### Watch",
        "### Setup",
        "### Auth",
        "### Config",
        "### Doctor",
        "### Uninstall",
        "### Shell completion",
        "## Configuration",
        "### API key source (`KASEIFU_KEY_SOURCE`)",
        "## Safety",
        "## Code quality",
    ] {
        assert!(
            readme.contains(heading),
            "missing README section: {heading}"
        );
    }

    let anchors = heading_anchors(&readme);
    let targets = link_targets(&readme);
    assert!(
        !targets.is_empty(),
        "the quick start links into the details"
    );
    for target in targets {
        assert!(
            anchors.contains(&target),
            "#{target} does not resolve to a README heading"
        );
    }
}

/// Every `](#anchor)` target, in order of appearance.
fn link_targets(readme: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = readme;
    while let Some(index) = rest.find("](#") {
        let after = &rest[index + 3..];
        if let Some(end) = after.find(')') {
            targets.push(after[..end].to_string());
        }
        rest = after;
    }
    targets
}

/// Slugified heading anchors (GitHub rules: lowercase, spaces to `-`, other
/// punctuation dropped, `_` and `-` kept).
fn heading_anchors(readme: &str) -> Vec<String> {
    readme
        .lines()
        .filter_map(|line| line.strip_prefix('#'))
        .map(|line| slugify(line.trim_start_matches('#').trim()))
        .collect()
}

fn slugify(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|character| match character {
            'a'..='z' | '0'..='9' | '-' | '_' => Some(character),
            'A'..='Z' => Some(character.to_ascii_lowercase()),
            ' ' => Some('-'),
            _ => None,
        })
        .collect()
}

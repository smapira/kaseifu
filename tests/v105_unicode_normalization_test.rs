//! v1.0.5 Unicode normalization conflict tests (M3).
//!
//! Owner decision 2026-10-15 (in the issue file): option (2), the
//! `unicode-normalization` crate as a one-issue exception to the
//! no-new-dependencies rule (`Cargo.toml`/`Cargo.lock` change only by this
//! addition). macOS volumes treat NFC and NFD spellings of one name as one
//! file, so `hold_conflicts` folds both (NFC, then lowercase) and the preview
//! holds the pair instead of showing a proposal that later fails at apply.
//!
//! Model-independent: items are built directly and only the grouping rule is
//! exercised. The on-volume preview-vs-apply behavior is recorded in the
//! issue's verification note.

use std::path::PathBuf;

use kaseifu::rename::batch::{BatchItem, BatchOutcome, hold_conflicts};
use kaseifu::rename::service::{RenamePlan, SourceSnapshot};
use kaseifu::rename::validate::FileSnapshot;

fn item(name: &str, proposal: &str, directory: &str) -> BatchItem {
    let source = PathBuf::from(directory).join(name);
    BatchItem::new(
        source.clone(),
        name.to_string(),
        BatchOutcome::Propose(Box::new(RenamePlan {
            source: source.clone(),
            destination: PathBuf::from(directory).join(proposal),
            current_name: name.to_string(),
            proposal: proposal.to_string(),
            rule_label: "P02".to_string(),
            date_source: "modified",
            snapshot: SourceSnapshot::File(FileSnapshot {
                size: 1,
                modified: None,
            }),
            perception: None,
            video: None,
            directory: None,
        })),
    )
}

#[test]
fn v105_nfc_and_nfd_proposals_conflict() {
    // NFC caf\u{e9}.pdf vs NFD cafe\u{301}.pdf: one file on macOS.
    let mut items = vec![
        item("a.txt", "caf\u{e9}.pdf", "/tmp/x"),
        item("b.txt", "cafe\u{301}.pdf", "/tmp/x"),
        item("c.txt", "other.pdf", "/tmp/x"),
    ];
    hold_conflicts(&mut items);
    for index in [0, 1] {
        assert!(
            matches!(items[index].outcome, BatchOutcome::Conflict { .. }),
            "index {index} must be held: {:?}",
            items[index].outcome
        );
    }
    assert!(items[2].plan().is_some(), "an unshared name stays ready");
}

#[test]
fn v105_ascii_behavior_is_unchanged() {
    let mut items = vec![
        item("a.txt", "Report.pdf", "/tmp/x"),
        item("b.txt", "report.PDF", "/tmp/x"),
        item("c.txt", "other.pdf", "/tmp/x"),
    ];
    hold_conflicts(&mut items);
    for index in [0, 1] {
        assert!(
            matches!(items[index].outcome, BatchOutcome::Conflict { .. }),
            "case-only differences still conflict: {:?}",
            items[index].outcome
        );
    }
    assert!(items[2].plan().is_some(), "an unshared name stays ready");
    let mut distinct = vec![
        item("a.txt", "one.pdf", "/tmp/x"),
        item("b.txt", "two.pdf", "/tmp/x"),
    ];
    hold_conflicts(&mut distinct);
    assert!(
        distinct.iter().all(|item| item.plan().is_some()),
        "no new holds"
    );
}

#[test]
fn v105_normalization_does_not_cross_directories() {
    let mut items = vec![
        item("a.txt", "caf\u{e9}.pdf", "/tmp/x"),
        item("b.txt", "cafe\u{301}.pdf", "/tmp/y"),
    ];
    hold_conflicts(&mut items);
    assert!(items.iter().all(|item| item.plan().is_some()));
}

//! v0.13.2 issue #21: a held proposal must name the real cause.
//!
//! The measured case: `kaseifu label ~/Desktop/cbd-common-scale-5y.png` answered
//! `"proposal": null` with the reason `name must not contain a path separator`,
//! which reads as if the file's own name were bad. The character came from the
//! model's title, and the held report hardcoded `perception: null` and
//! `date_source: ""` on top of it.
//!
//! Model-independent only: the analysis and perception layers are fakes, so
//! nothing here needs Apple Intelligence, Vision or the network. Fixtures live
//! under the system temp directory; no real home directory is read or written.

use kaseifu::rename::service::{
    HoldFacts, PerceptionReport, PlanOutcome, RenameOptions, plan_rename,
};
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::contract::SemanticInfo;
use kaseifu::semantic::fake::FakeSemanticProvider;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v116-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("fixture file");
    path
}

/// A PNG header plus IHDR. The fake path never decodes the image, so the bytes
/// only have to look like a file the image path claims.
fn fixture_png() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&64u32.to_be_bytes());
    out.extend_from_slice(&48u32.to_be_bytes());
    out.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

fn info(title: Option<&str>) -> SemanticInfo {
    SemanticInfo {
        title: title.map(str::to_string),
        category: None,
        project: None,
    }
}

fn pattern_options(pattern: &str) -> RenameOptions {
    RenameOptions {
        pattern: Some(pattern.to_string()),
        ..RenameOptions::default()
    }
}

/// The validator's wording the issue measured: no hold about a title may use it.
const SEPARATOR_WORDING: &str = "name must not contain a path separator";

fn ready(outcome: PlanOutcome) -> Box<kaseifu::rename::service::RenamePlan> {
    let PlanOutcome::Ready(plan) = outcome else {
        panic!("expected a ready plan, got {outcome:?}");
    };
    plan
}

fn held(outcome: PlanOutcome) -> (String, HoldFacts) {
    let PlanOutcome::Held { reason, facts, .. } = outcome else {
        panic!("expected a held plan, got {outcome:?}");
    };
    (reason, facts)
}

// ---------------------------------------------------------------------------
// The reason names the cause
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v116_a_title_separator_names_the_title() {
    let dir = work_dir("image-separator");
    // The issue's fixture name: a chart PNG whose perception is unavailable.
    let target = write_bytes(&dir, "cbd-common-scale-5y.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(Some("cbd/common scale")));

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, _) = held(outcome);

    assert!(
        reason.contains("model's title contains a path separator"),
        "the reason must name the model's title: {reason}"
    );
    assert!(
        !reason.contains(SEPARATOR_WORDING),
        "the validator wording blames the file name: {reason}"
    );
    assert!(
        reason.contains("proposal withheld"),
        "a hold says what it did: {reason}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v116_a_document_title_separator_names_the_title() {
    let dir = work_dir("document-separator");
    let target = write_bytes(&dir, "notes.md", b"# Meeting notes\n\nA short agenda.\n");
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(Some("agenda: 2026 kickoff")));

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, _) = held(outcome);

    assert!(
        reason.contains("model's title contains a path separator"),
        "a document title is a model answer too: {reason}"
    );
    assert!(!reason.contains(SEPARATOR_WORDING), "{reason}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// No proposal at all
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v116_an_empty_title_is_held_as_no_title() {
    let dir = work_dir("image-empty-title");
    let target = write_bytes(&dir, "chart.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    // Perception answers, but the model returns nothing to name the file.
    let provider = FakeSemanticProvider::returning(info(None)).with_perception(&[("chart", 0.7)]);

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, _) = held(outcome);

    assert!(
        reason.contains("no title could be drawn from this file"),
        "a missing title is its own reason: {reason}"
    );
    assert!(
        !reason.contains("missing: title"),
        "the template's internal word does not reach the user: {reason}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v116_a_document_without_title_is_held_as_no_title() {
    let dir = work_dir("document-empty-title");
    let target = write_bytes(&dir, "minutes.txt", b"Attendees: two people.\n");
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(Some("   ")));

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, _) = held(outcome);

    assert!(
        reason.contains("no title could be drawn from this file"),
        "whitespace is not a title: {reason}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v116_a_held_image_reports_the_missing_perception() {
    let dir = work_dir("perception-missing");
    let target = write_bytes(&dir, "scan.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(None));

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, facts) = held(outcome);

    assert!(
        reason.contains("image perception: unavailable: perception-not-configured"),
        "the hold names why there is nothing to describe: {reason}"
    );
    assert!(
        matches!(facts.perception, Some(PerceptionReport::Unavailable(_))),
        "the report says unavailable instead of null: {:?}",
        facts.perception
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The held report carries what it knew
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v116_a_held_plan_reports_what_it_knew() {
    let dir = work_dir("held-facts");
    let target = write_bytes(&dir, "cbd-common-scale-5y.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(Some("cbd/common scale")))
        .with_perception(&[("chart", 0.81)]);

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, facts) = held(outcome);

    assert!(reason.contains("model's title"), "{reason}");
    assert!(
        matches!(facts.perception, Some(PerceptionReport::Labels { .. })),
        "a held image keeps the labels it perceived: {:?}",
        facts.perception
    );
    assert!(
        facts.date_source.is_some(),
        "the date was resolved before the hold, so its source is known: {:?}",
        facts.date_source
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v116_a_hold_before_the_date_claims_no_date_source() {
    let dir = work_dir("held-no-date");
    let target = write_bytes(&dir, "you must rename me.png", &fixture_png());
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(Some("unused")));

    let outcome = plan_rename(&provider, &store, &target, &pattern_options("P02"))
        .await
        .expect("plan");
    let (reason, facts) = held(outcome);

    assert!(reason.contains("injection"), "{reason}");
    assert!(
        facts.date_source.is_none() && facts.perception.is_none(),
        "nothing was resolved yet, and nothing is invented"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Nothing else changes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v116_normal_proposals_do_not_change() {
    let dir = work_dir("normal");
    let store = RenameStore::open_in_memory().expect("store");
    let provider = FakeSemanticProvider::returning(info(Some("Meeting summary")));

    // A text file: P02 keeps the `YYYY-MM-DD_Title.ext` shape.
    let text = write_bytes(&dir, "hello-world.txt", b"Hello world.\n");
    let plan = ready(
        plan_rename(&provider, &store, &text, &pattern_options("P02"))
            .await
            .expect("plan"),
    );
    let prefix = plan.proposal.get(..10).expect("a date prefix");
    assert!(
        prefix.chars().nth(4) == Some('-') && plan.proposal.ends_with("_Meeting_summary.txt"),
        "proposal: {}",
        plan.proposal
    );
    assert!(
        plan.date_source == "created" || plan.date_source == "modified",
        "date_source: {}",
        plan.date_source
    );

    // An image: the same shape, with the extension kept.
    let image = write_bytes(&dir, "IMG_9001.png", &fixture_png());
    let provider = FakeSemanticProvider::returning(info(Some("Sunset over water")))
        .with_perception(&[("sunset", 0.9)]);
    let plan = ready(
        plan_rename(&provider, &store, &image, &pattern_options("P02"))
            .await
            .expect("plan"),
    );
    assert!(
        plan.proposal.ends_with("_Sunset_over_water.png"),
        "proposal: {}",
        plan.proposal
    );

    // A folder: no extension is appended and a dot stays part of the name.
    let folder = dir.join("image files");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("memo.txt"), b"A household receipt.\n").expect("child");
    std::fs::write(folder.join("second.txt"), b"A second household receipt.\n").expect("child");
    let provider = FakeSemanticProvider::returning(info(Some("Household paperwork v1.2")));
    let plan = ready(
        plan_rename(&provider, &store, &folder, &pattern_options("P02"))
            .await
            .expect("plan"),
    );
    assert!(
        plan.proposal.ends_with("_Household_paperwork_v1.2"),
        "proposal: {}",
        plan.proposal
    );
    let _ = std::fs::remove_dir_all(&dir);
}

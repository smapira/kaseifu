//! v0.9.8 renaming: a generation timeout is retried once, and every provider
//! failure reaches the user as a sentence.
//!
//! The measured case: `kaseifu rename IMG_2741.jpeg` reported
//! `Error: skipped: Timeout` on a real photo and answered on an immediate
//! retry (2026-10-15). The tests below fix both halves of that: one retry, and
//! a message that names the cause instead of leaking an internal state name.
//!
//! Model-independent: the provider is a stub, so nothing here needs Apple
//! Intelligence. Fixtures live under the system temp directory.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use kaseifu::models::file_context::{NormalizedFileContext, PerceptionInfo};
use kaseifu::rename::batch::{self, BatchOutcome};
use kaseifu::rename::service::{PlanOutcome, RenameError, RenameOptions, plan_rename};
use kaseifu::rename::store::RenameStore;
use kaseifu::semantic::contract::{Availability, SemanticInfo};
use kaseifu::semantic::perception::PerceptionError;
use kaseifu::semantic::provider::{SemanticError, SemanticProvider};
use kaseifu::semantic::video::VideoPerception;

// ---------------------------------------------------------------------------
// A provider that fails on purpose
// ---------------------------------------------------------------------------

/// What the stub does when it is asked for an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Time out `timeouts` times, then answer.
    Flaky,
    /// Always fail the generation itself.
    InferenceFailed,
    /// Always answer with something that does not match the contract.
    InvalidResult,
}

/// A provider whose failures are the point of the test.
struct StubProvider {
    mode: Mode,
    timeouts_left: AtomicUsize,
    calls: AtomicUsize,
}

impl StubProvider {
    fn new(mode: Mode, timeouts: usize) -> Self {
        Self {
            mode,
            timeouts_left: AtomicUsize::new(timeouts),
            calls: AtomicUsize::new(0),
        }
    }

    /// How many generations were attempted (a retry counts).
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl SemanticProvider for StubProvider {
    fn availability(&self) -> Result<Availability, SemanticError> {
        Ok(Availability::Available)
    }

    fn perceive(&self, _path: &Path) -> Result<PerceptionInfo, PerceptionError> {
        Err(PerceptionError::Unavailable {
            reason: "the stub does not perceive".to_string(),
        })
    }

    fn perceive_video(&self, _path: &Path) -> Result<VideoPerception, PerceptionError> {
        Err(PerceptionError::Unavailable {
            reason: "the stub does not perceive".to_string(),
        })
    }

    fn analyze<'a>(
        &'a self,
        _context: &'a NormalizedFileContext,
    ) -> Pin<Box<dyn Future<Output = Result<SemanticInfo, SemanticError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.mode {
                Mode::Flaky if self.timeouts_left.load(Ordering::SeqCst) > 0 => {
                    self.timeouts_left.fetch_sub(1, Ordering::SeqCst);
                    Err(SemanticError::Timeout)
                }
                Mode::Flaky => Ok(SemanticInfo {
                    title: Some("retried title".to_string()),
                    category: None,
                    project: None,
                }),
                Mode::InferenceFailed => {
                    Err(SemanticError::InferenceFailed("guardrail violation".into()))
                }
                Mode::InvalidResult => {
                    Err(SemanticError::InvalidResult("expected an object".into()))
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Fixtures (synthetic, temp dir only)
// ---------------------------------------------------------------------------

fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaseifu-v098-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::canonicalize(&dir).expect("canonical temp dir")
}

fn write_document(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, "a short note about the launch plan\n").expect("fixture file");
    path
}

/// A template with no variables, so the proposal is only the model's title.
fn title_only() -> RenameOptions {
    RenameOptions {
        template: Some("{title}".to_string()),
        ..RenameOptions::default()
    }
}

fn store() -> RenameStore {
    RenameStore::open_in_memory().expect("store")
}

// ---------------------------------------------------------------------------
// One retry, then a sentence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn v098_a_timeout_is_retried_once_and_then_succeeds() {
    let dir = work_dir("retry");
    let file = write_document(&dir, "note.txt");
    let provider = StubProvider::new(Mode::Flaky, 1);

    let outcome = plan_rename(&provider, &store(), &file, &title_only())
        .await
        .expect("the retry answers");
    match outcome {
        PlanOutcome::Ready(plan) => assert_eq!(plan.proposal, "retried_title.txt"),
        other => panic!("expected a ready plan, got {other:?}"),
    }
    assert_eq!(provider.calls(), 2, "exactly one retry");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v098_a_second_timeout_reports_a_sentence_without_internal_words() {
    let dir = work_dir("twice");
    let file = write_document(&dir, "note.txt");
    let provider = StubProvider::new(Mode::Flaky, 99);

    let error = plan_rename(&provider, &store(), &file, &title_only())
        .await
        .expect_err("two timeouts fail the plan");
    assert!(matches!(error, RenameError::ModelTimeout), "{error:?}");
    let text = error.to_string();
    assert!(text.contains("did not answer in time"), "{text}");
    assert!(text.contains("the file was not changed"), "{text}");
    assert!(
        !text.contains("skipped") && !text.contains("Timeout"),
        "no internal state name: {text}"
    );
    assert_eq!(provider.calls(), 2, "one retry, not a loop");
    assert!(file.is_file(), "planning changes nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn v098_only_a_timeout_is_retried() {
    for (mode, needle) in [
        (Mode::InferenceFailed, "the model could not answer"),
        (Mode::InvalidResult, "did not match the expected shape"),
    ] {
        let dir = work_dir("other");
        let file = write_document(&dir, "note.txt");
        let provider = StubProvider::new(mode, 0);

        let error = plan_rename(&provider, &store(), &file, &title_only())
            .await
            .expect_err("the failure reaches the caller");
        let text = error.to_string();
        assert!(text.contains(needle), "{text}");
        assert!(text.contains("the file was not changed"), "{text}");
        assert!(
            !text.contains("InferenceFailed") && !text.contains("InvalidResult"),
            "no internal type name: {text}"
        );
        assert_eq!(provider.calls(), 1, "only a timeout is retried: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[tokio::test]
async fn v098_a_failing_target_does_not_stop_a_batch() {
    let dir = work_dir("batch");
    write_document(&dir, "a.txt");
    write_document(&dir, "b.txt");
    // The first target spends both attempts, so the second one answers.
    let provider = StubProvider::new(Mode::Flaky, 2);

    let walk = batch::walk(&dir).expect("walk");
    let report = batch::plan(&provider, &store(), &walk, &dir, &title_only()).await;

    assert_eq!(report.held(), 1, "{report:?}");
    assert_eq!(report.proposals(), 1, "{report:?}");
    let held = report
        .items
        .iter()
        .find(|item| matches!(item.outcome, BatchOutcome::Held { .. }))
        .expect("one held item");
    let BatchOutcome::Held { reason } = &held.outcome else {
        unreachable!()
    };
    assert!(reason.contains("did not answer in time"), "{reason}");
    assert!(!reason.contains("Timeout"), "{reason}");
    assert_eq!(
        provider.calls(),
        3,
        "two attempts for the first, one for the second"
    );
    assert!(dir.join("a.txt").is_file() && dir.join("b.txt").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

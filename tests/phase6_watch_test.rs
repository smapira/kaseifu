//! Phase 6 - watch automation tests.
//!
//! Offline only: fake events, temp fixtures, injected timestamps. No real
//! FSEvents, no Jev API, no user directories.

use semantic_file_organizer::classifier::mock_jev::MockJevIntegration;
use semantic_file_organizer::storage::history::OperationHistory;
use semantic_file_organizer::watch::coordinator::{
    Disposition, EventCoordinator, ProcessingRequest, RawEvent, SuppressReason, WatchConfig,
};
use semantic_file_organizer::watch::runner::{ProcessOutcome, process_request};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn make_fixture(tag: &str) -> Fixture {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("phase6-fixture-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create fixture root");
    Fixture { root }
}

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write fixture file");
}

fn config(managed_paths: Vec<PathBuf>) -> WatchConfig {
    WatchConfig {
        debounce: Duration::from_millis(500),
        stability: Duration::from_secs(1),
        self_suppression: Duration::from_secs(10),
        managed_paths,
    }
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn event(path: &Path) -> RawEvent {
    RawEvent {
        path: path.to_path_buf(),
    }
}

/// Drive a pending path through debounce + stability and return the requests.
fn drain_later(coordinator: &mut EventCoordinator, t0: Instant) -> Vec<PathBuf> {
    coordinator.drain_due(t0 + ms(600)); // debounce passed, snapshot taken
    coordinator
        .drain_due(t0 + ms(600) + secs(1)) // stable -> process
        .into_iter()
        .map(|request| request.path)
        .collect()
}

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

#[test]
fn phase6_normal_file_event_becomes_one_request() {
    let fixture = make_fixture("normal");
    let file = fixture.root.join("note.txt");
    write_file(&file, "hello");

    let mut coordinator = EventCoordinator::new(config(vec![]));
    let t0 = Instant::now();

    assert_eq!(coordinator.submit(event(&file), t0), Disposition::Accepted);
    assert!(
        coordinator.drain_due(t0).is_empty(),
        "debounce must not have elapsed yet"
    );

    let requests = drain_later(&mut coordinator, t0);
    assert_eq!(requests, vec![file.clone()]);
    assert!(
        coordinator.drain_due(t0 + secs(30)).is_empty(),
        "a path must not be processed twice"
    );
}

#[test]
fn phase6_directory_event_is_ignored() {
    let fixture = make_fixture("directory");
    let directory = fixture.root.join("folder");
    fs::create_dir_all(&directory).unwrap();

    let mut coordinator = EventCoordinator::new(config(vec![]));

    assert_eq!(
        coordinator.submit(event(&directory), Instant::now()),
        Disposition::Suppressed(SuppressReason::Directory)
    );
}

#[test]
fn phase6_missing_path_is_ignored() {
    let fixture = make_fixture("missing");
    let missing = fixture.root.join("not-there.txt");

    let mut coordinator = EventCoordinator::new(config(vec![]));

    assert_eq!(
        coordinator.submit(event(&missing), Instant::now()),
        Disposition::Suppressed(SuppressReason::Missing)
    );
}

#[test]
fn phase6_temporary_files_are_ignored() {
    let fixture = make_fixture("temporary");
    let mut coordinator = EventCoordinator::new(config(vec![]));

    for name in [
        "note.txt.tmp",
        "download.crdownload",
        ".DS_Store",
        ".hidden",
    ] {
        let path = fixture.root.join(name);
        write_file(&path, "partial");
        assert_eq!(
            coordinator.submit(event(&path), Instant::now()),
            Disposition::Suppressed(SuppressReason::Temporary),
            "{name} must be ignored"
        );
    }
}

#[test]
fn phase6_managed_data_is_ignored() {
    let fixture = make_fixture("managed");
    let data_dir = fixture.root.join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let journal = data_dir.join("semantic-file-organizer.db");
    write_file(&journal, "sqlite");

    let mut coordinator = EventCoordinator::new(config(vec![journal.clone()]));

    assert_eq!(
        coordinator.submit(event(&journal), Instant::now()),
        Disposition::Suppressed(SuppressReason::ManagedData)
    );
}

// ---------------------------------------------------------------------------
// Debounce / dedupe / stability
// ---------------------------------------------------------------------------

#[test]
fn phase6_duplicate_events_collapse_into_one_request() {
    let fixture = make_fixture("debounce");
    let file = fixture.root.join("note.txt");
    write_file(&file, "hello");

    let mut coordinator = EventCoordinator::new(config(vec![]));
    let t0 = Instant::now();

    // A single save produces several events.
    for offset in [0, 100, 200] {
        assert_eq!(
            coordinator.submit(event(&file), t0 + ms(offset)),
            Disposition::Accepted
        );
    }

    // The debounce window restarts with each event.
    assert!(coordinator.drain_due(t0 + ms(600)).is_empty());

    let requests = drain_later(&mut coordinator, t0 + ms(200));
    assert_eq!(requests, vec![file.clone()], "exactly one request");
}

#[test]
fn phase6_in_flight_duplicate_is_rejected() {
    let fixture = make_fixture("in-flight");
    let file = fixture.root.join("note.txt");
    write_file(&file, "hello");

    let mut coordinator = EventCoordinator::new(config(vec![]));
    let t0 = Instant::now();

    coordinator.submit(event(&file), t0);
    assert_eq!(drain_later(&mut coordinator, t0), vec![file.clone()]);

    // The runner starts processing.
    coordinator.begin_processing(&file);

    assert_eq!(
        coordinator.submit(event(&file), t0 + secs(3)),
        Disposition::Suppressed(SuppressReason::DuplicateInFlight)
    );
    assert!(
        coordinator.drain_due(t0 + secs(5)).is_empty(),
        "an in-flight path must not be queued again"
    );

    // After processing finishes, new events are accepted again.
    coordinator.finish_processing(&file);
    assert_eq!(
        coordinator.submit(event(&file), t0 + secs(6)),
        Disposition::Accepted
    );
}

#[test]
fn phase6_unstable_file_is_deferred_until_stable() {
    let fixture = make_fixture("unstable");
    let file = fixture.root.join("growing.txt");
    write_file(&file, "a");

    let mut coordinator = EventCoordinator::new(config(vec![]));
    let t0 = Instant::now();

    coordinator.submit(event(&file), t0);
    assert!(
        coordinator.drain_due(t0 + ms(600)).is_empty(),
        "first observation only snapshots the file"
    );

    // The file is still being written.
    write_file(&file, "a-much-longer-payload");
    assert!(
        coordinator.drain_due(t0 + ms(600) + secs(1)).is_empty(),
        "a changed file must be re-observed, not processed"
    );

    let requests = coordinator
        .drain_due(t0 + ms(600) + secs(2))
        .into_iter()
        .map(|request| request.path)
        .collect::<Vec<_>>();
    assert_eq!(requests, vec![file.clone()], "stable file is processed");
}

#[test]
fn phase6_path_that_vanishes_during_debounce_is_dropped() {
    let fixture = make_fixture("vanished");
    let file = fixture.root.join("temporary.txt");
    write_file(&file, "here for a moment");

    let mut coordinator = EventCoordinator::new(config(vec![]));
    let t0 = Instant::now();

    coordinator.submit(event(&file), t0);
    fs::remove_file(&file).unwrap();

    assert!(
        coordinator.drain_due(t0 + ms(600) + secs(2)).is_empty(),
        "a vanished path must be dropped without a request"
    );
}

// ---------------------------------------------------------------------------
// Self-generated events
// ---------------------------------------------------------------------------

#[test]
fn phase6_self_generated_move_events_are_suppressed() {
    let fixture = make_fixture("self");
    let source = fixture.root.join("a.txt");
    let destination = fixture.root.join("b.txt");
    write_file(&source, "payload");
    write_file(&destination, "payload");

    let mut coordinator = EventCoordinator::new(config(vec![]));
    let t0 = Instant::now();

    // We performed a move: both paths will produce events.
    coordinator.record_self_operation(&[source.clone(), destination.clone()], t0);

    for path in [&source, &destination] {
        assert_eq!(
            coordinator.submit(event(path), t0 + secs(1)),
            Disposition::Suppressed(SuppressReason::SelfGenerated),
            "our own move must not feed back into the pipeline"
        );
    }

    // The suppression expires.
    assert_eq!(
        coordinator.submit(event(&destination), t0 + secs(11)),
        Disposition::Accepted
    );
}

// ---------------------------------------------------------------------------
// Runner: error isolation (offline, mock Jev, temp fixtures)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase6_failing_file_does_not_stop_the_watcher() {
    let fixture = make_fixture("isolation");
    let good = fixture.root.join("note.txt");
    write_file(&good, "hello");
    let vanished = fixture.root.join("never-existed.txt");

    let jev = MockJevIntegration::default();
    let history = OperationHistory::open_in_memory().unwrap();
    let mut coordinator = EventCoordinator::new(config(vec![]));

    // A path that cannot even be observed fails this request only.
    let failed = process_request(
        &ProcessingRequest {
            path: vanished.clone(),
        },
        &mut coordinator,
        &jev,
        &history,
        false,
    )
    .await;
    assert!(
        matches!(failed, ProcessOutcome::Failed(_)),
        "expected a per-file failure, got {failed:?}"
    );

    // The watcher continues with the next file.
    let next = process_request(
        &ProcessingRequest { path: good.clone() },
        &mut coordinator,
        &jev,
        &history,
        false,
    )
    .await;
    assert!(
        !matches!(next, ProcessOutcome::Failed(_)),
        "the next file must still be processed: {next:?}"
    );

    // Neither request left the path locked (no permanent in-flight state).
    assert_eq!(
        coordinator.submit(event(&good), Instant::now()),
        Disposition::Accepted
    );
}

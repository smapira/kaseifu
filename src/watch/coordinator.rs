//! Phase 6 - filesystem-event processing logic, independent of any OS API.
//!
//! ```text
//! RawEvent -> filter -> debounce -> dedupe -> stability -> ProcessingRequest
//! ```
//!
//! FSEvents is only a *trigger*: the current file state always comes from
//! the filesystem (and, downstream, from Phase 1 Observe). Event metadata is
//! never turned into a semantic decision.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Operational timing values.
///
/// These are **not** semantic policy: they only control how aggressively
/// identical events are collapsed. The Phase 5 execution policy
/// (confidence/margin thresholds) is unaffected by the watcher.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Coalesce events for the same path within this window.
    pub debounce: Duration,
    /// A file must be unchanged for this long before it is processed.
    pub stability: Duration,
    /// Suppress events caused by our own moves for this long.
    pub self_suppression: Duration,
    /// Paths we manage ourselves (journal, data) and must never process.
    pub managed_paths: Vec<PathBuf>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            stability: Duration::from_secs(1),
            self_suppression: Duration::from_secs(10),
            managed_paths: vec![PathBuf::from(
                crate::storage::history::DEFAULT_DATABASE_PATH,
            )],
        }
    }
}

/// A normalized filesystem event from any source (adapter, test).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    pub path: PathBuf,
}

/// Why an event will not become a processing request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressReason {
    Directory,
    Missing,
    Temporary,
    /// Our own journal / managed data.
    ManagedData,
    /// The path is currently being processed.
    DuplicateInFlight,
    /// The event was caused by our own move/undo.
    SelfGenerated,
}

/// Outcome of submitting an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// Stored as pending; a request may follow once debounce/stability pass.
    Accepted,
    Suppressed(SuppressReason),
}

/// A path that passed every gate and is ready for the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessingRequest {
    pub path: PathBuf,
}

/// Identifies a file's content state without reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

/// Per-path debounce/stability state.
#[derive(Debug, Clone)]
struct PendingPath {
    last_event_at: Instant,
    snapshot: Option<FileFingerprint>,
    snapshot_at: Option<Instant>,
}

/// Coalesces raw events into processing requests.
pub struct EventCoordinator {
    config: WatchConfig,
    pending: HashMap<PathBuf, PendingPath>,
    in_flight: HashSet<PathBuf>,
    self_events: HashMap<PathBuf, Instant>,
}

impl EventCoordinator {
    pub fn new(config: WatchConfig) -> Self {
        Self {
            config,
            pending: HashMap::new(),
            in_flight: HashSet::new(),
            self_events: HashMap::new(),
        }
    }

    /// Submit a raw event; returns what happened to it.
    pub fn submit(&mut self, event: RawEvent, now: Instant) -> Disposition {
        let path = event.path;

        // Never re-enter a path that is being processed.
        if self.in_flight.contains(&path) {
            return Disposition::Suppressed(SuppressReason::DuplicateInFlight);
        }

        // Ignore events caused by our own moves/undo.
        if self.is_self_suppressed(&path, now) {
            return Disposition::Suppressed(SuppressReason::SelfGenerated);
        }

        // The filesystem is the source of truth, not the event.
        if !path.exists() {
            return Disposition::Suppressed(SuppressReason::Missing);
        }
        if path.is_dir() {
            return Disposition::Suppressed(SuppressReason::Directory);
        }
        if is_temporary(&path) {
            return Disposition::Suppressed(SuppressReason::Temporary);
        }
        if self.is_managed(&path) {
            return Disposition::Suppressed(SuppressReason::ManagedData);
        }

        // Coalesce with any pending events for the same path (debounce).
        self.pending
            .entry(path)
            .and_modify(|pending| pending.last_event_at = now)
            .or_insert(PendingPath {
                last_event_at: now,
                snapshot: None,
                snapshot_at: None,
            });

        Disposition::Accepted
    }

    /// Collect paths that are debounced, stable and not suppressed.
    pub fn drain_due(&mut self, now: Instant) -> Vec<ProcessingRequest> {
        let mut requests = Vec::new();

        for path in self.pending.keys().cloned().collect::<Vec<_>>() {
            let Some(mut pending) = self.pending.remove(&path) else {
                continue;
            };

            // Still coalescing events for this path.
            if now.duration_since(pending.last_event_at) < self.config.debounce {
                self.pending.insert(path, pending);
                continue;
            }

            // Became in-flight after being queued.
            if self.in_flight.contains(&path) {
                continue;
            }

            // Caused by our own move after all (or it arrived late).
            if self.is_self_suppressed(&path, now) {
                continue;
            }

            // Stability gate: only process a file that stopped changing.
            let Ok(metadata) = std::fs::metadata(&path) else {
                // Vanished while pending; nothing to do.
                continue;
            };
            let fingerprint = FileFingerprint {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            };

            match pending.snapshot {
                None => {
                    pending.snapshot = Some(fingerprint);
                    pending.snapshot_at = Some(now);
                    self.pending.insert(path, pending);
                }
                Some(previous) if previous != fingerprint => {
                    // Still being written: re-observe.
                    pending.snapshot = Some(fingerprint);
                    pending.snapshot_at = Some(now);
                    self.pending.insert(path, pending);
                }
                Some(_) => {
                    let observed_for = pending
                        .snapshot_at
                        .map(|at| now.duration_since(at))
                        .unwrap_or_default();

                    if observed_for < self.config.stability {
                        self.pending.insert(path, pending);
                    } else {
                        requests.push(ProcessingRequest { path });
                    }
                }
            }
        }

        requests
    }

    /// Mark a request as being processed (deduplication).
    pub fn begin_processing(&mut self, path: &Path) {
        self.pending.remove(path);
        self.in_flight.insert(path.to_path_buf());
    }

    /// Mark processing as finished.
    pub fn finish_processing(&mut self, path: &Path) {
        self.in_flight.remove(path);
    }

    /// Record a mutation we performed, so its events are suppressed.
    pub fn record_self_operation(&mut self, paths: &[PathBuf], now: Instant) {
        for path in paths {
            self.self_events.insert(path.clone(), now);
        }
    }

    fn is_self_suppressed(&self, path: &Path, now: Instant) -> bool {
        self.self_events
            .get(path)
            .is_some_and(|at| now.duration_since(*at) < self.config.self_suppression)
    }

    fn is_managed(&self, path: &Path) -> bool {
        self.config
            .managed_paths
            .iter()
            .any(|managed| path == managed || path.starts_with(managed))
    }
}

/// Files that are still being produced, or are system noise.
fn is_temporary(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if name.starts_with('.') {
        return true;
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);

    matches!(
        extension.as_deref(),
        Some("tmp" | "part" | "crdownload" | "download" | "filepart" | "swp")
    )
}

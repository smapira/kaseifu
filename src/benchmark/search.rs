//! Scoped Spotlight search for the benchmark.
//!
//! The product path (`platform::macos::spotlight::search`) is deliberately not
//! touched: it runs a query with no scope and no timeout, which is wrong for a
//! benchmark that must never look outside its corpus and must not hang on a
//! pathological query. This module adds both, and only the benchmark uses it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// Poll interval while waiting for `mdfind`.
const POLL_MS: u64 = 20;

/// `mdfind -onlyin <root> <query>` with a hard timeout.
///
/// A timeout is not an error: it is counted and reported as an empty result,
/// because one slow query must not abort a long evaluation. Killing the child
/// is safe for the corpus, which `mdfind` only reads.
#[derive(Debug)]
pub struct ScopedSearch {
    root: PathBuf,
    timeout: Duration,
    timeouts: AtomicUsize,
}

impl ScopedSearch {
    pub fn new(root: impl Into<PathBuf>, timeout_ms: u64) -> Self {
        Self {
            root: root.into(),
            timeout: Duration::from_millis(timeout_ms.max(1)),
            timeouts: AtomicUsize::new(0),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn timeout_ms(&self) -> u64 {
        self.timeout.as_millis() as u64
    }

    /// How many queries were killed on timeout.
    pub fn timeouts(&self) -> usize {
        self.timeouts.load(Ordering::Relaxed)
    }

    /// The counter behind [`ScopedSearch::timeouts`], for recording the count
    /// in a run report after the evaluation finished.
    pub fn timeout_counter(&self) -> &AtomicUsize {
        &self.timeouts
    }

    /// Search boundary handed to `find_similar_files_with` (same signature as
    /// the product search, so the Discovery pipeline is unchanged).
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        let args = vec![
            "-onlyin".to_string(),
            self.root.to_string_lossy().to_string(),
            query.to_string(),
        ];
        match run_mdfind(&args, limit, self.timeout)? {
            Some(paths) => Ok(paths),
            None => {
                self.timeouts.fetch_add(1, Ordering::Relaxed);
                Ok(Vec::new())
            }
        }
    }

    /// Same as [`ScopedSearch::search`] but reports the timeout explicitly.
    pub fn search_checked(&self, query: &str, limit: usize) -> Result<Option<Vec<String>>> {
        let args = vec![
            "-onlyin".to_string(),
            self.root.to_string_lossy().to_string(),
            query.to_string(),
        ];
        let outcome = run_mdfind(&args, limit, self.timeout)?;
        if outcome.is_none() {
            self.timeouts.fetch_add(1, Ordering::Relaxed);
        }
        Ok(outcome)
    }
}

/// Run one `mdfind` invocation. `Ok(None)` means the timeout fired.
///
/// stdout is drained on a reader thread: without that, a result set larger
/// than the pipe buffer would block the child until the timeout, turning a
/// successful query into a timeout.
pub fn run_mdfind(args: &[String], limit: usize, timeout: Duration) -> Result<Option<Vec<String>>> {
    let mut child = Command::new("mdfind")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to execute mdfind")?;
    let stdout = child
        .stdout
        .take()
        .context("mdfind stdout is unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        let mut handle = stdout;
        let _ = handle.read_to_string(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut success = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                success = status.success();
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
    let text = reader.join().unwrap_or_default();
    if timed_out {
        return Ok(None);
    }
    if !success {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(
        text.lines().take(limit).map(ToOwned::to_owned).collect(),
    ))
}

/// Raw `mdutil -s <path>` status text, or an empty string when unavailable
/// (the tool can refuse without privileges; that is recorded, not fatal).
pub fn mdutil_status(path: &Path) -> String {
    Command::new("mdutil")
        .arg("-s")
        .arg(path)
        .output()
        .ok()
        .map(|output| {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            text.trim().to_string()
        })
        .unwrap_or_default()
}

/// Parse the indexing state out of `mdutil` output; `None` when it says
/// nothing usable.
pub fn parse_indexing_enabled(mdutil_output: &str) -> Option<bool> {
    let lowered = mdutil_output.to_lowercase();
    if lowered.contains("indexing enabled") {
        return Some(true);
    }
    if lowered.contains("indexing disabled") {
        return Some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mdutil_output() {
        assert_eq!(parse_indexing_enabled("Indexing enabled."), Some(true));
        assert_eq!(parse_indexing_enabled("Indexing disabled."), Some(false));
        assert_eq!(parse_indexing_enabled("no index"), None);
    }

    #[test]
    fn scoped_search_keeps_the_scope_and_the_timeout() {
        let search = ScopedSearch::new("/tmp/corpus", 1234);
        assert_eq!(search.root(), Path::new("/tmp/corpus"));
        assert_eq!(search.timeout_ms(), 1234);
        assert_eq!(search.timeouts(), 0);
    }

    /// `mdfind` is a macOS tool; elsewhere the call fails and the boundary
    /// reports it instead of panicking.
    #[test]
    fn search_reports_a_missing_binary_instead_of_panicking() {
        let search = ScopedSearch::new("/tmp/corpus", 100);
        let _ = search.search("kaseifu-bench-self-test", 10);
    }
}

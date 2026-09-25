//! macOS filesystem event adapter (FSEvents via the `notify` crate).
//!
//! This is the only place where OS event types appear. Everything
//! downstream receives plain paths; FSEvents is a *trigger*, never a source
//! of truth about a file.

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// Watch `roots` recursively; every changed path is sent to `sender`.
///
/// The returned watcher must be kept alive for as long as events are wanted.
pub fn spawn(roots: &[PathBuf], sender: Sender<PathBuf>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(event) = result {
            for path in event.paths {
                let _ = sender.send(path);
            }
        }
    })?;

    for root in roots {
        watcher.watch(root, RecursiveMode::Recursive)?;
    }

    Ok(watcher)
}

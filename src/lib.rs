//! kaseifu — Semantic file organizer for macOS.
//!
//! The pipeline is:
//!
//! ```text
//! Observe (context)
//!   -> Normalize (normalize)
//!   -> Discover (discovery)
//!   -> Decide (classifier)
//! ```

/// Debug macro — only prints when `KASEIFU_DEBUG=1` is set.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if std::env::var("KASEIFU_DEBUG").unwrap_or_default() == "1" {
            eprintln!("[debug] {}", format!($($arg)*));
        }
    };
}

pub mod classifier;
pub mod context;
pub mod discovery;
pub mod models;
pub mod normalize;
pub mod operations;
pub mod platform;
pub mod policy;
pub mod presentation;
pub mod storage;
pub mod watch;

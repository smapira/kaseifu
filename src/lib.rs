//! Semantic file organizer library.
//!
//! The pipeline is:
//!
//! ```text
//! Observe (context)
//!   -> Normalize (normalize)
//!   -> Discover (discovery)
//!   -> Decide (classifier)
//! ```

pub mod classifier;
pub mod context;
pub mod discovery;
pub mod models;
pub mod normalize;
pub mod operations;
pub mod platform;
pub mod policy;
pub mod storage;
pub mod watch;

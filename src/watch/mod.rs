//! Phase 6 - automation layer.
//!
//! Filesystem events are only a *trigger*; every decision still comes from
//! the existing Phase 1-5 pipeline.

pub mod coordinator;
pub mod runner;

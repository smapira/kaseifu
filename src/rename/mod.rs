//! AI rename service (original spec Phase 3 and Phase 4).
//!
//! Layering, from the bottom up:
//!
//! ```text
//! rules     (presets P01-P12, template variables, date formats and sources)
//! document  (bounded document text extraction, T: signals, document dates)
//! store     (user presets and the default rule in the existing SQLite DB)
//! validate  (file name checks, original spec 9.1)
//! service   (planner plus executor: SemanticProvider -> proposal -> journal)
//! ```
//!
//! The service reuses the observe/normalize pipeline, the semantic provider
//! boundary and the journal/undo design. It never invents a missing value
//! (original spec 6.5): a template whose required variable is empty is held
//! back with its reason.

pub mod document;
pub mod image;
pub mod rules;
pub mod service;
pub mod store;
pub mod validate;

//! Foundation Models integration (original spec phases 0-2).
//!
//! Layering, from the bottom up:
//!
//! ```text
//! contract  (fm-rename-v1 JSON types, bounds, parsing)
//!    ^
//! ffi       (raw C ABI + safe wrapper; ffi_stub.rs replaces Swift when it
//!    ^       could not be built)
//! bridge    (FmBridge: JSON in, JSON out; SwiftFmBridge is production)
//!    ^
//! provider  (SemanticProvider: NormalizedFileContext -> SemanticInfo)
//! ```
//!
//! Phase 5b adds the perception layer beside the generation path: Apple
//! Vision classifies the image (labels in, labels out) and Rust forwards the
//! bounded labels inside the same `fm-rename-v1` request, so the model can
//! title a picture instead of echoing its file name.
//!
//! Phase 6 widens perception to videos: AVFoundation measures the metadata
//! and extracts a bounded set of representative frames, Vision classifies
//! each frame, and Rust merges the frames into the same bounded label set
//! (original spec sections 4.4, 5.3 and 11.4). Nothing new is inferred here:
//! both media reuse one label budget and one prompt path.
//!
//! The Swift side (`swift/kaseifu_fm_bridge.swift`) only bridges Apple APIs:
//! file names, rules, journals and undo stay in Rust (original spec §2), and
//! the only files it ever reads are the image or video paths Rust hands it
//! (read-only; a video is never re-encoded or written). Nothing here writes to
//! the filesystem, the journal or the configuration.

pub mod bridge;
pub mod contract;
pub mod fake;
pub mod ffi;
pub mod perception;
pub mod provider;
pub mod video;

#[cfg(any(kaseifu_fm_stub = "no-sdk", kaseifu_fm_stub = "no-toolchain"))]
pub mod ffi_stub;

pub use bridge::{FmBridge, FmBridgeError, SwiftFmBridge};
pub use contract::{Availability, ContractError, SemanticInfo, TargetKind};
pub use ffi::{BRIDGE_KIND, FfiError, FmContext};
pub use perception::PerceptionError;
pub use provider::{FoundationModelsProvider, SemanticError, SemanticProvider};
pub use video::{FrameLabels, VideoPerception};

/// The reason the fallback build reports (`None` when the Swift bridge was
/// built).
#[cfg(any(kaseifu_fm_stub = "no-sdk", kaseifu_fm_stub = "no-toolchain"))]
pub const STUB_REASON: Option<&str> = Some(ffi_stub::REASON);

/// The reason the fallback build reports (`None` when the Swift bridge was
/// built).
#[cfg(not(any(kaseifu_fm_stub = "no-sdk", kaseifu_fm_stub = "no-toolchain")))]
pub const STUB_REASON: Option<&str> = None;

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
//! The Swift side (`swift/kaseifu_fm_bridge.swift`) only bridges Apple APIs:
//! file names, rules, journals and undo stay in Rust (original spec §2).
//! Nothing here writes to the filesystem, the journal or the configuration.

pub mod bridge;
pub mod contract;
pub mod fake;
pub mod ffi;
pub mod provider;

#[cfg(any(kaseifu_fm_stub = "no-sdk", kaseifu_fm_stub = "no-toolchain"))]
pub mod ffi_stub;

pub use bridge::{FmBridge, FmBridgeError, SwiftFmBridge};
pub use contract::{Availability, ContractError, SemanticInfo, TargetKind};
pub use ffi::{BRIDGE_KIND, FfiError, FmContext};
pub use provider::{FoundationModelsProvider, SemanticError, SemanticProvider};

/// The reason the fallback build reports (`None` when the Swift bridge was
/// built).
#[cfg(any(kaseifu_fm_stub = "no-sdk", kaseifu_fm_stub = "no-toolchain"))]
pub const STUB_REASON: Option<&str> = Some(ffi_stub::REASON);

/// The reason the fallback build reports (`None` when the Swift bridge was
/// built).
#[cfg(not(any(kaseifu_fm_stub = "no-sdk", kaseifu_fm_stub = "no-toolchain")))]
pub const STUB_REASON: Option<&str> = None;

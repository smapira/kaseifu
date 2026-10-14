//! The bridge boundary: JSON strings in, JSON strings out.
//!
//! The trait mirrors `classifier::jev::JevIntegration`: implementations only
//! move data across the boundary, they never validate meaning, score or touch
//! the filesystem. The production implementation is the Swift C ABI; tests
//! inject [`super::fake::FakeFmBridge`].

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use super::contract::{self, Availability, ContractError};
use super::ffi::{self, FfiError};

/// Transport-level failure at the bridge boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmBridgeError {
    /// No answer within the timeout; the generation was cancelled.
    Timeout,
    /// The bridge refused the request before starting (no callback followed).
    Rejected { code: i32, detail: String },
    /// The bridge answered something that is not the contract.
    Malformed(String),
    /// The boundary itself failed.
    Internal(String),
}

impl From<FfiError> for FmBridgeError {
    fn from(error: FfiError) -> Self {
        match error {
            FfiError::Rejected { code, detail } => Self::Rejected { code, detail },
            FfiError::Timeout => Self::Timeout,
            FfiError::Internal(message) => Self::Internal(message),
        }
    }
}

impl From<ContractError> for FmBridgeError {
    fn from(error: ContractError) -> Self {
        Self::Malformed(error.to_string())
    }
}

impl std::fmt::Display for FmBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "Timeout"),
            Self::Rejected { code, detail } => write!(f, "Rejected({code}: {detail})"),
            Self::Malformed(message) => write!(f, "Malformed({message})"),
            Self::Internal(message) => write!(f, "Internal({message})"),
        }
    }
}

/// The Foundation Models boundary.
pub trait FmBridge: Send + Sync {
    /// Ask the on-device model's availability (with its explicit reason).
    fn available(&self) -> Result<Availability, FmBridgeError>;

    /// Run one generation and answer with the raw `fm-rename-v1` response JSON.
    fn generate<'a>(
        &'a self,
        request_json: &'a str,
        timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<String, FmBridgeError>> + Send + 'a>>;
}

/// The production bridge: one context per generation, created and destroyed
/// inside the call (session reuse is a Phase 9 concern).
#[derive(Debug, Default, Clone, Copy)]
pub struct SwiftFmBridge;

impl SwiftFmBridge {
    pub fn new() -> Self {
        Self
    }
}

impl FmBridge for SwiftFmBridge {
    fn available(&self) -> Result<Availability, FmBridgeError> {
        let json = ffi::available().map_err(FmBridgeError::from)?;
        contract::parse_availability(&json).map_err(FmBridgeError::from)
    }

    fn generate<'a>(
        &'a self,
        request_json: &'a str,
        timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<String, FmBridgeError>> + Send + 'a>> {
        Box::pin(async move {
            let context = ffi::FmContext::create().map_err(FmBridgeError::from)?;
            context
                .generate(request_json, timeout)
                .await
                .map_err(FmBridgeError::from)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swift_bridge_reports_availability_or_an_explicit_reason() {
        let availability = SwiftFmBridge::new()
            .available()
            .expect("the bridge answers");
        match availability {
            Availability::Available => {}
            Availability::Unavailable { reason, .. } => {
                assert!(
                    !reason.is_empty(),
                    "an unavailable model must name its reason"
                );
            }
        }
    }
}

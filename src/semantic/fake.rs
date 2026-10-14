//! Test doubles for the semantic layer.
//!
//! Same role as `classifier::mock_jev`: they return what the test configured
//! and never contain business logic, so the pipeline and its tests never need
//! the on-device model (or an Apple Intelligence enabled machine).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::bridge::{FmBridge, FmBridgeError};
use super::contract::{self, Availability, ErrorKind, SemanticFields, SemanticInfo};
use super::perception::PerceptionError;
use super::provider::{SemanticError, SemanticProvider};
use super::video::{FrameLabels, VideoPerception};
use crate::models::file_context::{
    NormalizedFileContext, PerceptionInfo, PerceptionLabel, VideoInfo,
};

/// A configured outcome for [`FakeFmBridge`].
#[derive(Debug, Clone)]
enum Outcome {
    /// Answer with this exact string (malformed payloads, id mismatches).
    Raw(String),
    /// Answer with a success envelope that echoes the request id.
    Success(SemanticInfo),
    /// Answer with an error envelope that echoes the request id.
    Failure {
        kind: ErrorKind,
        reason: Option<String>,
    },
    /// Fail at the transport level.
    Error(FmBridgeError),
}

/// A bridge that answers with configured data and records its requests.
///
/// Cloning shares the same state, so a test can keep a handle after moving the
/// bridge into a provider.
#[derive(Debug, Clone)]
pub struct FakeFmBridge {
    inner: Arc<FakeFmBridgeInner>,
}

#[derive(Debug)]
struct FakeFmBridgeInner {
    availability: Result<Availability, FmBridgeError>,
    outcome: Mutex<Outcome>,
    requests: Mutex<Vec<String>>,
    perception: Mutex<Result<PerceptionInfo, PerceptionError>>,
    perceived: Mutex<Vec<PathBuf>>,
    video_perception: Mutex<Result<VideoPerception, PerceptionError>>,
    video_perceived: Mutex<Vec<PathBuf>>,
}

/// What a fake bridge answers when perception was not configured: the layer
/// cannot run here (the plan then degrades exactly like on a machine without
/// Vision, and no test ever reaches the OS).
fn unconfigured_perception() -> Result<PerceptionInfo, PerceptionError> {
    Err(PerceptionError::Unavailable {
        reason: "perception-not-configured".to_string(),
    })
}

/// What a fake bridge answers when video perception was not configured: the
/// same explicit degradation the unconfigured image path reports.
fn unconfigured_video_perception() -> Result<VideoPerception, PerceptionError> {
    Err(PerceptionError::Unavailable {
        reason: "video-perception-not-configured".to_string(),
    })
}

impl FakeFmBridge {
    /// A bridge whose model is available and that answers with `info`.
    pub fn answering(info: SemanticInfo) -> Self {
        Self::with_availability(Ok(Availability::Available), Outcome::Success(info))
    }

    /// A bridge whose model is available and that answers with `response`
    /// verbatim (for malformed payloads and id mismatches).
    pub fn responding(response: impl Into<String>) -> Self {
        Self::with_availability(Ok(Availability::Available), Outcome::Raw(response.into()))
    }

    /// A bridge that reports the model as unavailable and fails accordingly.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self::with_availability(
            Ok(Availability::Unavailable {
                reason: reason.clone(),
                detail: None,
            }),
            Outcome::Failure {
                kind: ErrorKind::Unavailable,
                reason: Some(reason),
            },
        )
    }

    /// A bridge that reports a specific model error.
    pub fn reporting(kind: ErrorKind, reason: Option<String>) -> Self {
        Self::with_availability(
            Ok(Availability::Available),
            Outcome::Failure { kind, reason },
        )
    }

    /// A bridge whose `generate` fails at the transport level.
    pub fn failing(error: FmBridgeError) -> Self {
        Self::with_availability(Ok(Availability::Available), Outcome::Error(error))
    }

    fn with_availability(
        availability: Result<Availability, FmBridgeError>,
        outcome: Outcome,
    ) -> Self {
        Self {
            inner: Arc::new(FakeFmBridgeInner {
                availability,
                outcome: Mutex::new(outcome),
                requests: Mutex::new(Vec::new()),
                perception: Mutex::new(unconfigured_perception()),
                perceived: Mutex::new(Vec::new()),
                video_perception: Mutex::new(unconfigured_video_perception()),
                video_perceived: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Answer perception with these labels (confidence pairs).
    pub fn perceiving(self, labels: &[(&str, f32)]) -> Self {
        let info = PerceptionInfo {
            labels: labels
                .iter()
                .map(|(label, confidence)| PerceptionLabel {
                    label: (*label).to_string(),
                    confidence: *confidence,
                })
                .collect(),
        };
        self.set_perception(Ok(info))
    }

    /// Answer perception with an explicit failure (unavailable, failed, ...).
    pub fn failing_perception(self, error: PerceptionError) -> Self {
        self.set_perception(Err(error))
    }

    fn set_perception(self, result: Result<PerceptionInfo, PerceptionError>) -> Self {
        *self.inner.perception.lock().expect("perception lock") = result;
        self
    }

    /// The image paths perception was asked about, in order.
    pub fn perceived_paths(&self) -> Vec<PathBuf> {
        self.inner.perceived.lock().expect("perceived lock").clone()
    }

    /// Answer video perception with these frames.
    pub fn perceiving_video(self, info: VideoInfo, frames: Vec<FrameLabels>) -> Self {
        *self
            .inner
            .video_perception
            .lock()
            .expect("video perception lock") = Ok(VideoPerception { info, frames });
        self
    }

    /// Answer video perception with an explicit failure (a video the bridge
    /// could not read, could not find a track in, or could not perceive).
    pub fn failing_video_perception(self, error: PerceptionError) -> Self {
        *self
            .inner
            .video_perception
            .lock()
            .expect("video perception lock") = Err(error);
        self
    }

    /// The video paths video perception was asked about, in order.
    pub fn video_perceived_paths(&self) -> Vec<PathBuf> {
        self.inner
            .video_perceived
            .lock()
            .expect("video perceived lock")
            .clone()
    }

    /// Replace the answer after construction.
    pub fn set_response(&self, response: impl Into<String>) {
        *self.inner.outcome.lock().expect("outcome lock") = Outcome::Raw(response.into());
    }

    /// The request JSONs the provider sent, in order.
    pub fn requests(&self) -> Vec<String> {
        self.inner.requests.lock().expect("requests lock").clone()
    }
}

impl FmBridge for FakeFmBridge {
    fn available(&self) -> Result<Availability, FmBridgeError> {
        self.inner.availability.clone()
    }

    fn perceive(&self, path: &Path) -> Result<PerceptionInfo, PerceptionError> {
        self.inner
            .perceived
            .lock()
            .expect("perceived lock")
            .push(path.to_path_buf());
        self.inner
            .perception
            .lock()
            .expect("perception lock")
            .clone()
    }

    fn video_perceive(&self, path: &Path) -> Result<VideoPerception, PerceptionError> {
        self.inner
            .video_perceived
            .lock()
            .expect("video perceived lock")
            .push(path.to_path_buf());
        self.inner
            .video_perception
            .lock()
            .expect("video perception lock")
            .clone()
    }

    fn generate<'a>(
        &'a self,
        request_json: &'a str,
        _timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<String, FmBridgeError>> + Send + 'a>> {
        self.inner
            .requests
            .lock()
            .expect("requests lock")
            .push(request_json.to_string());
        let outcome = self.inner.outcome.lock().expect("outcome lock").clone();
        let request_id = serde_json::from_str::<serde_json::Value>(request_json)
            .ok()
            .and_then(|value| value.get("request_id")?.as_str().map(str::to_string))
            .unwrap_or_default();
        Box::pin(async move {
            match outcome {
                Outcome::Raw(response) => Ok(response),
                Outcome::Success(info) => Ok(success_response(&request_id, &info)),
                Outcome::Failure { kind, reason } => {
                    Ok(failure_response(&request_id, &kind, reason.as_deref()))
                }
                Outcome::Error(error) => Err(error),
            }
        })
    }
}

/// A success envelope that echoes the request id.
fn success_response(request_id: &str, info: &SemanticInfo) -> String {
    let fields = SemanticFields {
        title: info.title.clone(),
        category: info.category.clone(),
        project: info.project.clone(),
    };
    serde_json::json!({
        "schema_version": contract::SCHEMA_VERSION,
        "request_id": request_id,
        "status": "success",
        "semantic": {
            "title": fields.title,
            "category": fields.category,
            "project": fields.project,
        },
    })
    .to_string()
}

/// An error envelope that echoes the request id.
fn failure_response(request_id: &str, kind: &ErrorKind, reason: Option<&str>) -> String {
    serde_json::json!({
        "schema_version": contract::SCHEMA_VERSION,
        "request_id": request_id,
        "status": "error",
        "semantic": serde_json::Value::Null,
        "error": { "kind": kind_str(kind), "reason": reason },
    })
    .to_string()
}

/// The wire name of an error kind (mirrors the Swift bridge).
fn kind_str(kind: &ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Unavailable => "unavailable",
        ErrorKind::ModelInitFailed => "model-init-failed",
        ErrorKind::InferenceFailed => "inference-failed",
        ErrorKind::InvalidOutput => "invalid-output",
        ErrorKind::Cancelled => "cancelled",
        ErrorKind::Internal => "internal",
        ErrorKind::Unknown => "unknown",
    }
}

/// A provider that answers with configured values (no bridge involved).
#[derive(Debug)]
pub struct FakeSemanticProvider {
    availability: Result<Availability, SemanticError>,
    result: Mutex<Result<SemanticInfo, SemanticError>>,
    perception: Mutex<Result<PerceptionInfo, PerceptionError>>,
    video_perception: Mutex<Result<VideoPerception, PerceptionError>>,
}

impl FakeSemanticProvider {
    /// A provider that is available and returns `info`.
    pub fn returning(info: SemanticInfo) -> Self {
        Self {
            availability: Ok(Availability::Available),
            result: Mutex::new(Ok(info)),
            perception: Mutex::new(unconfigured_perception()),
            video_perception: Mutex::new(unconfigured_video_perception()),
        }
    }

    /// A provider whose model is unavailable with `reason`.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            availability: Ok(Availability::Unavailable {
                reason: reason.clone(),
                detail: None,
            }),
            result: Mutex::new(Err(SemanticError::Unavailable { reason })),
            perception: Mutex::new(unconfigured_perception()),
            video_perception: Mutex::new(unconfigured_video_perception()),
        }
    }

    /// Answer perception with these labels (confidence pairs).
    pub fn with_perception(self, labels: &[(&str, f32)]) -> Self {
        let info = PerceptionInfo {
            labels: labels
                .iter()
                .map(|(label, confidence)| PerceptionLabel {
                    label: (*label).to_string(),
                    confidence: *confidence,
                })
                .collect(),
        };
        *self.perception.lock().expect("perception lock") = Ok(info);
        self
    }

    /// Answer video perception with these frames.
    pub fn with_video(self, info: VideoInfo, frames: Vec<FrameLabels>) -> Self {
        *self.video_perception.lock().expect("video perception lock") =
            Ok(VideoPerception { info, frames });
        self
    }
}

impl SemanticProvider for FakeSemanticProvider {
    fn availability(&self) -> Result<Availability, SemanticError> {
        self.availability.clone()
    }

    fn perceive(&self, _path: &Path) -> Result<PerceptionInfo, PerceptionError> {
        self.perception.lock().expect("perception lock").clone()
    }

    fn perceive_video(&self, _path: &Path) -> Result<VideoPerception, PerceptionError> {
        self.video_perception
            .lock()
            .expect("video perception lock")
            .clone()
    }

    fn analyze<'a>(
        &'a self,
        _context: &'a NormalizedFileContext,
    ) -> Pin<Box<dyn Future<Output = Result<SemanticInfo, SemanticError>> + Send + 'a>> {
        let result = self.result.lock().expect("result lock").clone();
        Box::pin(async move { result })
    }
}

//! The semantic provider boundary and the production implementation.
//!
//! `SemanticProvider` is the domain-facing contract (mirrors
//! `classifier::jev::JevIntegration`): it turns a normalized file observation
//! into semantic fields and never decides file names. The production
//! implementation talks to the Swift bridge; tests inject
//! [`super::fake::FakeFmBridge`].
//!
//! There is no fallback to another model: when Foundation Models is
//! unavailable the caller gets `SemanticError::Unavailable` with the reason
//! (original spec §4.2 / §12).

use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::bridge::{FmBridge, FmBridgeError, SwiftFmBridge};
use super::contract::{
    self, AnalysisFlags, Availability, ContractError, ErrorKind, FmDirectoryFacts, FmErrorInfo,
    FmImageFacts, FmInput, FmPerception, FmRequest, FmResponse, FmStatus, FmVideoFacts,
    MAX_FILENAME_CHARS, MAX_LANGUAGE_CHARS, MAX_PARENT_CHARS, MAX_PATH_CHARS, MAX_REQUEST_ID_CHARS,
    SCHEMA_VERSION, SemanticInfo, TargetKind,
};
use super::perception::PerceptionError;
use super::video::VideoPerception;
use crate::models::file_context::{NormalizedFileContext, PerceptionInfo};

/// Timeout for one generation (on-device inference plus scheduling).
pub const DEFAULT_FM_TIMEOUT: Duration = Duration::from_secs(20);

/// Language tag sent with every request (English unless overridden).
pub const DEFAULT_FM_LANGUAGE: &str = "en";

/// Why a semantic request did not produce fields (original spec §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticError {
    /// The model cannot run here (never falls back to another model).
    Unavailable { reason: String },
    /// The model session could not start: stop the run.
    ModelInitFailed(String),
    /// Inference failed for this target: skip it.
    InferenceFailed(String),
    /// No answer within the timeout: skip the target.
    Timeout,
    /// The request was cancelled: skip the target.
    Cancelled,
    /// The answer did not match the contract: skip the target.
    InvalidResult(String),
    /// The boundary itself failed.
    Bridge(String),
}

impl SemanticError {
    /// True when the caller should skip this target and continue.
    pub fn is_skippable(&self) -> bool {
        matches!(
            self,
            Self::InferenceFailed(_) | Self::Timeout | Self::Cancelled | Self::InvalidResult(_)
        )
    }

    /// True when the feature is unavailable on this machine.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { reason } => write!(f, "Unavailable({reason})"),
            Self::ModelInitFailed(message) => write!(f, "ModelInitFailed({message})"),
            Self::InferenceFailed(message) => write!(f, "InferenceFailed({message})"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::InvalidResult(message) => write!(f, "InvalidResult({message})"),
            Self::Bridge(message) => write!(f, "Bridge({message})"),
        }
    }
}

impl From<FmBridgeError> for SemanticError {
    fn from(error: FmBridgeError) -> Self {
        match error {
            FmBridgeError::Timeout => Self::Timeout,
            FmBridgeError::Rejected { code, detail } => {
                Self::Bridge(format!("rejected by the bridge (code {code}: {detail})"))
            }
            FmBridgeError::Malformed(message) => Self::InvalidResult(message),
            FmBridgeError::Internal(message) => Self::Bridge(message),
        }
    }
}

impl From<ContractError> for SemanticError {
    fn from(error: ContractError) -> Self {
        Self::InvalidResult(error.to_string())
    }
}

/// The semantic boundary.
pub trait SemanticProvider: Send + Sync {
    /// Model availability with the explicit reason when it cannot run.
    fn availability(&self) -> Result<Availability, SemanticError>;

    /// Image perception for one path (Apple Vision).
    ///
    /// Perception is an add-on, not a precondition: a failure never fails a
    /// plan, it only leaves the labels out (`PerceptionError` is a separate
    /// type on purpose, so it cannot be confused with model availability).
    fn perceive(&self, path: &Path) -> Result<PerceptionInfo, PerceptionError>;

    /// Video perception for one path (AVFoundation + Apple Vision, Phase 6).
    ///
    /// Unlike the image add-on this one is a precondition: a video is only
    /// worth a proposal when the frames could be seen (original spec sections
    /// 5.3 and 12), so a failure holds the target with its reason instead of
    /// proposing a name built from the file name alone.
    fn perceive_video(&self, path: &Path) -> Result<VideoPerception, PerceptionError>;

    /// Extract the semantic fields for one observed file.
    fn analyze<'a>(
        &'a self,
        context: &'a NormalizedFileContext,
    ) -> Pin<Box<dyn Future<Output = Result<SemanticInfo, SemanticError>> + Send + 'a>>;
}

/// The production provider (Foundation Models through the Swift bridge).
pub struct FoundationModelsProvider<B: FmBridge = SwiftFmBridge> {
    bridge: B,
    language: String,
    timeout: Duration,
    next_request: AtomicU64,
}

impl FoundationModelsProvider<SwiftFmBridge> {
    /// The provider the CLI will use (Phase 3 and later).
    pub fn production() -> Self {
        Self::with_bridge(SwiftFmBridge::new())
    }
}

impl<B: FmBridge> FoundationModelsProvider<B> {
    /// Build a provider around a bridge (tests inject a fake).
    pub fn with_bridge(bridge: B) -> Self {
        Self {
            bridge,
            language: DEFAULT_FM_LANGUAGE.to_string(),
            timeout: DEFAULT_FM_TIMEOUT,
            next_request: AtomicU64::new(0),
        }
    }

    /// Override the language tag (bounded).
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = contract::bound_chars(&language.into(), MAX_LANGUAGE_CHARS);
        self
    }

    /// Override the per-request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The bridge in use.
    pub fn bridge(&self) -> &B {
        &self.bridge
    }

    /// The next request id (`rename-0001`, `rename-0002`, ...).
    pub fn next_request_id(&self) -> String {
        let number = self.next_request.fetch_add(1, Ordering::Relaxed) + 1;
        contract::bound_chars(&format!("rename-{number:04}"), MAX_REQUEST_ID_CHARS)
    }
}

impl<B: FmBridge> SemanticProvider for FoundationModelsProvider<B> {
    fn availability(&self) -> Result<Availability, SemanticError> {
        self.bridge.available().map_err(SemanticError::from)
    }

    fn perceive(&self, path: &Path) -> Result<PerceptionInfo, PerceptionError> {
        // Defense in depth: only image extensions reach the classifier, so a
        // PDF or a video is refused here instead of being handed to ImageIO.
        if !crate::context::image_metadata::is_image_file(path) {
            return Err(PerceptionError::Failed {
                detail: format!("{} is not an image", path.display()),
            });
        }
        self.bridge.perceive(path)
    }

    fn perceive_video(&self, path: &Path) -> Result<VideoPerception, PerceptionError> {
        // Same defense in depth as the image path: only video extensions reach
        // AVFoundation, so a document or a picture is refused here instead of
        // being handed to a media framework that cannot read it.
        if !crate::context::video::is_video_file(path) {
            return Err(PerceptionError::Failed {
                detail: format!("{} is not a video", path.display()),
            });
        }
        self.bridge.video_perceive(path)
    }

    fn analyze<'a>(
        &'a self,
        context: &'a NormalizedFileContext,
    ) -> Pin<Box<dyn Future<Output = Result<SemanticInfo, SemanticError>> + Send + 'a>> {
        Box::pin(async move {
            let request_id = self.next_request_id();
            let request = build_request(context, &request_id, &self.language);
            let json = serde_json::to_string(&request)
                .map_err(|error| SemanticError::Bridge(error.to_string()))?;
            let raw = self
                .bridge
                .generate(&json, self.timeout)
                .await
                .map_err(SemanticError::from)?;
            let response = contract::parse_response(&raw).map_err(SemanticError::from)?;
            interpret(response, &request_id)
        })
    }
}

/// Turn one bounded observation into the request the bridge sends to the model.
pub fn build_request(
    context: &NormalizedFileContext,
    request_id: &str,
    language: &str,
) -> FmRequest {
    FmRequest {
        schema_version: SCHEMA_VERSION.to_string(),
        request_id: contract::bound_chars(request_id, MAX_REQUEST_ID_CHARS),
        target_type: target_kind(&context.target.path),
        path: contract::bound_chars(&context.target.path.to_string_lossy(), MAX_PATH_CHARS),
        language: contract::bound_chars(language, MAX_LANGUAGE_CHARS),
        analysis: AnalysisFlags::default(),
        input: FmInput {
            filename: contract::bound_chars(&context.target.filename, MAX_FILENAME_CHARS),
            extension: context
                .target
                .extension
                .clone()
                .filter(|value| !value.is_empty()),
            size_bytes: context.size_bytes,
            created_at: context.creation_date.clone(),
            modified_at: context
                .modification_date
                .clone()
                .or_else(|| context.target.modified_at.clone()),
            parent_directory: contract::bound_chars(&context.parent_directory, MAX_PARENT_CHARS),
            semantic_signals: contract::bound_signals(&context.semantic_signals),
            image: image_facts(context),
            perception: perception_facts(context),
            video: video_facts(context),
            directory: directory_facts(context),
        },
    }
}

/// The target kind this build can decide: folders by filesystem type, images
/// and videos by extension, the rest is a document.
///
/// The filesystem type wins over the name: a folder called `notes.png` is a
/// folder (original spec section 5.4).
pub fn target_kind(path: &Path) -> TargetKind {
    if crate::context::directory::is_directory_target(path) {
        TargetKind::Directory
    } else if crate::context::image_metadata::is_image_file(path) {
        TargetKind::Image
    } else if crate::context::video::is_video_file(path) {
        TargetKind::Video
    } else {
        TargetKind::Document
    }
}

/// Image facts already extracted in Rust (EXIF / Spotlight); `None` when the
/// observation carries nothing informative.
fn image_facts(context: &NormalizedFileContext) -> Option<FmImageFacts> {
    let metadata = context.image_metadata.as_ref()?;
    if metadata.is_empty() {
        return None;
    }
    Some(FmImageFacts {
        capture_time: metadata
            .capture_time
            .as_ref()
            .map(|capture| capture.value.clone()),
        camera: camera_label(&metadata.camera),
        width: metadata
            .dimensions
            .as_ref()
            .map(|dimensions| dimensions.width),
        height: metadata
            .dimensions
            .as_ref()
            .map(|dimensions| dimensions.height),
    })
}

/// Perception labels already extracted in Rust (Apple Vision, Phase 5b and
/// Phase 6); `None` when the observation carries none (documents, degraded
/// builds, or a classifier that found nothing).
fn perception_facts(context: &NormalizedFileContext) -> Option<FmPerception> {
    let info = context.perception.as_ref()?;
    if info.is_empty() {
        return None;
    }
    Some(FmPerception::from(info))
}

/// Video facts the bridge measured (AVFoundation, Phase 6); `None` when the
/// observation carries nothing informative (documents, images, or a container
/// that reported no value at all).
fn video_facts(context: &NormalizedFileContext) -> Option<FmVideoFacts> {
    let info = context.video.as_ref()?;
    if info.is_empty() {
        return None;
    }
    Some(FmVideoFacts::from(info))
}

/// Folder facts kaseifu read from the target's children (Phase 7); `None` when
/// the observation carries none (files, images, videos, or a folder that was
/// not read). The facts are already bounded by the context layer, and the wire
/// type bounds them again.
fn directory_facts(context: &NormalizedFileContext) -> Option<FmDirectoryFacts> {
    let facts = context.directory.as_ref()?;
    Some(FmDirectoryFacts::from(facts))
}

/// `make model`, omitting empty parts.
fn camera_label(camera: &crate::models::file_context::CameraInfo) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(make) = camera.make.as_deref().filter(|value| !value.is_empty()) {
        parts.push(make);
    }
    if let Some(model) = camera.model.as_deref().filter(|value| !value.is_empty()) {
        parts.push(model);
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Map one contract response onto the domain result.
fn interpret(response: FmResponse, request_id: &str) -> Result<SemanticInfo, SemanticError> {
    if response.request_id != request_id {
        return Err(SemanticError::InvalidResult(format!(
            "the response answers {} instead of {request_id}",
            response.request_id
        )));
    }
    match response.status {
        FmStatus::Success => Ok(response.semantic.unwrap_or_default().into()),
        FmStatus::Cancelled => Err(SemanticError::Cancelled),
        FmStatus::Error => match response.error {
            Some(error) => Err(error_from(error)),
            None => Err(SemanticError::InvalidResult(
                "error response without an error payload".to_string(),
            )),
        },
        FmStatus::Unknown => Err(SemanticError::InvalidResult(
            "unknown response status".to_string(),
        )),
    }
}

/// Classify one error payload (original spec §12).
fn error_from(error: FmErrorInfo) -> SemanticError {
    let detail = error.detail.or(error.reason.clone()).unwrap_or_default();
    match error.kind {
        ErrorKind::Unavailable => SemanticError::Unavailable {
            reason: error.reason.unwrap_or_else(|| "unknown".to_string()),
        },
        ErrorKind::ModelInitFailed => SemanticError::ModelInitFailed(detail),
        ErrorKind::InferenceFailed => SemanticError::InferenceFailed(detail),
        ErrorKind::InvalidOutput => SemanticError::InvalidResult(detail),
        ErrorKind::Cancelled => SemanticError::Cancelled,
        ErrorKind::Internal => SemanticError::Bridge(detail),
        ErrorKind::Unknown => SemanticError::InvalidResult(format!("unknown error kind: {detail}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::file_context::{CaptureTime, ImageMetadata, TargetFile};
    use std::path::PathBuf;

    fn context(filename: &str) -> NormalizedFileContext {
        NormalizedFileContext {
            target: TargetFile {
                path: PathBuf::from(format!("/tmp/{filename}")),
                filename: filename.to_string(),
                extension: filename.rsplit_once('.').map(|(_, ext)| ext.to_string()),
                size_bytes: 12,
                modified_at: Some("2026-10-14T00:00:00Z".to_string()),
            },
            parent_directory: "/tmp".to_string(),
            size_bytes: 12,
            creation_date: Some("2026-10-13T00:00:00Z".to_string()),
            modification_date: None,
            semantic_signals: vec!["F:proposal".to_string()],
            ..NormalizedFileContext::default()
        }
    }

    #[test]
    fn the_provider_defaults_to_english_and_accepts_an_override() {
        use super::super::fake::FakeFmBridge;
        use crate::semantic::contract::SemanticInfo;

        assert_eq!(DEFAULT_FM_LANGUAGE, "en");
        let plain =
            FoundationModelsProvider::with_bridge(FakeFmBridge::answering(SemanticInfo::default()));
        assert_eq!(plain.language, "en");
        let request = build_request(&context("proposal.pdf"), "rename-0001", &plain.language);
        assert_eq!(request.language, "en");
        let japanese =
            FoundationModelsProvider::with_bridge(FakeFmBridge::answering(SemanticInfo::default()))
                .with_language("ja");
        assert_eq!(japanese.language, "ja");
        let request = build_request(&context("proposal.pdf"), "rename-0002", &japanese.language);
        assert_eq!(request.language, "ja");
    }

    #[test]
    fn build_request_maps_the_observation() {
        let request = build_request(&context("proposal.pdf"), "rename-0001", "ja");
        assert_eq!(request.request_id, "rename-0001");
        assert_eq!(request.target_type, TargetKind::Document);
        assert_eq!(request.input.filename, "proposal.pdf");
        assert_eq!(request.input.extension.as_deref(), Some("pdf"));
        assert_eq!(request.input.size_bytes, 12);
        assert_eq!(request.input.parent_directory, "/tmp");
        assert_eq!(
            request.input.created_at.as_deref(),
            Some("2026-10-13T00:00:00Z")
        );
        // modification_date wins over the target timestamp when present.
        assert_eq!(
            request.input.modified_at.as_deref(),
            Some("2026-10-14T00:00:00Z")
        );
        assert_eq!(request.analysis, AnalysisFlags::default());
        assert!(request.input.image.is_none());
    }

    #[test]
    fn build_request_recognizes_images_and_bounds_the_fields() {
        let mut image = context("IMG_1234.HEIC");
        image.image_metadata = Some(ImageMetadata {
            capture_time: Some(CaptureTime {
                value: "2026-10-01T10:00:00".to_string(),
                timezone: None,
                source: "exif".to_string(),
                timezone_known: false,
            }),
            camera: crate::models::file_context::CameraInfo {
                make: Some("Apple".to_string()),
                model: Some("iPhone 17".to_string()),
                source: None,
            },
            ..ImageMetadata::default()
        });
        let request = build_request(&image, "rename-0002", "ja");
        assert_eq!(request.target_type, TargetKind::Image);
        let facts = request.input.image.expect("image facts");
        assert_eq!(facts.capture_time.as_deref(), Some("2026-10-01T10:00:00"));
        assert_eq!(facts.camera.as_deref(), Some("Apple iPhone 17"));

        let long = "あ".repeat(600);
        let request = build_request(&context(&long), "rename-0003", "ja");
        assert_eq!(request.input.filename.chars().count(), MAX_FILENAME_CHARS);
    }

    #[test]
    fn interpret_maps_success_and_missing_fields() {
        let response = FmResponse {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-0001".to_string(),
            status: FmStatus::Success,
            semantic: Some(contract::SemanticFields {
                title: Some("海辺の夕日".to_string()),
                category: None,
                project: Some("   ".to_string()),
            }),
            error: None,
        };
        let info = interpret(response, "rename-0001").expect("success maps");
        assert_eq!(info.title.as_deref(), Some("海辺の夕日"));
        assert_eq!(info.category, None);
        assert_eq!(info.project, None);
        assert!(!info.is_empty());
    }

    #[test]
    fn interpret_maps_every_error_kind() {
        let error = |kind: ErrorKind, reason: Option<&str>| FmResponse {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-0001".to_string(),
            status: FmStatus::Error,
            semantic: None,
            error: Some(FmErrorInfo {
                kind,
                reason: reason.map(str::to_string),
                detail: None,
            }),
        };
        assert_eq!(
            interpret(
                error(ErrorKind::Unavailable, Some("appleIntelligenceNotEnabled")),
                "rename-0001"
            ),
            Err(SemanticError::Unavailable {
                reason: "appleIntelligenceNotEnabled".to_string()
            })
        );
        assert!(matches!(
            interpret(error(ErrorKind::ModelInitFailed, None), "rename-0001"),
            Err(SemanticError::ModelInitFailed(_))
        ));
        assert!(matches!(
            interpret(error(ErrorKind::InferenceFailed, None), "rename-0001"),
            Err(SemanticError::InferenceFailed(_))
        ));
        assert!(matches!(
            interpret(error(ErrorKind::InvalidOutput, None), "rename-0001"),
            Err(SemanticError::InvalidResult(_))
        ));
        assert_eq!(
            interpret(error(ErrorKind::Cancelled, None), "rename-0001"),
            Err(SemanticError::Cancelled)
        );
        assert!(matches!(
            interpret(error(ErrorKind::Internal, None), "rename-0001"),
            Err(SemanticError::Bridge(_))
        ));
        assert!(matches!(
            interpret(error(ErrorKind::Unknown, None), "rename-0001"),
            Err(SemanticError::InvalidResult(_))
        ));
    }

    #[test]
    fn interpret_rejects_a_mismatched_request_id_and_an_unknown_status() {
        let mismatched = FmResponse {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-9999".to_string(),
            status: FmStatus::Success,
            semantic: Some(contract::SemanticFields::default()),
            error: None,
        };
        assert!(matches!(
            interpret(mismatched, "rename-0001"),
            Err(SemanticError::InvalidResult(_))
        ));
        let unknown = FmResponse {
            schema_version: SCHEMA_VERSION.to_string(),
            request_id: "rename-0001".to_string(),
            status: FmStatus::Unknown,
            semantic: None,
            error: None,
        };
        assert!(matches!(
            interpret(unknown, "rename-0001"),
            Err(SemanticError::InvalidResult(_))
        ));
    }

    #[test]
    fn errors_are_classified_for_skipping() {
        assert!(SemanticError::Timeout.is_skippable());
        assert!(SemanticError::Cancelled.is_skippable());
        assert!(SemanticError::InvalidResult("x".to_string()).is_skippable());
        assert!(SemanticError::InferenceFailed("x".to_string()).is_skippable());
        assert!(
            !SemanticError::Unavailable {
                reason: "x".to_string()
            }
            .is_skippable()
        );
        assert!(!SemanticError::ModelInitFailed("x".to_string()).is_skippable());
        assert!(!SemanticError::Bridge("x".to_string()).is_skippable());
        assert!(
            SemanticError::Unavailable {
                reason: "x".to_string()
            }
            .is_unavailable()
        );
    }
}

//! Jev integration contract and the real HTTP adapter.
//!
//! `JevIntegration` is the boundary between the decision pipeline and a Jev
//! implementation:
//!
//! - `MockJevIntegration` (tests; returns configured data verbatim)
//! - `RealJevIntegration` (external Jev API over HTTP)
//!
//! The contract is deliberately domain-shaped: implementations return a
//! `JevResponse` (a selection plus probabilities). They never validate,
//! score, or touch the filesystem. The `models::decision` domain owns:
//!
//! - candidate membership validation (`DecisionContext::validate_decision`)
//! - probability validation (`validate_probabilities`)
//! - confidence/margin calculation (`confidence_and_margin`)
//! - decision construction (`JevDecision::from_selection`)

use crate::models::decision::{DecisionCandidate, DecisionContext};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Errors from the Jev boundary.
///
/// `NONE` is a *decision* ("no destination") and is never expressed as an
/// error. An error means the decision could not be obtained.
#[derive(Debug, Clone)]
pub enum JevError {
    /// No valid response was obtained (network, timeout, DNS, ...).
    TransportError(String),
    /// The API rejected our credentials (HTTP 401/403).
    Authentication(String),
    /// The API rejected the request (other HTTP 4xx).
    ApiError(String),
    /// The API failed (HTTP 5xx).
    ServerError(String),
    /// The response did not match the expected schema.
    InvalidResponse(String),
    /// The adapter is not configured (e.g. missing environment variables).
    Configuration(String),
}

impl std::fmt::Display for JevError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            JevError::TransportError(msg) => write!(f, "TransportError({msg})"),
            JevError::Authentication(msg) => write!(f, "Authentication({msg})"),
            JevError::ApiError(msg) => write!(f, "ApiError({msg})"),
            JevError::ServerError(msg) => write!(f, "ServerError({msg})"),
            JevError::InvalidResponse(msg) => write!(f, "InvalidResponse({msg})"),
            JevError::Configuration(msg) => write!(f, "Configuration({msg})"),
        }
    }
}

/// Response from a Jev implementation.
///
/// `destination_id` is an untrusted string: it is validated against the
/// offered candidates by the domain before it can become a decision.
#[derive(Debug, Clone)]
pub struct JevResponse {
    pub destination_id: Option<String>,
    pub probabilities: HashMap<String, f64>,
    #[allow(dead_code)]
    pub confidence: f64,
    #[allow(dead_code)]
    pub margin: f64,
}

impl Default for JevResponse {
    fn default() -> Self {
        Self {
            destination_id: Some("NONE".to_string()),
            probabilities: HashMap::new(),
            confidence: 0.0,
            margin: 0.0,
        }
    }
}

/// The Jev decision boundary.
pub trait JevIntegration: Send + Sync {
    /// Invoke the Jev decision service and return a decision
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>>;
}

/// Environment variable holding the TypeSafe API base URL
/// (e.g. `https://api.typesafe.ai`).
pub const JEV_API_URL_ENV: &str = "JEV_API_URL";

/// Legacy alias for [`JEV_API_URL_ENV`].
pub const JEV_ENDPOINT_ENV: &str = "JEV_API_BASE_URL";

/// Environment variable holding the Jev API key (never hard-coded).
pub const JEV_API_KEY_ENV: &str = "JEV_API_KEY";

/// Environment variable overriding the model name sent to the API.
pub const JEV_MODEL_ENV: &str = "JEV_MODEL";

/// Model alias used when `JEV_MODEL` is not set (see `GET /v1/models`).
pub const DEFAULT_JEV_MODEL: &str = "jev-latest";

/// The TypeSafe decision endpoint path.
const SYSTEMONE_PATH: &str = "/v1/systemone";

/// Name of the choice question asked in every request. The answer is read
/// back from `answers.<name>`.
const DESTINATION_QUESTION: &str = "destination";

/// Instructions sent with the choice question.
const DESTINATION_INSTRUCTIONS: &str = "Select the existing directory that best fits this file. Select NONE if none of the candidates is a good fit.";

/// Description of the mandatory `NONE` choice.
const NONE_DESCRIPTION: &str = "No suitable destination among the offered candidates.";

/// HTTP adapter for the TypeSafe (`/v1/systemone`) API.
///
/// Scope: map `DecisionContext` to the API request schema, perform the HTTP
/// call, and map the API response back to `JevResponse`. Validation, scoring
/// and decision construction stay in the domain.
///
/// The base URL is injected (tests use a local mock server; production reads
/// `JEV_API_URL`), and the API key is read from `JEV_API_KEY`.
pub struct RealJevIntegration {
    endpoint: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::Client,
    send_capture_time_to_jev: bool,
    send_gps_to_jev: bool,
}

/// Request timeout for the Jev API.
const JEV_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

impl RealJevIntegration {
    /// Create an adapter for the given base URL.
    ///
    /// `endpoint` is the API base URL; the decision endpoint path
    /// (`/v1/systemone`) is appended. Tests inject a local mock server URL;
    /// production uses [`RealJevIntegration::from_env`].
    pub fn new(endpoint: impl Into<String>, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(JEV_REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            endpoint: endpoint.into(),
            api_key,
            model: DEFAULT_JEV_MODEL.to_string(),
            client,
            send_capture_time_to_jev: crate::config::defaults::DEFAULT_SEND_CAPTURE_TIME_TO_JEV,
            send_gps_to_jev: crate::config::defaults::DEFAULT_SEND_GPS_TO_JEV,
        }
    }

    /// Override the model alias sent to the API.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override image privacy gating (tests and explicit wiring).
    ///
    /// GPS stays disabled unless explicitly enabled.
    pub fn with_privacy(mut self, send_capture_time: bool, send_gps: bool) -> Self {
        self.send_capture_time_to_jev = send_capture_time;
        self.send_gps_to_jev = send_gps;
        self
    }

    /// Create an adapter from resolved configuration (preferred entry point).
    ///
    /// Resolution (CLI > env > user TOML/Keychain > defaults) happens in
    /// `crate::config`; the adapter only receives the outcome, so it never
    /// searches the environment or the working directory itself.
    pub fn from_resolved(config: &crate::config::JevConfig) -> Self {
        Self::new(
            config.endpoint.clone(),
            config.api_key.as_ref().map(|key| key.expose().to_string()),
        )
        .with_model(config.model.clone())
    }

    /// Create an adapter from resolved configuration (preferred entry point).
    ///
    /// Privacy flags come from `[metadata.privacy]`; GPS stays disabled
    /// unless the user explicitly enables it.
    pub fn from_resolved_full(resolved: &crate::config::ResolvedConfig) -> Self {
        Self::from_resolved(&resolved.jev).with_privacy(
            resolved.metadata.send_capture_time_to_jev,
            resolved.metadata.send_gps_to_jev,
        )
    }

    /// Create an adapter from the environment (`JEV_API_URL`,
    /// `JEV_API_KEY`, optional `JEV_MODEL`). Credentials are never read from
    /// source or config files.
    pub fn from_env() -> Result<Self, JevError> {
        let endpoint = std::env::var(JEV_API_URL_ENV)
            .or_else(|_| std::env::var(JEV_ENDPOINT_ENV))
            .map_err(|_| {
                JevError::Configuration(format!(
                    "{JEV_API_URL_ENV} is not set (environment or .env)"
                ))
            })?;
        let api_key = std::env::var(JEV_API_KEY_ENV).ok();
        let mut adapter = Self::new(endpoint, api_key);
        if let Ok(model) = std::env::var(JEV_MODEL_ENV) {
            adapter = adapter.with_model(model);
        }
        Ok(adapter)
    }

    /// The configured base URL (used for diagnostics).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The configured API key presence (never the key itself).
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// The full decision endpoint URL.
    ///
    /// `JEV_API_URL` may be configured either as the API base URL
    /// (`https://api.typesafe.ai`) or as the full endpoint
    /// (`https://api.typesafe.ai/v1/systemone`); both are accepted without
    /// duplicating the path.
    fn request_url(&self) -> String {
        let base = self.endpoint.trim_end_matches('/');

        if base.ends_with("/systemone") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/systemone")
        } else {
            format!("{base}{SYSTEMONE_PATH}")
        }
    }
}

impl JevIntegration for RealJevIntegration {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        // The request DTO owns its data, so the returned future borrows only
        // `self`; the JevIntegration contract stays unchanged.
        let request = SystemOneRequest::from_context_with_privacy(
            context,
            &self.model,
            self.send_capture_time_to_jev,
            self.send_gps_to_jev,
        );
        let url = self.request_url();

        Box::pin(async move {
            let mut http_request = self.client.post(&url).json(&request);
            if let Some(api_key) = &self.api_key {
                http_request = http_request.bearer_auth(api_key);
            }

            let response = http_request
                .send()
                .await
                .map_err(|error| JevError::TransportError(error.to_string()))?;

            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|error| JevError::TransportError(error.to_string()))?;

            if !status.is_success() {
                return Err(map_http_error(status.as_u16(), &body));
            }

            let api_response: SystemOneResponse = serde_json::from_str(&body).map_err(|error| {
                JevError::InvalidResponse(format!("{error}; body: {}", body_snippet(&body)))
            })?;

            api_response.into_jev_response()
        })
    }
}

// ---------------------------------------------------------------------------
// TypeSafe /v1/systemone DTOs (adapter-internal; the domain model never leaks
// into the API schema and the API schema never leaks into the domain model)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct SystemOneRequest {
    state: StateDto,
    model: String,
    questions: QuestionsDto,
}

/// The content all questions refer to: the target file plus its normalized
/// semantic signals (verbatim; the adapter never re-derives them).
#[derive(Debug, Serialize)]
struct StateDto {
    target: TargetDto,
    semantic_signals: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TargetDto {
    filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    file_type: Option<String>,
    /// Capture time (`YYYY-MM-DDTHH:MM:SS[ zone]`); sent only when the
    /// privacy flags allow it. Never a guessed value.
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_date: Option<String>,
    /// Camera make/model joined (`Apple iPhone`); not location-sensitive.
    #[serde(skip_serializing_if = "Option::is_none")]
    camera: Option<String>,
    /// GPS coordinates; present only when `send_gps_to_jev` is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<LocationDto>,
    /// Pixel dimensions, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<DimensionsDto>,
}

/// GPS coordinates inside `state.target` (v3.2; gated by privacy).
#[derive(Debug, Serialize)]
struct LocationDto {
    latitude: f64,
    longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    altitude_m: Option<f64>,
}

/// Pixel dimensions inside `state.target` (v3.2).
#[derive(Debug, Serialize)]
struct DimensionsDto {
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
struct QuestionsDto {
    destination: ChoiceQuestionDto,
}

#[derive(Debug, Serialize)]
struct ChoiceQuestionDto {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: String,
    criteria: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct SystemOneResponse {
    answers: HashMap<String, ChoiceAnswerDto>,
}

#[derive(Debug, Deserialize)]
struct ChoiceAnswerDto {
    #[serde(rename = "type")]
    kind: String,
    choice: Option<String>,
    #[serde(default)]
    probabilities: HashMap<String, f64>,
    /// API-side confidence: informational only. It is deliberately NOT copied
    /// into the domain decision (kaseifu computes its own confidence/margin).
    #[serde(default)]
    #[allow(dead_code)]
    confidence: Option<f64>,
}

impl SystemOneRequest {
    /// Build the request with explicit image privacy gating (v3.2).
    ///
    /// Mapping stays on real `/v1/systemone` fields only: image context
    /// extends the free-form `state.target` object and the verbatim
    /// `state.semantic_signals` list. No new top-level API fields are
    /// introduced and no Rust-side fit score is computed.
    fn from_context_with_privacy(
        context: &DecisionContext,
        model: &str,
        send_capture_time: bool,
        send_gps: bool,
    ) -> Self {
        let target = context.target();
        let image = context.image_metadata();

        let mut criteria: BTreeMap<String, String> = context
            .candidates()
            .iter()
            .map(|candidate| (candidate.id.clone(), describe_candidate(candidate)))
            .collect();
        criteria.insert("NONE".to_string(), NONE_DESCRIPTION.to_string());

        Self {
            state: StateDto {
                target: TargetDto {
                    filename: target.filename.clone(),
                    extension: target.extension.clone(),
                    file_type: target.file_type.clone(),
                    capture_date: capture_date_for(image, send_capture_time),
                    camera: camera_for(image),
                    location: location_for(image, send_gps),
                    dimensions: dimensions_for(image),
                },
                semantic_signals: filtered_signals(context, send_capture_time, send_gps),
            },
            model: model.to_string(),
            questions: QuestionsDto {
                destination: ChoiceQuestionDto {
                    kind: "choice",
                    instructions: DESTINATION_INSTRUCTIONS.to_string(),
                    criteria,
                },
            },
        }
    }
}

/// Description of when a candidate applies (the API's `criteria` values).
/// Uses only data the domain already provides.
fn describe_candidate(candidate: &DecisionCandidate) -> String {
    let mut description = format!(
        "Directory: {} — {} related file(s)",
        candidate.path, candidate.evidence_count
    );
    if !candidate.reasons.is_empty() {
        description.push_str(&format!("; evidence: {}", candidate.reasons.join("; ")));
    }
    description
}

/// Capture date for `state.target` (v3.2; gated by privacy).
fn capture_date_for(
    image: Option<&crate::models::file_context::ImageMetadata>,
    send_capture_time: bool,
) -> Option<String> {
    if !send_capture_time {
        return None;
    }
    image?
        .capture_time
        .as_ref()
        .map(|capture| match &capture.timezone {
            Some(zone) => format!("{} {}", capture.value, zone),
            None => capture.value.clone(),
        })
}

/// Camera summary for `state.target` (v3.2; not location-sensitive).
fn camera_for(image: Option<&crate::models::file_context::ImageMetadata>) -> Option<String> {
    let camera = &image?.camera;
    match (&camera.make, &camera.model) {
        (Some(make), Some(model)) if make != model => Some(format!("{make} {model}")),
        (Some(make), _) => Some(make.clone()),
        (None, Some(model)) => Some(model.clone()),
        (None, None) => None,
    }
}

/// GPS coordinates for `state.target` (v3.2; sent only when enabled).
fn location_for(
    image: Option<&crate::models::file_context::ImageMetadata>,
    send_gps: bool,
) -> Option<LocationDto> {
    if !send_gps {
        return None;
    }
    let location = image?.location.as_ref()?;
    Some(LocationDto {
        latitude: location.latitude,
        longitude: location.longitude,
        altitude_m: location.altitude_m,
    })
}

/// Pixel dimensions for `state.target` (v3.2).
fn dimensions_for(
    image: Option<&crate::models::file_context::ImageMetadata>,
) -> Option<DimensionsDto> {
    let dims = image?.dimensions.as_ref()?;
    Some(DimensionsDto {
        width: dims.width,
        height: dims.height,
    })
}

/// Semantic signals with image presence markers filtered by privacy.
///
/// A withheld field leaves no trace: neither its value nor an
/// "available" marker is forwarded when its flag is off.
fn filtered_signals(
    context: &DecisionContext,
    send_capture_time: bool,
    send_gps: bool,
) -> Vec<String> {
    context
        .semantic_signals()
        .iter()
        .filter(|signal| keep_signal(signal, send_capture_time, send_gps))
        .cloned()
        .collect()
}

fn keep_signal(signal: &str, send_capture_time: bool, send_gps: bool) -> bool {
    if !send_capture_time && signal == "M:Capture date available" {
        return false;
    }
    if !send_gps && signal == "M:GPS available" {
        return false;
    }
    true
}

impl SystemOneResponse {
    fn into_jev_response(self) -> Result<JevResponse, JevError> {
        let answer = self.answers.get(DESTINATION_QUESTION).ok_or_else(|| {
            JevError::InvalidResponse(format!(
                "response is missing the '{DESTINATION_QUESTION}' answer"
            ))
        })?;

        if answer.kind != "choice" {
            return Err(JevError::InvalidResponse(format!(
                "answer '{DESTINATION_QUESTION}' has unexpected type '{}'",
                answer.kind
            )));
        }

        let destination_id = answer.choice.clone().ok_or_else(|| {
            JevError::InvalidResponse(format!(
                "answer '{DESTINATION_QUESTION}' is missing 'choice'"
            ))
        })?;

        // The selection is passed through verbatim; candidate membership is
        // validated by the domain, never by the adapter.
        Ok(JevResponse {
            destination_id: Some(destination_id),
            probabilities: answer.probabilities.clone(),
            confidence: 0.0,
            margin: 0.0,
        })
    }
}

/// Map an HTTP failure to the contract error type. A failed decision is
/// always an `Err`; it is never converted into a `NONE` decision.
fn map_http_error(status: u16, body: &str) -> JevError {
    let detail = format!("HTTP {status}: {}", body_snippet(body));

    match status {
        401 | 403 => JevError::Authentication(detail),
        400..=499 => JevError::ApiError(detail),
        500..=599 => JevError::ServerError(detail),
        _ => JevError::ApiError(detail),
    }
}

fn body_snippet(body: &str) -> String {
    body.chars().take(200).collect()
}

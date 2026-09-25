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
        }
    }

    /// Override the model alias sent to the API.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
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
        let request = SystemOneRequest::from_context(context, &self.model);
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
    fn from_context(context: &DecisionContext, model: &str) -> Self {
        let target = context.target();

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
                },
                semantic_signals: context.semantic_signals().to_vec(),
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

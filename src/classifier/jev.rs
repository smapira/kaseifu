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

use crate::models::decision::DecisionContext;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// Environment variable holding the full Jev decision endpoint URL.
pub const JEV_ENDPOINT_ENV: &str = "JEV_API_BASE_URL";

/// Environment variable holding the Jev API key (never hard-coded).
pub const JEV_API_KEY_ENV: &str = "JEV_API_KEY";

/// HTTP adapter for the external Jev API.
///
/// Scope: map `DecisionContext` to the API request schema, perform the HTTP
/// call, and map the API response back to `JevResponse`. Validation, scoring
/// and decision construction stay in the domain.
///
/// The endpoint is injected (tests use a local mock server; production reads
/// `JEV_API_BASE_URL`), and the API key is read from `JEV_API_KEY`.
pub struct RealJevIntegration {
    endpoint: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

/// Request timeout for the Jev API.
const JEV_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

impl RealJevIntegration {
    /// Create an adapter for the given endpoint.
    ///
    /// `endpoint` is the full decision endpoint URL. Tests inject a local
    /// mock server URL; production uses [`RealJevIntegration::from_env`].
    pub fn new(endpoint: impl Into<String>, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(JEV_REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            endpoint: endpoint.into(),
            api_key,
            client,
        }
    }

    /// Create an adapter from the environment (`JEV_API_BASE_URL`,
    /// `JEV_API_KEY`). Credentials are never read from source or config.
    pub fn from_env() -> Result<Self, JevError> {
        let endpoint = std::env::var(JEV_ENDPOINT_ENV)
            .map_err(|_| JevError::Configuration(format!("{JEV_ENDPOINT_ENV} is not set")))?;
        let api_key = std::env::var(JEV_API_KEY_ENV).ok();
        Ok(Self::new(endpoint, api_key))
    }

    /// The configured endpoint (used for diagnostics).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The configured API key presence (never the key itself).
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }
}

impl JevIntegration for RealJevIntegration {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        // The request DTO owns its data, so the returned future borrows only
        // `self`; the JevIntegration contract stays unchanged.
        let request = JevApiRequest::from_context(context);

        Box::pin(async move {
            let mut http_request = self.client.post(&self.endpoint).json(&request);
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

            let api_response: JevApiResponse = serde_json::from_str(&body)
                .map_err(|error| JevError::InvalidResponse(error.to_string()))?;

            api_response.into_jev_response()
        })
    }
}

// ---------------------------------------------------------------------------
// API DTOs (adapter-internal; the domain model never leaks into the API
// schema and the API schema never leaks into the domain model)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JevApiRequest {
    target: JevApiTarget,
    semantic_signals: Vec<String>,
    candidates: Vec<JevApiCandidate>,
}

#[derive(Debug, Serialize)]
struct JevApiTarget {
    filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    file_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct JevApiCandidate {
    id: String,
    related_files: usize,
}

#[derive(Debug, Deserialize)]
struct JevApiResponse {
    /// The chosen option. Both documented spellings are accepted.
    #[serde(alias = "choice")]
    destination_id: Option<String>,
    #[serde(default)]
    probabilities: HashMap<String, f64>,
    /// API-side confidence: informational only. It is deliberately NOT copied
    /// into the domain decision (kaseifu computes its own confidence/margin).
    #[serde(default)]
    #[allow(dead_code)]
    confidence: Option<f64>,
}

impl JevApiRequest {
    fn from_context(context: &DecisionContext) -> Self {
        let target = context.target();

        Self {
            target: JevApiTarget {
                filename: target.filename.clone(),
                extension: target.extension.clone(),
                file_type: target.file_type.clone(),
            },
            semantic_signals: context.semantic_signals().to_vec(),
            candidates: context
                .candidates()
                .iter()
                .map(|candidate| JevApiCandidate {
                    id: candidate.id.clone(),
                    related_files: candidate.evidence_count,
                })
                .collect(),
        }
    }
}

impl JevApiResponse {
    fn into_jev_response(self) -> Result<JevResponse, JevError> {
        let destination_id = self.destination_id.ok_or_else(|| {
            JevError::InvalidResponse("response is missing 'choice'/'destination_id'".to_string())
        })?;

        // The selection is passed through verbatim; candidate membership is
        // validated by the domain, never by the adapter.
        Ok(JevResponse {
            destination_id: Some(destination_id),
            probabilities: self.probabilities,
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

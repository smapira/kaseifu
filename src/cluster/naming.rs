//! Naming a group (issue #26).
//!
//! Membership is decided in [`super::grouping`]; this module only chooses the
//! *word* a group is filed under. The model is asked with the same
//! `/v1/systemone` envelope the classifier uses, but a different question: the
//! API offers `noul` / `choice` / `score` / `box` questions and no free-text
//! answer, so the names offered to the model are generated deterministically in
//! Rust and the model selects one per group. Paths never come from the model:
//! a returned name must be one of the offered candidates, and the directory is
//! built from that name by the caller.
//!
//! Without a key, without a network, or with any unexpected answer the
//! deterministic name (the group's first candidate) is used, so the command
//! always produces a plan.

use super::grouping::Group;
use crate::classifier::jev::systemone_url;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// Where a group's name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSource {
    Jev,
    Fallback,
}

impl NameSource {
    /// The report word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jev => "jev",
            Self::Fallback => "fallback",
        }
    }
}

/// One group's name and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedGroup {
    pub name: String,
    pub source: NameSource,
}

/// Why a model naming pass could not be used.
#[derive(Debug, Clone, PartialEq)]
pub struct NameFailure {
    /// A short, user-facing reason (`no Jev key`, `HTTP 401: ...`).
    pub reason: String,
}

/// Choose one name per group.
#[async_trait::async_trait]
pub trait GroupNamer: Send + Sync {
    /// One name per group, in order. `Err` means no name came from the model
    /// and every group must use its fallback.
    async fn name_groups(&self, groups: &[Group]) -> Result<Vec<NamedGroup>, NameFailure>;
}

/// The deterministic namer: the first candidate of every group.
pub struct TypeNamer;

#[async_trait::async_trait]
impl GroupNamer for TypeNamer {
    async fn name_groups(&self, groups: &[Group]) -> Result<Vec<NamedGroup>, NameFailure> {
        Ok(groups
            .iter()
            .map(|group| NamedGroup {
                name: group.candidate_names().first().cloned().unwrap_or_default(),
                source: NameSource::Fallback,
            })
            .collect())
    }
}

/// The model-backed namer (one HTTP request for every group).
pub struct JevNamer {
    endpoint: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

/// Timeout for the naming request (same order as the decision call).
const NAME_TIMEOUT: Duration = Duration::from_secs(15);

impl JevNamer {
    /// Build a namer from resolved configuration, when a key is present.
    pub fn from_resolved(config: &crate::config::ResolvedConfig) -> Option<Self> {
        let api_key = config.jev.api_key.as_ref()?.expose().to_string();
        let client = reqwest::Client::builder()
            .timeout(NAME_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Some(Self {
            endpoint: config.jev.endpoint.clone(),
            api_key,
            model: config.jev.model.clone(),
            client,
        })
    }

    /// The request body for these groups.
    fn request(&self, groups: &[Group]) -> NameRequest {
        let mut questions = BTreeMap::new();
        let mut state_groups = Vec::with_capacity(groups.len());
        for (index, group) in groups.iter().enumerate() {
            let candidates = group.candidate_names();
            let number = index + 1;
            state_groups.push(NameGroupDto {
                index: number,
                kind: group.category.as_str(),
                file_count: group.members.len(),
                bytes: group.bytes(),
                tokens: group.shared_tokens.clone(),
                examples: group
                    .members
                    .iter()
                    .take(3)
                    .map(|member| member.name.clone())
                    .collect(),
            });
            let criteria = candidates
                .iter()
                .map(|name| (name.clone(), describe(group, name)))
                .collect();
            questions.insert(
                format!("group_{number}"),
                ChoiceQuestionDto {
                    kind: "choice",
                    instructions: "Pick the folder name that best describes this group of files. Choose exactly one of the offered names.".to_string(),
                    criteria,
                },
            );
        }
        NameRequest {
            model: self.model.clone(),
            state: NameStateDto {
                task: "Name each group of files with one short folder name.",
                groups: state_groups,
            },
            questions,
        }
    }
}

#[async_trait::async_trait]
impl GroupNamer for JevNamer {
    async fn name_groups(&self, groups: &[Group]) -> Result<Vec<NamedGroup>, NameFailure> {
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        let request = self.request(groups);
        let response = self
            .client
            .post(systemone_url(&self.endpoint))
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|error| NameFailure {
                reason: format!("transport: {error}"),
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| NameFailure {
            reason: format!("transport: {error}"),
        })?;
        if !status.is_success() {
            return Err(NameFailure {
                reason: format!(
                    "HTTP {}: {}",
                    status.as_u16(),
                    body.chars().take(160).collect::<String>()
                ),
            });
        }
        let parsed: NameResponse = serde_json::from_str(&body).map_err(|error| NameFailure {
            reason: format!("unexpected response: {error}"),
        })?;
        let answers = parsed.answers;
        if answers.is_empty() {
            return Err(NameFailure {
                reason: "the response carried no answers".to_string(),
            });
        }

        // An individual answer that is missing or not one of the offered names
        // falls back for that group only: one bad answer must not discard the
        // names that were usable.
        Ok(groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let candidates = group.candidate_names();
                let choice = answers
                    .get(&format!("group_{}", index + 1))
                    .and_then(|answer| answer.choice.clone())
                    .filter(|choice| candidates.contains(choice));
                match choice {
                    Some(name) => NamedGroup {
                        name,
                        source: NameSource::Jev,
                    },
                    None => NamedGroup {
                        name: candidates.first().cloned().unwrap_or_default(),
                        source: NameSource::Fallback,
                    },
                }
            })
            .collect())
    }
}

/// The description shown next to one candidate name.
fn describe(group: &Group, name: &str) -> String {
    let tokens = if group.shared_tokens.is_empty() {
        String::new()
    } else {
        format!("; shared name tokens: {}", group.shared_tokens.join(", "))
    };
    format!(
        "{} files of type {} filed as `{}`{}",
        group.members.len(),
        group.category.as_str(),
        name,
        tokens
    )
}

#[derive(Debug, Serialize)]
struct NameRequest {
    model: String,
    state: NameStateDto,
    questions: BTreeMap<String, ChoiceQuestionDto>,
}

#[derive(Debug, Serialize)]
struct NameStateDto {
    task: &'static str,
    groups: Vec<NameGroupDto>,
}

#[derive(Debug, Serialize)]
struct NameGroupDto {
    index: usize,
    kind: &'static str,
    file_count: usize,
    bytes: u64,
    tokens: Vec<String>,
    /// File names only, never full paths.
    examples: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ChoiceQuestionDto {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: String,
    criteria: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct NameResponse {
    #[serde(default)]
    answers: BTreeMap<String, ChoiceAnswerDto>,
}

#[derive(Debug, Deserialize)]
struct ChoiceAnswerDto {
    #[serde(default)]
    choice: Option<String>,
}

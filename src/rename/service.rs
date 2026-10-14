//! Rename planner and executor (original spec sections 8.1-8.3, 9 and 10).
//!
//! Planning is side-effect free: observe, extract bounded document signals,
//! ask the semantic provider, apply the naming rule and validate. Execution
//! re-verifies everything immediately before the rename (TOCTOU), records
//! the operation in the journal **before** mutating (the existing `move`
//! row, so `undo` works unchanged), then marks the result.

use std::path::{Path, PathBuf};

use crate::models::file_context::{
    NormalizedFileContext, PerceptionInfo, PerceptionLabel, VideoInfo,
};
use crate::semantic::contract::SemanticInfo;
use crate::semantic::perception::PerceptionError;
use crate::semantic::provider::{SemanticError, SemanticProvider};
use crate::semantic::video::VideoPerception;
use crate::storage::history::OperationHistory;

use super::document::{
    DocumentDate, DocumentKind, MAX_DOCUMENT_BYTES, classify_document, contains_prompt_injection,
    extract_signals, read_bounded, scan_document_date,
};
use super::image::{classify_image, is_image};
use super::rules::{
    PresetDef, ResolvedDate, RuleInputs, bound_stem, find_preset, is_date_format, resolve_date,
    resolve_image_date, resolve_template, resolve_video_date, strip_redundant_extension,
    underscore_whitespace,
};
use super::store::RenameStore;
use super::validate::{
    FileSnapshot, NameProblem, check_filename, is_symlink, recheck_before_execute, sibling_path,
    snapshot,
};
use super::video::{classify_video, is_video};

/// How the naming rule was chosen.
#[derive(Debug, Clone)]
pub enum RuleSelection {
    /// `--template` won over everything.
    CustomTemplate(String),
    /// `--pattern` id or slug.
    Pattern(PresetDef),
    /// `--preset NAME` stored template.
    Preset(String, String),
    /// Stored default (or [`DEFAULT_RULE_ID`] when unset).
    Default(String),
}

impl RuleSelection {
    /// Journal `candidate_id` and display label.
    pub fn label(&self) -> String {
        match self {
            Self::CustomTemplate(_) => "custom".to_string(),
            Self::Pattern(preset) => preset.id.to_string(),
            Self::Preset(name, _) => format!("preset:{name}"),
            Self::Default(reference) => reference.clone(),
        }
    }

    /// The template text to resolve (`None` for stored defaults).
    pub fn template(&self) -> Option<&str> {
        match self {
            Self::CustomTemplate(template) => Some(template),
            Self::Pattern(preset) => Some(preset.template),
            Self::Preset(_, template) => Some(template),
            Self::Default(_) => Option::None,
        }
    }
}

/// Bounded document text, an optional unambiguous calendar date, and whether
/// the text looks like a prompt-injection payload.
type DocumentSignals = (Vec<String>, Option<(i32, u32, u32)>, bool);

/// Caller-supplied overrides (mirrors the CLI flags).
#[derive(Debug, Clone, Default)]
pub struct RenameOptions {
    pub pattern: Option<String>,
    pub template: Option<String>,
    pub preset: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub project: Option<String>,
    pub seq: Option<u32>,
    pub date_format: Option<String>,
}

/// A validated rename proposal (no side effects yet).
#[derive(Debug, Clone)]
pub struct RenamePlan {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub current_name: String,
    pub proposal: String,
    pub rule_label: String,
    pub date_source: &'static str,
    pub snapshot: FileSnapshot,
    /// What the perception layer contributed (images and videos).
    pub perception: Option<PerceptionReport>,
    /// What AVFoundation measured (videos only).
    pub video: Option<VideoReport>,
}

/// What the video perception contributed to one plan (Phase 6).
///
/// The labels ride in [`PerceptionReport`] exactly like an image's, so the
/// report shape and the model path stay shared; this type carries what is
/// video-only: what the container reported and how many representative frames
/// were classified (original spec sections 5.3 and 11.4).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoReport {
    pub info: VideoInfo,
    pub frames: usize,
}

impl VideoReport {
    /// One bounded line for reports (metadata and the frame count).
    ///
    /// Only values the container actually reported are printed; nothing is
    /// invented for a missing one (original spec section 6.5).
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(duration) = self.info.duration_seconds {
            parts.push(format!("{duration:.1}s"));
        }
        if let (Some(width), Some(height)) = (self.info.width, self.info.height) {
            parts.push(format!("{width}x{height}"));
        }
        if let Some(rate) = self.info.frame_rate {
            parts.push(format!("{rate:.1} fps"));
        }
        parts.push(format!("frames {}", self.frames));
        let recorded = self
            .info
            .creation_date
            .as_deref()
            .filter(|value| !value.is_empty());
        if let Some(recorded) = recorded {
            parts.push(format!("recorded {recorded}"));
        }
        parts.join(", ")
    }
}

/// What the perception layer contributed to one plan (Phase 5b).
///
/// `Labels` with an empty vector means perception ran and the classifier named
/// nothing; `Unavailable` means it could not run at all. The two stay apart so
/// a report never claims a picture was understood when it was not. Videos
/// carry the same shape: the labels merged across their frames.
#[derive(Debug, Clone, PartialEq)]
pub enum PerceptionReport {
    Labels(Vec<PerceptionLabel>),
    Unavailable(String),
}

impl PerceptionReport {
    /// One bounded line for reports (the labels are ASCII identifiers).
    ///
    /// At most [`SHOWN_PERCEPTION_LABELS`] labels stay visible; the rest is
    /// summarised so a report line never grows with the classifier's output.
    pub fn summary(&self) -> String {
        match self {
            Self::Unavailable(reason) => format!("unavailable ({reason})"),
            Self::Labels(labels) if labels.is_empty() => "no labels".to_string(),
            Self::Labels(labels) => {
                let shown: Vec<String> = labels
                    .iter()
                    .take(SHOWN_PERCEPTION_LABELS)
                    .map(|entry| format!("{}({:.3})", entry.label, entry.confidence))
                    .collect();
                let hidden = labels.len().saturating_sub(shown.len());
                if hidden == 0 {
                    shown.join(" ")
                } else {
                    format!("{} (+{hidden} more)", shown.join(" "))
                }
            }
        }
    }

    /// The labels for the request (`None` when perception did not run).
    pub fn into_labels(self) -> Option<Vec<PerceptionLabel>> {
        match self {
            Self::Labels(labels) => Some(labels),
            Self::Unavailable(_) => None,
        }
    }
}

/// Labels shown in one report line before the remainder is summarised.
pub const SHOWN_PERCEPTION_LABELS: usize = 6;

/// The outcome of planning a rename.
#[derive(Debug, Clone)]
pub enum PlanOutcome {
    /// Ready to preview or execute.
    Ready(RenamePlan),
    /// Source and proposal are identical: nothing to do.
    Noop {
        current_name: String,
        rule_label: String,
    },
    /// A required value is missing or the name is unsafe: held with a reason.
    Held {
        current_name: String,
        rule_label: String,
        reason: String,
    },
}

/// Why planning or execution failed outright.
#[derive(Debug, Clone)]
pub enum RenameError {
    Unsupported(String),
    Unreadable(String),
    Semantic(String),
    ModelUnavailable(String),
    Store(String),
    History(String),
    Io(String),
    Precondition(NameProblem),
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(detail) => write!(f, "{detail}"),
            Self::Unreadable(detail) => write!(f, "{detail}"),
            Self::Semantic(detail) => write!(f, "{detail}"),
            Self::ModelUnavailable(detail) => write!(f, "model unavailable: {detail}"),
            Self::Store(detail) => write!(f, "preset store: {detail}"),
            Self::History(detail) => write!(f, "journal: {detail}"),
            Self::Io(detail) => write!(f, "{detail}"),
            Self::Precondition(problem) => write!(f, "{problem}"),
        }
    }
}

impl From<NameProblem> for RenameError {
    fn from(problem: NameProblem) -> Self {
        Self::Precondition(problem)
    }
}

/// Plan a rename: observe, extract, perceive, analyze, resolve, validate.
///
/// Reads the filesystem but changes nothing and writes no journal rows.
pub async fn plan_rename(
    provider: &impl SemanticProvider,
    store: &RenameStore,
    target: &Path,
    options: &RenameOptions,
) -> Result<PlanOutcome, RenameError> {
    if is_symlink(target) {
        return Err(RenameError::Precondition(NameProblem::Symlink));
    }
    if is_image(target) {
        return plan_image_rename(provider, store, target, options).await;
    }
    if is_video(target) {
        return plan_video_rename(provider, store, target, options).await;
    }
    plan_document_rename(provider, store, target, options).await
}

/// Plan an image rename: observe, select the rule, analyze, resolve, validate.
///
/// Images carry no text body, so no bounded read and no document-date scan
/// happen here: only the file name is checked for prompt-injection markers.
/// EXIF facts already ride along in the observed context (capture time feeds
/// `{date}` first, the camera feeds `{device}`). Changes nothing and writes
/// no journal rows.
async fn plan_image_rename(
    provider: &impl SemanticProvider,
    store: &RenameStore,
    target: &Path,
    options: &RenameOptions,
) -> Result<PlanOutcome, RenameError> {
    classify_image(target).map_err(|error| RenameError::Unsupported(error.to_string()))?;
    let mut context = observe(target)?;
    // Cheap checks first: rule errors and poisoned names never spend inference
    // (perception included).
    let selection = select_rule(store, options)?;
    if contains_prompt_injection(&context.target.filename) {
        return Ok(PlanOutcome::Held {
            current_name: context.target.filename.clone(),
            rule_label: selection.label(),
            reason: "probable prompt injection; proposal withheld".to_string(),
        });
    }
    // Perception before the model call: the labels ride in the same request
    // and the model turns them into a title (the naming rule stays here).
    let perception = perceive(provider, target);
    let labels = perception.clone().into_labels().unwrap_or_default();
    if !labels.is_empty() {
        context.perception = Some(PerceptionInfo {
            labels: labels.clone(),
        });
    }
    let semantic = analyze(provider, &context).await?;
    if let Some(reason) = label_echo(&semantic, &labels) {
        return Ok(PlanOutcome::Held {
            current_name: context.target.filename.clone(),
            rule_label: selection.label(),
            reason,
        });
    }
    let (date, _) = resolve_plan_date(&context, None, DateMode::Image);
    finish_plan(
        store,
        target,
        &context,
        &semantic,
        &selection,
        options,
        &PlanFacts {
            date,
            perception: Some(perception),
            video: None,
        },
    )
}

/// Plan a video rename: observe, perceive the frames, analyze, resolve,
/// validate.
///
/// Videos keep the naming rules (and the extension) exactly like documents
/// and images; what differs is the observation: AVFoundation metadata plus a
/// bounded set of representative frames, classified and merged into one label
/// set that rides in the same request the image path uses (original spec
/// sections 4.4 and 5.3). The perception layer is a precondition here: a
/// video whose frames could not be perceived is held with its reason instead
/// of being proposed from its file name (original spec section 12). Changes
/// nothing and writes no journal rows.
async fn plan_video_rename(
    provider: &impl SemanticProvider,
    store: &RenameStore,
    target: &Path,
    options: &RenameOptions,
) -> Result<PlanOutcome, RenameError> {
    classify_video(target).map_err(|error| RenameError::Unsupported(error.to_string()))?;
    let mut context = observe(target)?;
    // Cheap checks first: rule errors and poisoned names never spend
    // extraction or inference.
    let selection = select_rule(store, options)?;
    if contains_prompt_injection(&context.target.filename) {
        return Ok(PlanOutcome::Held {
            current_name: context.target.filename.clone(),
            rule_label: selection.label(),
            reason: "probable prompt injection; proposal withheld".to_string(),
        });
    }
    let (video, labels) = match perceive_video(provider, target) {
        Ok(perceived) => perceived,
        Err(reason) => {
            return Ok(PlanOutcome::Held {
                current_name: context.target.filename.clone(),
                rule_label: selection.label(),
                reason,
            });
        }
    };
    context.video = Some(video.info.clone());
    context.perception = Some(labels.clone());
    let semantic = analyze(provider, &context).await?;
    if let Some(reason) = label_echo(&semantic, &labels.labels) {
        return Ok(PlanOutcome::Held {
            current_name: context.target.filename.clone(),
            rule_label: selection.label(),
            reason,
        });
    }
    let (date, _) = resolve_plan_date(&context, None, DateMode::Video);
    finish_plan(
        store,
        target,
        &context,
        &semantic,
        &selection,
        options,
        &PlanFacts {
            date,
            perception: Some(PerceptionReport::Labels(labels.labels)),
            video: Some(video),
        },
    )
}

/// Plan a document rename: observe, extract bounded signals, analyze,
/// resolve, validate.
///
/// Reads the filesystem but changes nothing and writes no journal rows.
async fn plan_document_rename(
    provider: &impl SemanticProvider,
    store: &RenameStore,
    target: &Path,
    options: &RenameOptions,
) -> Result<PlanOutcome, RenameError> {
    let kind =
        classify_document(target).map_err(|error| RenameError::Unsupported(error.to_string()))?;
    let mut context = observe(target)?;
    let (signals, document_ymd, injection) = read_document_signals(target, kind)?;
    context.semantic_signals.extend(signals);
    // Cheap checks first: rule errors and poisoned documents never spend inference.
    let selection = select_rule(store, options)?;
    if injection || contains_prompt_injection(&context.target.filename) {
        return Ok(PlanOutcome::Held {
            current_name: context.target.filename.clone(),
            rule_label: selection.label(),
            reason: "probable prompt injection; proposal withheld".to_string(),
        });
    }
    let semantic = analyze(provider, &context).await?;
    let (date, _) = resolve_plan_date(&context, document_ymd, DateMode::Document);
    finish_plan(
        store,
        target,
        &context,
        &semantic,
        &selection,
        options,
        &PlanFacts {
            date,
            perception: None,
            video: None,
        },
    )
}

/// Run the perception layer for one image path.
///
/// Perception is an add-on: a failure never fails the plan, it only leaves the
/// labels out with an explicit reason (never a silent degradation).
fn perceive(provider: &impl SemanticProvider, target: &Path) -> PerceptionReport {
    match provider.perceive(target) {
        Ok(info) => PerceptionReport::Labels(info.labels),
        Err(error) => PerceptionReport::Unavailable(error.to_string()),
    }
}

/// Run the video perception layer for one path.
///
/// Unlike the image add-on this layer is a precondition: without frames there
/// is nothing to perceive, and a video name must not be built from the file
/// name alone. A failure therefore comes back as the reason the proposal is
/// held (original spec sections 5.3 and 12).
fn perceive_video(
    provider: &impl SemanticProvider,
    target: &Path,
) -> Result<(VideoReport, PerceptionInfo), String> {
    let perceived: VideoPerception = provider
        .perceive_video(target)
        .map_err(|error| video_hold_reason(&error))?;
    let labels = perceived.merged_labels();
    if labels.labels.is_empty() {
        return Err(
            "no visual content could be perceived in the frames; proposal withheld".to_string(),
        );
    }
    let report = VideoReport {
        info: perceived.info,
        frames: perceived.frames.len(),
    };
    Ok((report, labels))
}

/// One bounded reason line for a video that could not be perceived.
///
/// The branches mirror the bridge's reason strings (original spec §12): a
/// container without a video track is audio only, one AVFoundation cannot open
/// is unreadable, and one whose frames could not be extracted or classified
/// has nothing to perceive. Every branch holds the proposal back.
fn video_hold_reason(error: &PerceptionError) -> String {
    match error {
        PerceptionError::Unavailable { reason } => {
            format!("video perception is unavailable ({reason}); proposal withheld")
        }
        PerceptionError::Failed { detail } if detail.starts_with("video-no-video-track") => {
            "the video has no video track (audio only); proposal withheld".to_string()
        }
        PerceptionError::Failed { detail } if detail.starts_with("video-unreadable") => {
            "AVFoundation could not read the video; proposal withheld".to_string()
        }
        PerceptionError::Failed { detail } if detail.starts_with("video-frames-unavailable") => {
            "no representative frame could be extracted; proposal withheld".to_string()
        }
        PerceptionError::Failed { detail } => {
            format!("video perception failed ({detail}); proposal withheld")
        }
        PerceptionError::Malformed { detail } => {
            format!("video perception answered something unexpected ({detail}); proposal withheld")
        }
    }
}

/// Hold a title that is exactly one perception label.
///
/// The classifier's English identifiers are data for the model, never a name
/// (original spec sections 4.3 and 5.2). A copy means the model echoed the
/// label instead of describing the picture, so the proposal is withheld with
/// a reason rather than renamed. Comparison normalises case and separators;
/// partial matches are not flagged.
fn label_echo(semantic: &SemanticInfo, labels: &[PerceptionLabel]) -> Option<String> {
    let title = normalize_echo(semantic.title.as_deref().unwrap_or_default());
    if title.is_empty() {
        return None;
    }
    labels
        .iter()
        .any(|entry| normalize_echo(&entry.label) == title)
        .then(|| format!("title repeats a perception label ({title}); proposal withheld"))
}

/// Lowercase and collapse whitespace/underscores for the echo comparison.
fn normalize_echo(value: &str) -> String {
    value
        .split(|character: char| character.is_whitespace() || character == '_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Observe a file through the existing pipeline.
fn observe(target: &Path) -> Result<NormalizedFileContext, RenameError> {
    let observed = crate::context::builder::build(target)
        .map_err(|error| RenameError::Unreadable(error.to_string()))?;
    crate::normalize::normalize(observed)
        .map_err(|error| RenameError::Unreadable(error.to_string()))
}

/// Read the bounded document text into signals, an optional date, and whether
/// the text looks like a prompt-injection payload.
fn read_document_signals(
    target: &Path,
    kind: DocumentKind,
) -> Result<DocumentSignals, RenameError> {
    let bytes = read_bounded(target, MAX_DOCUMENT_BYTES)
        .map_err(|error| RenameError::Unreadable(error.to_string()))?;
    let signals = extract_signals(kind, &bytes);
    let text = String::from_utf8_lossy(&bytes);
    let document_ymd = match scan_document_date(&text) {
        DocumentDate::One(year, month, day) => Some((year, month, day)),
        DocumentDate::None | DocumentDate::Ambiguous => Option::None,
    };
    Ok((signals, document_ymd, contains_prompt_injection(&text)))
}

/// Run the semantic provider, mapping unavailability explicitly.
async fn analyze(
    provider: &impl SemanticProvider,
    context: &NormalizedFileContext,
) -> Result<SemanticInfo, RenameError> {
    match provider.analyze(context).await {
        Ok(info) => Ok(info),
        Err(error) if error.is_unavailable() => {
            Err(RenameError::ModelUnavailable(error.to_string()))
        }
        Err(SemanticError::ModelInitFailed(detail)) => Err(RenameError::Semantic(format!(
            "model init failed: {detail}"
        ))),
        Err(error) => Err(RenameError::Semantic(format!("skipped: {error}"))),
    }
}

/// Choose the naming rule: `--template`, `--pattern`, `--preset`, default.
fn select_rule(store: &RenameStore, options: &RenameOptions) -> Result<RuleSelection, RenameError> {
    if let Some(template) = options.template.as_deref() {
        return Ok(RuleSelection::CustomTemplate(template.to_string()));
    }
    if let Some(spec) = options.pattern.as_deref() {
        let preset = find_preset(spec)
            .ok_or_else(|| RenameError::Unsupported(format!("unknown pattern: {spec}")))?;
        if preset.id == "P12" {
            return Err(RenameError::Unsupported(
                "P12 custom needs --template".to_string(),
            ));
        }
        return Ok(RuleSelection::Pattern(preset));
    }
    if let Some(name) = options.preset.as_deref() {
        let stored = store
            .get_preset(name)
            .map_err(|error| RenameError::Store(error.to_string()))?;
        let Some(preset) = stored else {
            return Err(RenameError::Unsupported(format!("unknown preset: {name}")));
        };
        return Ok(RuleSelection::Preset(preset.name, preset.template));
    }
    default_rule(store)
}

/// Resolve the stored default rule into a deferred selection.
fn default_rule(store: &RenameStore) -> Result<RuleSelection, RenameError> {
    let reference = store
        .get_default()
        .map_err(|error| RenameError::Store(error.to_string()))?;
    Ok(RuleSelection::Default(reference))
}

/// Resolve a deferred default reference into its template text.
fn default_template(store: &RenameStore, reference: &str) -> Result<String, RenameError> {
    if let Some(preset) = find_preset(reference) {
        if preset.id == "P12" {
            return Err(RenameError::Unsupported(
                "default P12 custom needs --template".to_string(),
            ));
        }
        return Ok(preset.template.to_string());
    }
    let name = reference.strip_prefix("preset:").unwrap_or(reference);
    let stored = store
        .get_preset(name)
        .map_err(|error| RenameError::Store(error.to_string()))?;
    match stored {
        Some(preset) => Ok(preset.template),
        Option::None => Err(RenameError::Store(format!(
            "default rule points at missing preset: {name}"
        ))),
    }
}

/// Which date order a target follows (original spec sections 5.2, 5.3, 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateMode {
    /// Creation first, then modification, then the document text.
    Document,
    /// EXIF capture time first: filesystem dates of photos are transfer times.
    Image,
    /// The container's recording timestamp first (same reason as images).
    Video,
}

/// Resolve the naming date for one plan.
///
/// Images and videos lead with their real capture signal (EXIF capture time,
/// the container's recording timestamp); documents keep the creation-first
/// order (original spec sections 6.4 and 5.3).
fn resolve_plan_date(
    context: &NormalizedFileContext,
    document_ymd: Option<(i32, u32, u32)>,
    mode: DateMode,
) -> (super::rules::ResolvedDate, super::rules::DatePreference) {
    let created = context.creation_date.as_deref();
    let modified = context
        .modification_date
        .as_deref()
        .or(context.target.modified_at.as_deref());
    match mode {
        DateMode::Document => resolve_date(created, modified, exif_capture(context), document_ymd),
        DateMode::Image => resolve_image_date(created, modified, exif_capture(context)),
        DateMode::Video => {
            resolve_video_date(created, modified, video_recorded(context).as_deref())
        }
    }
}

/// The EXIF capture value already extracted into the context, if any.
fn exif_capture(context: &NormalizedFileContext) -> Option<&str> {
    context.image_metadata.as_ref().and_then(|meta| {
        meta.capture_time
            .as_ref()
            .map(|capture| capture.value.as_str())
    })
}

/// The container's recording timestamp already extracted into the context, if
/// any (AVFoundation, Phase 6).
fn video_recorded(context: &NormalizedFileContext) -> Option<String> {
    context
        .video
        .as_ref()
        .and_then(|info| info.creation_date.clone())
}

/// The observation facts a plan carries beyond the model's answer.
#[derive(Debug, Clone)]
struct PlanFacts {
    /// The naming date (image order or document order, already resolved).
    date: ResolvedDate,
    /// What perception contributed (images and videos; `None` for documents).
    perception: Option<PerceptionReport>,
    /// What AVFoundation measured (videos only).
    video: Option<VideoReport>,
}

/// Resolve the template and validate the proposal.
///
/// Image targets resolve `{date}` EXIF-first (filesystem dates of photos are
/// transfer times); document targets keep the creation-first order.
fn finish_plan(
    store: &RenameStore,
    target: &Path,
    context: &NormalizedFileContext,
    semantic: &SemanticInfo,
    selection: &RuleSelection,
    options: &RenameOptions,
    facts: &PlanFacts,
) -> Result<PlanOutcome, RenameError> {
    let date = &facts.date;
    let date_format = options
        .date_format
        .clone()
        .unwrap_or_else(|| "YYYY-MM-DD".to_string());
    if !is_date_format(&date_format) {
        return Err(RenameError::Unsupported(format!(
            "unknown date format: {date_format}"
        )));
    }
    let current_name = context.target.filename.clone();
    let current_extension = context.target.extension.clone().unwrap_or_default();
    let inputs = rule_inputs(semantic, context, options, date, &current_extension);
    let template = match selection.template() {
        Some(template) => template.to_string(),
        Option::None => {
            let RuleSelection::Default(reference) = selection else {
                return Err(RenameError::Store("rule has no template".to_string()));
            };
            default_template(store, reference)?
        }
    };
    let stem = match resolve_template(&template, &inputs, &date_format) {
        // The model sometimes echoes the file name including its extension;
        // the extension is owned by the rule engine, so a trailing match is
        // the model's echo, not a second extension.
        Ok(stem) => strip_redundant_extension(&stem, &current_extension),
        Err(error) => {
            return Ok(PlanOutcome::Held {
                current_name,
                rule_label: selection.label(),
                reason: error.to_string(),
            });
        }
    };
    // Trailing dots and spaces are model punctuation, not name content
    // ("Budget memo. " must not become "Budget memo. .txt"), over-long
    // titles are cut back to MAX_STEM_CHARS at a word boundary (which needs
    // the spaces still in place), and the surviving whitespace runs become
    // underscores because spaces break shell tab-completion.
    let stem = bound_stem(stem.trim_end_matches(&['.', ' '][..]));
    let stem = underscore_whitespace(&stem);
    if stem.is_empty() {
        return Ok(PlanOutcome::Held {
            current_name,
            rule_label: selection.label(),
            reason: "title is empty after removing the echoed extension".to_string(),
        });
    }
    let proposal = format!("{stem}.{current_extension}");
    if let Err(problem) = check_filename(&proposal, &current_extension) {
        return Ok(PlanOutcome::Held {
            current_name,
            rule_label: selection.label(),
            reason: problem.to_string(),
        });
    }
    assemble_plan(
        target,
        &current_name,
        &proposal,
        selection,
        date,
        facts.perception.clone(),
        facts.video.clone(),
    )
}

/// Build the template inputs from semantics, context and CLI overrides.
fn rule_inputs(
    semantic: &SemanticInfo,
    context: &NormalizedFileContext,
    options: &RenameOptions,
    date: &ResolvedDate,
    _current_extension: &str,
) -> RuleInputs {
    RuleInputs {
        title: semantic.title.clone().unwrap_or_default(),
        category: semantic.category.clone().unwrap_or_default(),
        project: options
            .project
            .clone()
            .or_else(|| semantic.project.clone())
            .unwrap_or_default(),
        prefix: options.prefix.clone().unwrap_or_default(),
        suffix: options.suffix.clone().unwrap_or_default(),
        date: date.clone(),
        seq: options.seq.unwrap_or(1),
        parent: parent_name(&context.parent_directory),
        device: camera_label(context),
        file_type: context
            .target
            .extension
            .clone()
            .unwrap_or_default()
            .to_ascii_lowercase(),
    }
}

/// The last path component of the parent directory.
fn parent_name(parent: &str) -> String {
    Path::new(parent)
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Camera label from image metadata (empty for documents).
fn camera_label(context: &NormalizedFileContext) -> String {
    let Some(metadata) = context.image_metadata.as_ref() else {
        return String::new();
    };
    let mut parts = Vec::new();
    if let Some(make) = metadata
        .camera
        .make
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(make);
    }
    if let Some(model) = metadata
        .camera
        .model
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(model);
    }
    parts.join(" ")
}

/// Assemble the validated plan, detecting no-ops.
fn assemble_plan(
    target: &Path,
    current_name: &str,
    proposal: &str,
    selection: &RuleSelection,
    date: &ResolvedDate,
    perception: Option<PerceptionReport>,
    video: Option<VideoReport>,
) -> Result<PlanOutcome, RenameError> {
    if proposal == current_name {
        return Ok(PlanOutcome::Noop {
            current_name: current_name.to_string(),
            rule_label: selection.label(),
        });
    }
    let destination = sibling_path(target, proposal)
        .ok_or(RenameError::Precondition(NameProblem::ParentMissing))?;
    if destination.exists() {
        return Ok(PlanOutcome::Held {
            current_name: current_name.to_string(),
            rule_label: selection.label(),
            reason: NameProblem::Collision(destination.display().to_string()).to_string(),
        });
    }
    let snapshot = snapshot(target)?;
    Ok(PlanOutcome::Ready(RenamePlan {
        source: target.to_path_buf(),
        destination,
        current_name: current_name.to_string(),
        proposal: proposal.to_string(),
        rule_label: selection.label(),
        date_source: date.source,
        snapshot,
        perception,
        video,
    }))
}

/// Execute a plan: re-verify, journal before mutating, rename, mark result.
///
/// Ordering mirrors `operations::mover::execute_move` (record, then move).
pub fn execute_plan(plan: &RenamePlan, history: &OperationHistory) -> Result<String, RenameError> {
    recheck_before_execute(&plan.source, &plan.destination, &plan.snapshot)?;
    let operation_id = history
        .record_pending(
            "move",
            &plan.source,
            &plan.destination,
            Some(&plan.rule_label),
            Option::None,
            Option::None,
        )
        .map_err(|error| RenameError::History(error.to_string()))?;
    match std::fs::rename(&plan.source, &plan.destination) {
        Ok(()) => {
            // Mirrors `mover::execute_move_inner`: a rename that cannot be
            // marked successful is reported, so the row stays `pending`.
            history
                .mark_success(&operation_id)
                .map_err(|error| RenameError::History(error.to_string()))?;
            Ok(operation_id)
        }
        Err(error) => {
            // The filesystem was not mutated (rename is atomic); record the
            // failure and never a success.
            let _ = history.mark_failure(&operation_id, &error.to_string());
            Err(RenameError::Io(error.to_string()))
        }
    }
}

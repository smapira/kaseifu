//! Rename planner and executor (original spec sections 8.1-8.3, 9 and 10).
//!
//! Planning is side-effect free: observe, extract bounded document signals,
//! ask the semantic provider, apply the naming rule and validate. Execution
//! re-verifies everything immediately before the rename (TOCTOU), records
//! the operation in the journal **before** mutating (the existing `move`
//! row, so `undo` works unchanged), then marks the result.

use std::path::{Path, PathBuf};

use crate::models::file_context::NormalizedFileContext;
use crate::semantic::contract::SemanticInfo;
use crate::semantic::provider::{SemanticError, SemanticProvider};
use crate::storage::history::OperationHistory;

use super::document::{
    DocumentDate, DocumentKind, MAX_DOCUMENT_BYTES, classify_document, contains_prompt_injection,
    extract_signals, read_bounded, scan_document_date,
};
use super::rules::{
    PresetDef, ResolvedDate, RuleInputs, find_preset, is_date_format, resolve_date,
    resolve_template, strip_redundant_extension,
};
use super::store::RenameStore;
use super::validate::{
    FileSnapshot, NameProblem, check_filename, is_symlink, recheck_before_execute, sibling_path,
    snapshot,
};

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
}

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

/// Plan a rename: observe, extract, analyze, resolve, validate.
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
    finish_plan(
        store,
        target,
        &context,
        &semantic,
        &selection,
        options,
        document_ymd,
    )
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

/// Resolve the template and validate the proposal.
fn finish_plan(
    store: &RenameStore,
    target: &Path,
    context: &NormalizedFileContext,
    semantic: &SemanticInfo,
    selection: &RuleSelection,
    options: &RenameOptions,
    document_ymd: Option<(i32, u32, u32)>,
) -> Result<PlanOutcome, RenameError> {
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
    let (date, _) = resolve_date(
        context.creation_date.as_deref(),
        context
            .modification_date
            .as_deref()
            .or(context.target.modified_at.as_deref()),
        context.image_metadata.as_ref().and_then(|meta| {
            meta.capture_time
                .as_ref()
                .map(|capture| capture.value.as_str())
        }),
        document_ymd,
    );
    let inputs = rule_inputs(semantic, context, options, &date, &current_extension);
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
    // ("Budget memo. " must not become "Budget memo. .txt").
    let stem = stem.trim_end_matches(&['.', ' '][..]).to_string();
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
    assemble_plan(target, &current_name, &proposal, selection, &date)
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

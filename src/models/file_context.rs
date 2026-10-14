use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileContext {
    pub schema_version: String,
    pub target: TargetFile,
    pub directory_context: DirectoryContext,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetFile {
    pub path: PathBuf,
    pub filename: String,
    pub extension: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectoryContext {
    pub current: PathBuf,
    pub parent: Option<PathBuf>,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    #[default]
    Other,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source: EvidenceSource,
    pub key: String,
    pub value: serde_json::Value,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceSource {
    Fs,
    Dir,
    SpotlightMeta,
    SpotlightSearch,
    Xattr,
    Content,
    Derived,
}

// ============ PHASE 3: NORMALIZED MODEL ============
// NormalizedFileContext - output of Normalize phase

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NormalizedFileContext {
    pub target: TargetFile,
    pub semantic_signals: Vec<String>,
    pub provenance: Provenance,
    pub directory_summary: DirectorySummary,
    pub file_type: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
    pub size_bytes: u64,
    pub parent_directory: String,
    pub candidate_destinations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub image_metadata: Option<ImageMetadata>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub perception: Option<PerceptionInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub video: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub directory: Option<DirectoryFacts>,
}

/// Video facts the Swift bridge measured through AVFoundation (Phase 6).
///
/// Rust never opens or decodes a video: the bridge reports what it measured
/// (original spec section 5.3) and Rust carries the values into the request,
/// the naming date and the report. Every field is optional on purpose: a
/// container that does not report a value keeps it missing instead of
/// inventing one (original spec section 6.5).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VideoInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub frame_rate: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub creation_date: Option<String>,
}

impl VideoInfo {
    /// True when the bridge measured nothing informative.
    pub fn is_empty(&self) -> bool {
        self.duration_seconds.is_none()
            && self.width.is_none()
            && self.height.is_none()
            && self.frame_rate.is_none()
            && self.creation_date.is_none()
    }
}

/// Capture time with its origin (v3.2 image metadata).
///
/// `timezone` is only set when the source actually carries one (e.g. mdls).
/// EXIF datetimes have no zone: `timezone` stays `None` and
/// `timezone_known` is false. Never guess a zone.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CaptureTime {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timezone: Option<String>,
    pub source: String,
    pub timezone_known: bool,
}

/// GPS location in numeric form (v3.2 image metadata).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ImageLocation {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub altitude_m: Option<f64>,
    pub source: String,
}

/// Camera identification (v3.2 image metadata).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CameraInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub make: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
}

/// Pixel dimensions (v3.2 image metadata).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
}

/// Names for the model that do not come from pixels or from EXIF: the
/// on-device image classifier's labels (Phase 5b).
///
/// The labels never become a file name on their own: they ride to the model
/// as bounded data and the naming rule engine stays in Rust (original spec
/// sections 4.3 and 5.2). Omitted when the perception layer did not run.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PerceptionInfo {
    #[serde(default)]
    pub labels: Vec<PerceptionLabel>,
}

impl PerceptionInfo {
    /// True when the classifier contributed nothing.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }
}

/// One classification label with the confidence the classifier reported.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PerceptionLabel {
    pub label: String,
    pub confidence: f32,
}

/// Normalized image metadata (v3.2). Omitted for non-image files.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ImageMetadata {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub capture_time: Option<CaptureTime>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<ImageLocation>,
    #[serde(default)]
    pub camera: CameraInfo,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dimensions: Option<ImageDimensions>,
}

impl ImageMetadata {
    /// True when at least one informative field is present.
    pub fn is_empty(&self) -> bool {
        self.capture_time.is_none()
            && self.location.is_none()
            && self.camera.make.is_none()
            && self.camera.model.is_none()
            && self.dimensions.is_none()
    }

    /// True when GPS coordinates are present.
    pub fn has_location(&self) -> bool {
        self.location.is_some()
    }

    /// True when a capture time is present.
    pub fn has_capture_time(&self) -> bool {
        self.capture_time.is_some()
    }
}

/// What kaseifu read from a folder's own children (Phase 7, original spec
/// section 5.4).
///
/// This is the folder-side counterpart of [`VideoInfo`]: facts measured by one
/// bounded read, carried into the request so the model can name a collection.
/// It is deliberately **not** a second directory summary: the counts come from
/// [`crate::normalize::noise::DirectorySummary`], the single place that
/// aggregates `DirectoryEntry` values, and this type carries the observation
/// the request needs beside those counts (the bounded name samples, the file
/// type distribution and whether the read hit its cap).
///
/// Only a sample ever travels: at most [`DirectoryFacts::MAX_FILES`] file names
/// and [`DirectoryFacts::MAX_FOLDERS`] folder names, each cut to
/// [`DirectoryFacts::MAX_NAME_CHARS`] characters. Nothing here is recursive:
/// only the folder's own children were read (original spec section 5.4 forbids
/// unbounded recursion).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DirectoryFacts {
    /// Entries seen after noise names were dropped (at most 200).
    pub total_entries: usize,
    pub file_count: usize,
    pub directory_count: usize,
    /// True when the folder held more entries than the read bound: the counts
    /// are then a floor and the prompt says so instead of claiming a total.
    pub truncated: bool,
    /// Bounded, sorted child file names.
    pub files: Vec<String>,
    /// Bounded, sorted child folder names.
    pub folders: Vec<String>,
    /// File type distribution over every entry that was read, strongest first.
    pub extensions: Vec<ExtensionCount>,
}

/// One file type in a folder's distribution.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExtensionCount {
    pub extension: String,
    pub count: usize,
}

impl DirectoryFacts {
    /// How many child file names are carried.
    pub const MAX_FILES: usize = 12;
    /// How many child folder names are carried.
    pub const MAX_FOLDERS: usize = 8;
    /// How many file types are carried.
    pub const MAX_EXTENSIONS: usize = 8;
    /// Character budget of one carried child name.
    pub const MAX_NAME_CHARS: usize = 48;

    /// True when the folder offered nothing to name after exclusions.
    ///
    /// The caller holds the proposal instead of naming a folder from its own
    /// name (original spec section 5.4).
    pub fn is_empty(&self) -> bool {
        self.total_entries == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Provenance {
    pub downloader: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirectorySummary {
    pub parent_directory: String,
    pub child_count: usize,
}

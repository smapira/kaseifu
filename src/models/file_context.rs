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

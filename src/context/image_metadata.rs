//! v3.2 image metadata orchestration (Spotlight first, EXIF fallback).
//!
//! Assembly only: Spotlight values win, EXIF fills gaps, sources are
//! recorded per field, conflicts never overwrite blindly. Validation
//! (ranges, NaN/Inf, datetime sanity) lives in `crate::normalize::image`.
//!
//! Capture time has one extra rule: it must be a real capture signal. The
//! Spotlight EXIF attribute wins, then the EXIF in the file. A filesystem
//! creation date is never a capture time (photos carry the transfer time in
//! their filesystem dates): it stays available as `creation_date`, so a
//! screenshot without EXIF is dated honestly as "created" instead of being
//! reported as capture metadata that does not exist.

use std::path::Path;

use crate::models::file_context::{
    CaptureTime, Evidence, ImageDimensions, ImageLocation, ImageMetadata,
};

use super::exif::{self, ExifOutcome};

/// Extraction toggles (mirror `[metadata.image]`; defaults both on).
#[derive(Debug, Clone, Copy)]
pub struct MetadataOptions {
    pub spotlight: bool,
    pub exif_fallback: bool,
}

impl Default for MetadataOptions {
    fn default() -> Self {
        Self {
            spotlight: true,
            exif_fallback: true,
        }
    }
}

impl MetadataOptions {
    /// Build extraction toggles from resolved `[metadata.image]` settings.
    ///
    /// `enabled = false` disables both sources at once.
    pub fn from_resolved(metadata: &crate::config::ResolvedMetadata) -> Self {
        if !metadata.image_enabled {
            return Self {
                spotlight: false,
                exif_fallback: false,
            };
        }
        Self {
            spotlight: metadata.spotlight,
            exif_fallback: metadata.exif_fallback,
        }
    }
}

/// True for file names treated as images (EXIF fallback candidates).
///
/// Detection is extension-based; unknown extensions are not images.
pub fn is_image_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "heic" | "heif" | "hif" | "tif" | "tiff" | "png" | "webp"
        ),
        None => false,
    }
}

/// True for formats with an EXIF fallback path (JPEG/TIFF/HEIC best-effort).
pub fn has_exif_path(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "tif" | "tiff" | "heic" | "heif" | "hif"
        ),
        None => false,
    }
}

/// Assemble image metadata for one file.
///
/// - Non-images yield `None` (JSON omits `image_metadata`).
/// - Spotlight evidence is read first; EXIF runs only for missing fields.
/// - File mtime is never used as capture time; time zones are never guessed.
/// - A total miss yields `None` rather than an empty struct.
pub fn collect(
    path: &Path,
    evidence: &[Evidence],
    options: &MetadataOptions,
) -> Option<ImageMetadata> {
    if !is_image_file(path) {
        return None;
    }
    let mut meta = ImageMetadata::default();
    if options.spotlight {
        apply_spotlight(evidence, &mut meta);
    }
    if options.exif_fallback && has_exif_path(path) && has_gaps(&meta) {
        apply_exif_fallback(path, &mut meta);
    }
    if options.exif_fallback && is_png_file(path) && meta.dimensions.is_none() {
        apply_png_dimensions(path, &mut meta);
    }
    if meta.is_empty() { None } else { Some(meta) }
}

fn is_png_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
}

fn has_gaps(meta: &ImageMetadata) -> bool {
    meta.capture_time.is_none()
        || meta.location.is_none()
        || meta.camera.make.is_none()
        || meta.camera.model.is_none()
        || meta.dimensions.is_none()
}

fn apply_png_dimensions(path: &Path, meta: &mut ImageMetadata) {
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    if let Some((width, height)) = exif::png_dimensions(&bytes) {
        meta.dimensions = Some(ImageDimensions {
            width,
            height,
            source: Some("exif".to_string()),
        });
    }
}

// ---------------------------------------------------------------------------
// Spotlight
// ---------------------------------------------------------------------------

fn apply_spotlight(evidence: &[Evidence], meta: &mut ImageMetadata) {
    apply_spotlight_capture(evidence, meta);
    apply_spotlight_location(evidence, meta);
    apply_spotlight_camera(evidence, meta);
    apply_spotlight_dimensions(evidence, meta);
}

fn spotlight_value(evidence: &[Evidence], key: &str) -> Option<String> {
    evidence
        .iter()
        .find(|item| item.key == key)
        .and_then(|item| item.value.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "(null)")
}

/// Capture from the Spotlight EXIF attribute (a real capture signal).
fn apply_spotlight_capture(evidence: &[Evidence], meta: &mut ImageMetadata) {
    if meta.capture_time.is_some() {
        return;
    }
    if let Some(raw) = spotlight_value(evidence, "kMDItemEXIFDateTimeOriginal") {
        meta.capture_time = Some(spotlight_capture(&raw));
    }
}

fn spotlight_capture(raw: &str) -> CaptureTime {
    let (value, timezone) = split_spotlight_datetime(raw);
    CaptureTime {
        value,
        timezone: timezone.clone(),
        source: "spotlight".to_string(),
        timezone_known: timezone.is_some(),
    }
}

/// Split an `mdls` date (`2026-04-06 08:10:57 +0000`) into value + zone.
///
/// EXIF-style values without a zone keep `timezone_known = false`.
fn split_spotlight_datetime(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    match split_trailing_zone(trimmed) {
        Some((value, zone)) => (value, Some(zone)),
        None => (trimmed.to_string(), None),
    }
}

fn split_trailing_zone(text: &str) -> Option<(String, String)> {
    if text.ends_with('Z') && text.len() > 1 {
        let value = text[..text.len() - 1].trim().to_string();
        return Some((value, "Z".to_string()));
    }
    let mut parts = text.rsplit(' ');
    let last = parts.next()?;
    if is_numeric_zone(last) {
        let value = text[..text.len() - last.len()].trim().to_string();
        return Some((value, last.to_string()));
    }
    None
}

fn is_numeric_zone(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() != 5 || (chars[0] != '+' && chars[0] != '-') {
        return false;
    }
    chars[1..].iter().all(|c| c.is_ascii_digit())
}

fn apply_spotlight_location(evidence: &[Evidence], meta: &mut ImageMetadata) {
    if meta.location.is_some() {
        return;
    }
    let latitude = spotlight_value(evidence, "kMDItemLatitude").and_then(parse_finite_f64);
    let longitude = spotlight_value(evidence, "kMDItemLongitude").and_then(parse_finite_f64);
    let (Some(latitude), Some(longitude)) = (latitude, longitude) else {
        return;
    };
    let altitude_m = spotlight_value(evidence, "kMDItemAltitude").and_then(parse_finite_f64);
    meta.location = Some(ImageLocation {
        latitude,
        longitude,
        altitude_m,
        source: "spotlight".to_string(),
    });
}

fn parse_finite_f64(text: String) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn apply_spotlight_camera(evidence: &[Evidence], meta: &mut ImageMetadata) {
    if meta.camera.make.is_none()
        && let Some(make) = spotlight_value(evidence, "kMDItemAcquisitionMake")
    {
        meta.camera.make = Some(make);
        set_camera_source_once(meta, "spotlight");
    }
    if meta.camera.model.is_none()
        && let Some(model) = spotlight_value(evidence, "kMDItemAcquisitionModel")
    {
        meta.camera.model = Some(model);
        set_camera_source_once(meta, "spotlight");
    }
}

/// First writer wins: Spotlight runs before EXIF, so a mixed make/model
/// pair keeps the Spotlight origin instead of being relabeled.
fn set_camera_source_once(meta: &mut ImageMetadata, source: &str) {
    if meta.camera.source.is_none() {
        meta.camera.source = Some(source.to_string());
    }
}

fn apply_spotlight_dimensions(evidence: &[Evidence], meta: &mut ImageMetadata) {
    if meta.dimensions.is_some() {
        return;
    }
    let width = spotlight_value(evidence, "kMDItemPixelWidth").and_then(parse_u32);
    let height = spotlight_value(evidence, "kMDItemPixelHeight").and_then(parse_u32);
    let (Some(width), Some(height)) = (width, height) else {
        return;
    };
    meta.dimensions = Some(ImageDimensions {
        width,
        height,
        source: Some("spotlight".to_string()),
    });
}

fn parse_u32(text: String) -> Option<u32> {
    let cleaned = text.replace(',', "").trim().to_string();
    cleaned
        .parse::<f64>()
        .ok()
        .filter(|v| *v >= 0.0)
        .map(|v| v as u32)
}

// ---------------------------------------------------------------------------
// EXIF fallback (fills gaps only, records its source)
// ---------------------------------------------------------------------------

fn apply_exif_fallback(path: &Path, meta: &mut ImageMetadata) {
    let outcome = exif::parse_file(path);
    let ExifOutcome::Found(raw) = outcome else {
        return;
    };
    if meta.capture_time.is_none()
        && let Some(capture) = raw.capture_original.as_deref().and_then(exif_capture)
    {
        meta.capture_time = Some(capture);
    }
    if meta.location.is_none()
        && let (Some(latitude), Some(longitude)) = (raw.latitude, raw.longitude)
        && latitude.is_finite()
        && longitude.is_finite()
    {
        meta.location = Some(ImageLocation {
            latitude,
            longitude,
            altitude_m: raw.altitude_m.filter(|v| v.is_finite()),
            source: "exif".to_string(),
        });
    }
    if meta.camera.make.is_none()
        && let Some(make) = raw.make.clone()
    {
        meta.camera.make = Some(make);
        set_camera_source_once(meta, "exif");
    }
    if meta.camera.model.is_none()
        && let Some(model) = raw.model.clone()
    {
        meta.camera.model = Some(model);
        set_camera_source_once(meta, "exif");
    }
    if meta.dimensions.is_none()
        && let (Some(width), Some(height)) = (raw.width, raw.height)
    {
        meta.dimensions = Some(ImageDimensions {
            width,
            height,
            source: Some("exif".to_string()),
        });
    }
}

/// Convert EXIF `YYYY:MM:DD HH:MM:SS` to `YYYY-MM-DDTHH:MM:SS`.
///
/// EXIF carries no zone: `timezone` is always `None`. Returns `None` for
/// malformed input instead of guessing.
fn exif_capture(raw: &str) -> Option<CaptureTime> {
    let normalized = normalize_exif_datetime(raw)?;
    Some(CaptureTime {
        value: normalized,
        timezone: None,
        source: "exif".to_string(),
        timezone_known: false,
    })
}

fn normalize_exif_datetime(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let (date, time) = trimmed.split_once(' ')?;
    let date_parts: Vec<&str> = date.split(':').collect();
    if date_parts.len() != 3 {
        return None;
    }
    if !is_valid_date_parts(&date_parts) || !is_valid_time_part(time) {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}",
        date_parts[0], date_parts[1], date_parts[2], time
    ))
}

fn is_valid_date_parts(parts: &[&str]) -> bool {
    if parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let digits = parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()));
    if !digits {
        return false;
    }
    let month: u32 = parts[1].parse().unwrap_or(0);
    let day: u32 = parts[2].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn is_valid_time_part(time: &str) -> bool {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Redacted location summary for logs (never raw coordinates).
pub fn masked_location(meta: &ImageMetadata) -> String {
    if meta.location.is_some() {
        "GPS available (masked)".to_string()
    } else {
        "GPS not available".to_string()
    }
}

//! v3.2 image metadata validation (no guessing, origin-preserving).
//!
//! Rules: latitude -90..=90, longitude -180..=180, NaN/Infinity rejected,
//! partial GPS dropped, EXIF datetimes keep `timezone_known = false`, file
//! mtime is never treated as capture time.

use crate::models::file_context::ImageMetadata;

/// Validate assembled metadata; invalid fields are dropped, not repaired.
///
/// Returns `None` when nothing valid remains.
pub fn validate(mut meta: ImageMetadata) -> Option<ImageMetadata> {
    validate_location(&mut meta);
    validate_capture(&mut meta);
    validate_camera(&mut meta);
    validate_dimensions(&mut meta);
    if meta.is_empty() { None } else { Some(meta) }
}

fn validate_location(meta: &mut ImageMetadata) {
    let valid = meta.location.as_ref().is_some_and(|loc| {
        loc.latitude.is_finite()
            && loc.longitude.is_finite()
            && (-90.0..=90.0).contains(&loc.latitude)
            && (-180.0..=180.0).contains(&loc.longitude)
            && loc.altitude_m.is_none_or(|alt| alt.is_finite())
    });
    if !valid {
        meta.location = None;
    }
}

fn validate_capture(meta: &mut ImageMetadata) {
    let valid = meta.capture_time.as_ref().is_some_and(|capture| {
        !capture.value.trim().is_empty() && !capture.value.contains("(null)")
    });
    if !valid {
        meta.capture_time = None;
    }
}

fn validate_camera(meta: &mut ImageMetadata) {
    let make_empty = meta
        .camera
        .make
        .as_deref()
        .is_some_and(|s| s.trim().is_empty());
    let model_empty = meta
        .camera
        .model
        .as_deref()
        .is_some_and(|s| s.trim().is_empty());
    if make_empty {
        meta.camera.make = None;
    }
    if model_empty {
        meta.camera.model = None;
    }
    if meta.camera.make.is_none() && meta.camera.model.is_none() {
        meta.camera.source = None;
    }
}

fn validate_dimensions(meta: &mut ImageMetadata) {
    let valid = meta.dimensions.as_ref().is_some_and(|dims| {
        dims.width > 0 && dims.height > 0 && dims.width <= 100_000 && dims.height <= 100_000
    });
    if !valid {
        meta.dimensions = None;
    }
}

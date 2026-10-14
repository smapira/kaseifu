//! Bounded image input (original spec section 5.2, text-equivalent path).
//!
//! Images never cross the FFI as bytes: Rust passes the file path and the
//! already-extracted EXIF facts as text (the `fm-rename-v1` request carries
//! both). This module only classifies the target by extension. There is no
//! image decoding here: pixel dimensions and EXIF come from the existing
//! `context::image_metadata` pipeline, which this module reuses for the
//! extension predicate (single source of truth, no second extension list).

use std::path::Path;

/// Image kinds Phase 5 accepts (extension-based, case-insensitive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// JPEG / TIFF / HEIC: an EXIF fallback path exists.
    Photo,
    /// PNG / WebP: dimensions only, no EXIF.
    Raster,
}

/// Why a file is not a Phase 5 image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Not an image extension (documents, video, extension-less, ...).
    NotImage(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotImage(detail) => write!(f, "{detail}"),
        }
    }
}

/// True for image extensions (reuses the metadata pipeline predicate).
pub fn is_image(path: &Path) -> bool {
    crate::context::image_metadata::is_image_file(path)
}

/// Classify a path by extension (case-insensitive).
///
/// Images have no text body to scan, so the caller checks prompt-injection
/// markers on the file name only. Non-images are refused without guessing.
pub fn classify_image(path: &Path) -> Result<ImageKind, ImageError> {
    if !is_image(path) {
        return Err(ImageError::NotImage(format!(
            "{} is not an image (jpg, jpeg, png, heic, heif, tif, tiff, webp)",
            path.display()
        )));
    }
    if crate::context::image_metadata::has_exif_path(path) {
        Ok(ImageKind::Photo)
    } else {
        Ok(ImageKind::Raster)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extensions_classify_case_insensitively() {
        assert_eq!(
            classify_image(&PathBuf::from("IMG_1234.HEIC")),
            Ok(ImageKind::Photo)
        );
        assert_eq!(
            classify_image(&PathBuf::from("photo.JPG")),
            Ok(ImageKind::Photo)
        );
        assert_eq!(
            classify_image(&PathBuf::from("a.png")),
            Ok(ImageKind::Raster)
        );
        assert_eq!(
            classify_image(&PathBuf::from("a.TIFF")),
            Ok(ImageKind::Photo)
        );
        assert_eq!(
            classify_image(&PathBuf::from("a.webp")),
            Ok(ImageKind::Raster)
        );
    }

    #[test]
    fn non_images_are_refused_without_guessing() {
        assert!(matches!(
            classify_image(&PathBuf::from("note.txt")),
            Err(ImageError::NotImage(_))
        ));
        assert!(matches!(
            classify_image(&PathBuf::from("movie.mp4")),
            Err(ImageError::NotImage(_))
        ));
        assert!(matches!(
            classify_image(&PathBuf::from("noext")),
            Err(ImageError::NotImage(_))
        ));
    }

    #[test]
    fn predicate_matches_the_metadata_pipeline() {
        assert!(is_image(Path::new("a.heif")));
        assert!(is_image(Path::new("a.hif")));
        assert!(!is_image(Path::new("a.pdf")));
    }
}

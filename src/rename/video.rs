//! Bounded video input (original spec sections 5.3 and 4.4).
//!
//! Videos never cross the FFI as bytes: Rust passes the file path, the Swift
//! bridge opens the video read-only through AVFoundation, measures the
//! metadata and classifies a bounded set of representative frames. This
//! module only classifies the target by extension (through the shared
//! `context::video` predicate, so the list has one home) and refuses anything
//! else with the reason.

use std::path::Path;

/// Why a file is not a Phase 6 video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoError {
    /// Not a video extension (documents, images, extension-less, ...).
    NotVideo(String),
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotVideo(detail) => write!(f, "{detail}"),
        }
    }
}

/// True for video extensions (shares the predicate with the context layer).
pub fn is_video(path: &Path) -> bool {
    crate::context::video::is_video_file(path)
}

/// Classify a path by extension (case-insensitive).
///
/// Videos carry no text body and no EXIF in Rust: the caller checks
/// prompt-injection markers on the file name only, and the frames are read by
/// the bridge. Non-videos are refused without guessing.
pub fn classify_video(path: &Path) -> Result<(), VideoError> {
    if is_video(path) {
        return Ok(());
    }
    Err(VideoError::NotVideo(format!(
        "{} is not a video (mp4, mov, m4v, 3gp, 3g2)",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn video_extensions_classify_case_insensitively() {
        for name in ["clip.mp4", "VID_0012.MOV", "a.m4v", "b.3gp", "c.3G2"] {
            assert_eq!(classify_video(&PathBuf::from(name)), Ok(()), "{name}");
        }
    }

    #[test]
    fn non_videos_are_refused_without_guessing() {
        for name in ["note.txt", "IMG_1234.HEIC", "noext", "a.mkv"] {
            let outcome = classify_video(&PathBuf::from(name));
            let Err(VideoError::NotVideo(detail)) = outcome else {
                panic!("expected a refusal for {name}, got {outcome:?}");
            };
            assert!(detail.contains("is not a video"), "{detail}");
        }
    }

    #[test]
    fn the_predicate_matches_the_context_layer() {
        assert!(is_video(Path::new("a.MOV")));
        assert!(!is_video(Path::new("a.pdf")));
    }
}

//! Video file predicate (original spec section 5.3).
//!
//! Phase 6 renames the video containers AVFoundation reads on macOS: mp4,
//! mov, m4v, 3gp and 3g2. The list lives here, beside the image predicate, so
//! the rename layer and the semantic provider share one source of truth and
//! no layer has to depend on another one for it. Rust never opens a video:
//! the predicate only routes a path, and the Swift bridge reads the file
//! read-only. A container AVFoundation cannot read is reported by the bridge
//! with an explicit reason, never guessed here.

use std::path::Path;

/// True for file names treated as videos (extension-based, case-insensitive).
pub fn is_video_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_ascii_lowercase().as_str(),
            "mp4" | "mov" | "m4v" | "3gp" | "3g2"
        ),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn video_extensions_classify_case_insensitively() {
        for name in ["clip.mp4", "VID_0012.MOV", "a.M4V", "b.3gp", "c.3G2"] {
            assert!(is_video_file(&PathBuf::from(name)), "{name} is a video");
        }
    }

    #[test]
    fn other_extensions_are_not_videos() {
        for name in ["note.txt", "IMG_1234.HEIC", "noext", "a.webm", "a.mkv"] {
            assert!(
                !is_video_file(&PathBuf::from(name)),
                "{name} is not a video"
            );
        }
    }
}

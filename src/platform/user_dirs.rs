#![allow(clippy::bool_assert_comparison)]
//! The OS standard directories, and the file kinds they answer for.
//!
//! `dirs` already resolves the platform answers (the standard macOS folders, the
//! XDG entries on Linux), and this module gives them the one thing discovery
//! was missing: a *permission* side to go with `destinations::is_excluded_\n//! destination`'s refusal. `~/Documents` is not merely "not excluded": it is the
//! place documents belong, and an unindexed file has no other way to say so
//! (issue #18).
//!
//! Every function here is a pure lookup plus one `exists` call; nothing creates,
//! moves or inspects a file.

use std::path::PathBuf;

/// A standard user directory, as the operating system defines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserDirKind {
    Document,
    Picture,
    Video,
    Audio,
    Download,
    Desktop,
}

impl UserDirKind {
    /// Every kind, in the order a candidate list should offer them.
    pub const ALL: [UserDirKind; 6] = [
        UserDirKind::Document,
        UserDirKind::Picture,
        UserDirKind::Video,
        UserDirKind::Audio,
        UserDirKind::Download,
        UserDirKind::Desktop,
    ];

    /// The lowercase word used in reports and reasons.
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "documents",
            Self::Picture => "pictures",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Download => "downloads",
            Self::Desktop => "desktop",
        }
    }

    /// The platform path for this kind, when the platform defines one.
    ///
    /// `dirs` returns `None` for a folder the OS does not define (a Linux box
    /// without `xdg-user-dirs`) and a path that may not exist; both are normal.
    pub fn resolve(self) -> Option<PathBuf> {
        match self {
            Self::Document => dirs::document_dir(),
            Self::Picture => dirs::picture_dir(),
            Self::Video => dirs::video_dir(),
            Self::Audio => dirs::audio_dir(),
            Self::Download => dirs::download_dir(),
            Self::Desktop => dirs::desktop_dir(),
        }
    }
}

/// Every standard directory that exists on this machine.
///
/// The order is [`UserDirKind::ALL`], so two runs on one machine produce the
/// same list. A directory the OS does not define, or one that was never
/// created, is simply absent: callers add nothing and go on with the candidates
/// the search produced.
pub fn known_dirs() -> Vec<(UserDirKind, PathBuf)> {
    existing_dirs(
        UserDirKind::ALL
            .iter()
            .filter_map(|kind| kind.resolve().map(|path| (*kind, path))),
    )
}

/// [`known_dirs`] over an explicit candidate list, keeping only existing ones.
///
/// Split out so a test can name its own directories instead of depending on
/// the machine's home layout, which is the one thing a candidate test must
/// never do.
pub fn existing_dirs(
    candidates: impl IntoIterator<Item = (UserDirKind, PathBuf)>,
) -> Vec<(UserDirKind, PathBuf)> {
    candidates
        .into_iter()
        .filter(|(_, path)| path.is_dir())
        .collect()
}

/// The standard directory that answers for one file extension.
///
/// The table is deliberately small and explicit: it maps the types this tool
/// already knows (the image and video lists, the document kinds) plus the
/// archives a download folder collects. An unknown extension maps to nothing,
/// so no candidate is invented for it.
pub fn kind_for_extension(extension: &str) -> Option<UserDirKind> {
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let kind = match extension.as_str() {
        "pdf" | "md" | "txt" | "csv" | "doc" | "docx" | "rtf" | "pages" | "xls" | "xlsx"
        | "ppt" | "pptx" | "key" | "numbers" => UserDirKind::Document,
        "png" | "jpg" | "jpeg" | "heic" | "heif" | "hif" | "tif" | "tiff" | "webp" | "gif"
        | "bmp" => UserDirKind::Picture,
        "mp4" | "mov" | "m4v" | "avi" | "mkv" => UserDirKind::Video,
        "mp3" | "m4a" | "wav" | "aif" | "aiff" | "flac" => UserDirKind::Audio,
        "zip" | "dmg" | "pkg" | "tar" | "gz" | "iso" => UserDirKind::Download,
        _ => return None,
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_table_covers_the_standard_kinds() {
        // The issue's examples first, then the neighbours of each family.
        assert_eq!(kind_for_extension("pdf"), Some(UserDirKind::Document));
        assert_eq!(kind_for_extension("md"), Some(UserDirKind::Document));
        assert_eq!(kind_for_extension("csv"), Some(UserDirKind::Document));
        assert_eq!(kind_for_extension("png"), Some(UserDirKind::Picture));
        assert_eq!(kind_for_extension("jpg"), Some(UserDirKind::Picture));
        assert_eq!(kind_for_extension("mp4"), Some(UserDirKind::Video));
        assert_eq!(kind_for_extension("mp3"), Some(UserDirKind::Audio));
        assert_eq!(kind_for_extension("zip"), Some(UserDirKind::Download));
        // Case and a leading dot are not a different type.
        assert_eq!(kind_for_extension(".PNG"), Some(UserDirKind::Picture));
        assert_eq!(kind_for_extension(" Pdf "), Some(UserDirKind::Document));
        // Unknown, empty and odd shapes map to nothing (never to Desktop).
        for extension in ["", "   ", "xyz", "tar.gz", "no-extension"] {
            assert_eq!(
                kind_for_extension(extension),
                None,
                "must map to nothing: {extension}"
            );
        }
    }

    #[test]
    fn the_desktop_is_never_an_extension_kind() {
        // Files come *from* the Desktop; nothing belongs there by type.
        for extension in ["pdf", "png", "mp4", "mp3", "zip"] {
            assert_ne!(kind_for_extension(extension), Some(UserDirKind::Desktop));
        }
    }

    #[test]
    fn existing_dirs_keeps_the_defined_and_the_created() {
        let root = std::env::temp_dir().join(format!("kaseifu-user-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Documents")).expect("fixture");
        let candidates = vec![
            (UserDirKind::Document, root.join("Documents")),
            (UserDirKind::Picture, root.join("Pictures")),
        ];
        let kept = existing_dirs(candidates);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].0, UserDirKind::Document);
        // The real machine's list is only checked for shape: a missing folder
        // is absent, and nothing panics when the OS defines none.
        for (kind, path) in known_dirs() {
            assert!(path.is_dir(), "kept a missing folder: {}", path.display());
            assert!(!kind.label().is_empty());
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

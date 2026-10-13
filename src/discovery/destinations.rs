//! Directories that can never be a move destination.
//!
//! Spotlight indexes files from places a user would never choose as a
//! destination: app-internal stores, caches, hidden trees and
//! machine-generated folders. Left alone, those folders win the ranking on
//! name similarity alone — measured on a real photo,
//! `~/Pictures/Eagle.library/images/<hash>.info/IMG_5020.jpg` (Eagle stores
//! one image per hash-named folder) scored 0.88 filename similarity against
//! `IMG_3020.jpeg` and filled 38 of the 50 candidate slots.
//!
//! The rules below describe *path shapes* that cannot be a destination
//! rather than a list of banned paths, so they hold for folders that were
//! never measured. Every rule was chosen against a real population: a scan
//! of 171,428 directories under `$HOME` and `/Volumes` found no hand-made
//! folder that any rule here would drop.

use crate::discovery::scoring;

/// Extensions of macOS package / app-managed container directories (without
/// the leading dot): a file inside one of these belongs to an application's
/// own storage, not to a folder the user organizes by hand. Extensions
/// ending in `library` are matched separately because the family is large
/// (`.library`, `.photoslibrary`, `.tvlibrary`, `.aplibrary`, …).
const BUNDLE_EXTENSIONS: [&str; 6] = ["app", "bundle", "framework", "pkg", "plugin", "xcodeproj"];

/// Deepest path component that counts as a macOS library root: `/Library`,
/// `/Users/<user>/Library`, `/Volumes/<volume>/Library`. A `Library` folder
/// further down a user tree can be a folder the user made, so it stays.
const LIBRARY_ROOT_MAX_DEPTH: usize = 2;

/// Path component names that only appear inside cache and temp areas. The
/// plural forms cover macOS (`Caches`), the singular forms cover app-local
/// folders (`cache/`), and the `*.cache` suffix covers dotted variants such
/// as `~/.cache`, which may sit above a candidate directory.
const CACHE_COMPONENT_NAMES: [&str; 3] = ["cache", "caches", "temporaryitems"];

/// Hidden system areas that exist on every macOS volume and are never a
/// destination. The destination's own name is already refused while it is
/// hidden; naming the components closes the nested case
/// (`~/.Trash/old-album`) where a visible folder name would otherwise pass.
const SYSTEM_AREA_COMPONENT_NAMES: [&str; 6] = [
    ".trash",
    ".trashes",
    ".spotlight-v100",
    ".fseventsd",
    ".documentrevisions-v100",
    ".temporaryitems",
];

/// Shortest hex token treated as a content hash or storage id.
const HEX_TOKEN_MIN_LENGTH: usize = 16;

/// Shortest `A-Z0-9` token treated as a serial (Eagle's `<hash>.info`
/// folders are 13 characters; timestamp folders such as
/// `20260925T175534` are 15).
const SERIAL_TOKEN_MIN_LENGTH: usize = 12;

/// True when `dir` cannot be a move destination for a user file.
///
/// `dir` is a directory path as discovered from the filesystem. Only the
/// path text is inspected — no filesystem access, no user-specific
/// configuration — so the answer is stable across machines.
///
/// The target file's own parent directory is excluded by the caller; this
/// predicate is about the shape of the destination itself.
pub fn is_excluded_destination(dir: &str) -> bool {
    let components: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    let Some(name) = components.last() else {
        return false;
    };
    is_inside_bundle(&components)
        || is_macos_library_tree(&components)
        || is_hidden_destination(name)
        || is_cache_or_temp(&components)
        || is_system_area(&components)
        || is_generated_name(name)
}

/// True when any component is an app-managed container, e.g.
/// `~/Pictures/Eagle.library/images` or `/Applications/Foo.app/Contents`.
fn is_inside_bundle(components: &[&str]) -> bool {
    components.iter().any(|name| is_bundle_container(name))
}

/// True when the component's extension marks a package: `Eagle.library`,
/// `Photos.photoslibrary`, `App.app`. The extension sits after the last dot,
/// so a plain folder named `mylibrary` or `notes.app-backup` stays.
fn is_bundle_container(name: &str) -> bool {
    let lowered = name.to_lowercase();
    let Some((_, extension)) = lowered.rsplit_once('.') else {
        return false;
    };
    extension.ends_with("library") || BUNDLE_EXTENSIONS.contains(&extension)
}

/// True for the standard macOS library trees (`/Library`,
/// `/Users/<user>/Library`, `/Volumes/<volume>/Library`).
fn is_macos_library_tree(components: &[&str]) -> bool {
    components
        .iter()
        .take(LIBRARY_ROOT_MAX_DEPTH + 1)
        .any(|name| name.eq_ignore_ascii_case("Library"))
}

/// True when the destination directory itself is hidden (`.Trash`,
/// `.config`): macOS hides such folders from the user, so they cannot be a
/// deliberate destination.
///
/// Only the destination's own name is inspected. A visible folder inside a
/// hidden tree (`~/.ghq/<repo>/static/img`, a checkout under `.delta/`) is
/// still reachable through its visible part and is kept.
fn is_hidden_destination(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

/// True when any component marks a cache or temporary area, e.g.
/// `~/Library/Caches/...`, `/Volumes/Work/TemporaryItems` or `.../.cache/...`.
fn is_cache_or_temp(components: &[&str]) -> bool {
    components.iter().any(|name| {
        let lowered = name.to_lowercase();
        CACHE_COMPONENT_NAMES.contains(&lowered.as_str()) || lowered.ends_with(".cache")
    })
}

/// True when any component names an OS-reserved area (trash, Spotlight
/// index, volume metadata).
fn is_system_area(components: &[&str]) -> bool {
    components
        .iter()
        .any(|name| SYSTEM_AREA_COMPONENT_NAMES.contains(&name.to_lowercase().as_str()))
}

/// True when the directory name looks machine-generated rather than chosen.
///
/// The extension is stripped first (`LY2TLHNM2EO5K.info` → `LY2TLHNM2EO5K`),
/// reusing the same stem split the scorer uses. Human names survive: they
/// keep a separator (`2024-photos`, `roll-001`), mix case
/// (`PhotoBackup2024`) or stay short (`IMG3020`).
fn is_generated_name(name: &str) -> bool {
    let (stem, _) = scoring::split_stem(name);
    is_uuid(&stem) || is_hash_token(&stem) || is_serial_token(&stem)
}

/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` (any case) — the identifier shape
/// apps and services use for per-item folders.
fn is_uuid(token: &str) -> bool {
    const GROUP_LENGTHS: [usize; 5] = [8, 4, 4, 4, 12];
    let groups: Vec<&str> = token.split('-').collect();
    groups.len() == GROUP_LENGTHS.len()
        && groups.iter().zip(GROUP_LENGTHS).all(|(group, length)| {
            group.len() == length && group.chars().all(|c| c.is_ascii_hexdigit())
        })
}

/// A long run of hexadecimal characters: content hashes (`sha256/<hex>`) and
/// editor storage ids.
fn is_hash_token(token: &str) -> bool {
    token.len() >= HEX_TOKEN_MIN_LENGTH && token.chars().all(|c| c.is_ascii_hexdigit())
}

/// A long run of `A-Z0-9` mixing letters and digits: Eagle's `<hash>.info`
/// folders, timestamp folders such as `20260925T175534`, browser storage ids.
fn is_serial_token(token: &str) -> bool {
    token.len() >= SERIAL_TOKEN_MIN_LENGTH
        && token
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && token.chars().any(|c| c.is_ascii_digit())
        && token.chars().any(|c| c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Excluded destinations, rule by rule
    // -----------------------------------------------------------------------

    #[test]
    fn excludes_app_managed_containers() {
        // Measured root cause: Eagle keeps one image per `<hash>.info` folder
        // inside `Eagle.library`, and those folders filled 38 of the 50
        // candidate slots for a real photo.
        for dir in [
            "/Users/test/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info",
            "/Users/test/Pictures/Photos.photoslibrary/originals",
            "/Volumes/Archive/Music.tvlibrary/Media",
            "/Applications/Preview.app/Contents/Resources",
            "/Users/test/dev/App.framework/Versions",
            "/Users/test/Downloads/Installer.pkg/Payload",
        ] {
            assert!(
                is_excluded_destination(dir),
                "app-managed container must be excluded: {dir}"
            );
        }
    }

    #[test]
    fn excludes_macos_library_trees() {
        for dir in [
            "/Library/Fonts",
            "/Users/test/Library/Application Support/Anki2/User 1/collection.media",
            "/Users/test/Library/Application Support/Code/User/workspaceStorage/e368edc30f1c4d7f88489c955fa6569a/chatSessions",
            "/Volumes/Backup/Library/Photos",
        ] {
            assert!(
                is_excluded_destination(dir),
                "library tree must be excluded: {dir}"
            );
        }
    }

    #[test]
    fn keeps_library_folders_inside_a_user_tree() {
        // Only the standard roots (`/Library`, `<home>/Library`, volume root)
        // count; a `Library` folder deeper in a user tree stays.
        for dir in [
            "/Users/test/Documents/Library/images",
            "/Volumes/Work/Projects/Library/Archive",
        ] {
            assert!(
                !is_excluded_destination(dir),
                "user-made Library folder must stay: {dir}"
            );
        }
    }

    #[test]
    fn excludes_cache_hidden_and_system_areas() {
        for dir in [
            "/Users/test/.Trash",
            "/Users/test/.Trash/old-album",
            "/Volumes/Archive/.Spotlight-V100/store",
            "/Volumes/Archive/.fseventsd",
            "/Users/test/dev/.cache/img",
            "/Users/test/Pictures/Caches",
            "/Users/test/Pictures/cache/2024",
            "/Volumes/Work/TemporaryItems",
            "/Users/test/AppData/things.cache/objects",
        ] {
            assert!(
                is_excluded_destination(dir),
                "cache, hidden or system area must be excluded: {dir}"
            );
        }
    }

    #[test]
    fn keeps_visible_folders_inside_hidden_trees() {
        // Hidden ancestors are common in real layouts (`.ghq` checkouts,
        // `.local/share`, this repository's own worktrees under `.delta/`),
        // and the visible folder below them is still a usable destination, so
        // only the destination's own name decides.
        for dir in [
            "/Users/test/.ghq/github.com/me/blog/static/img",
            "/Users/test/.local/share/app/photos",
            "/Users/test/dev/.delta/worktrees/abc/repo/target/fixture/images",
        ] {
            assert!(
                !is_excluded_destination(dir),
                "visible folder inside a hidden tree must stay: {dir}"
            );
        }
    }

    #[test]
    fn excludes_machine_generated_names() {
        for dir in [
            // Eagle-style serial (the store rule already covers this path).
            "/Users/test/serial/LY2TLHNM2EO5K.info",
            // Timestamp folder (editor logs).
            "/Users/test/serial/20260925T175534",
            // Content hash and editor storage id.
            "/Users/test/hashes/e368edc30f1c4d7f88489c955fa6569a",
            "/Users/test/.docker/sbom/sha256/98e1fa296b26034fc29f9032ec3436338972edb5f05cec880fea0d51b85dee9d",
            // UUID folder.
            "/Users/test/uuids/A7EC43F6-2D4A-4B40-91C0-D4F2CB1D2722",
        ] {
            assert!(
                is_excluded_destination(dir),
                "generated folder must be excluded: {dir}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Kept destinations: the over-exclusion boundary
    // -----------------------------------------------------------------------

    #[test]
    fn keeps_user_folders() {
        for dir in [
            "/Users/test/Pictures",
            "/Users/test/Documents/2024-photos",
            "/Volumes/External/PhotoBackup2024",
            "/Users/test/Pictures/2024photos",
            "/Users/test/Pictures/IMG3020",
            "/Users/test/Pictures/IMG_3020",
            "/Users/test/Pictures/roll-001",
            "/Users/test/dev/sha256",
            "/Users/test/Desktop/auction/done/camera",
            "/Users/test/OS/media/symphony_workspaces/documents/media/misc",
            "/Users/test/Documents/State Of Mind Labs",
            "/Users/test/Desktop/stash/document/Heml博覧会",
        ] {
            assert!(
                !is_excluded_destination(dir),
                "user folder must stay a destination: {dir}"
            );
        }
    }

    #[test]
    fn keeps_folders_that_only_look_like_packages() {
        // The package rule keys off the extension after the last dot, so a
        // folder is never dropped for a word it merely contains.
        for dir in [
            "/Users/test/Pictures/mylibrary",
            "/Users/test/Pictures/notes.app-backup",
            "/Users/test/Pictures/photos.library-backup",
        ] {
            assert!(
                !is_excluded_destination(dir),
                "only real package extensions are excluded: {dir}"
            );
        }
    }

    #[test]
    fn serial_shaped_hand_written_names_are_a_documented_tradeoff() {
        // All-uppercase, separator-free and long: indistinguishable from an
        // app-generated serial. A scan of 171,428 real directories under
        // `$HOME` and `/Volumes` found none, so the tradeoff is accepted —
        // pinned here so a future change is a deliberate one.
        assert!(is_excluded_destination(
            "/Users/test/Pictures/PHOTOS2024BACKUP"
        ));
    }

    #[test]
    fn handles_path_edge_shapes() {
        assert!(
            !is_excluded_destination(""),
            "empty path is not a destination"
        );
        assert!(
            !is_excluded_destination("/"),
            "filesystem root is not excluded"
        );
        assert!(
            !is_excluded_destination("/Users/test/Pictures/日本語フォルダ"),
            "non-ASCII names are not serials"
        );
        assert!(
            is_excluded_destination(
                "/Users/test/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info/"
            ),
            "a trailing slash must not change the answer"
        );
        assert!(
            !is_excluded_destination("/Users/test/Pictures/IMG_3020/"),
            "a trailing slash must not change the answer"
        );
        assert!(
            is_excluded_destination("//Users//test//Library//images"),
            "duplicate slashes must not change the answer"
        );
    }
}

//! Bounded folder observation and the folder path rules that decide whether a
//! folder may be renamed at all (original spec section 5.4).
//!
//! Two jobs live here because both belong to the context layer:
//!
//! - **Reading.** [`read_directory_facts`] reads one directory level (never a
//!   recursion) up to a fixed cap and returns the bounded observation the
//!   request carries: counts, child names, file type distribution. One entry
//!   more than the cap is read so a truncated read is *detected* rather than
//!   reported as an exact total.
//! - **Path rules.** [`refuse_reason`] answers whether kaseifu may rename a
//!   folder. Its name-shape rules ([`is_bundle_container`],
//!   [`is_library_tree`], [`is_system_area`], [`is_cache_or_temp`],
//!   [`is_hidden_name`], [`is_system_root_name`]) are the ones the destination
//!   authority in `crate::discovery::destinations` already used; they moved
//!   down here so each rule has one implementation and both policies
//!   (destination eligibility, rename eligibility) can call it. The video
//!   extension predicate moved to `crate::context::video` in Phase 6 for the
//!   same reason.
//!
//! Nothing in this module writes, follows a symbolic link or shells out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::models::file_context::{DirectoryEntry, DirectoryFacts, EntryKind, ExtensionCount};
use crate::normalize::noise::{DirectorySummary, is_noise_file};
use crate::platform::macos::filesystem;

/// How many child entries are read (one extra is read to detect the cap).
pub const MAX_DIRECTORY_ENTRIES: usize = 200;

/// True when `path` is a folder (`false` when it does not exist or is a file).
///
/// The filesystem type decides, not the name: a folder called `notes.png` is a
/// folder.
pub fn is_directory_target(path: &Path) -> bool {
    path.is_dir()
}

/// Read one folder level into bounded facts.
///
/// Names are sorted (the order `read_dir` returns is unspecified), noise names
/// are dropped through [`is_noise_file`] (the existing authority) and the file
/// type distribution is counted over every entry that was read, not over the
/// bounded name sample: a 200 entry folder must not report two types just
/// because only twelve names travel.
///
/// An unreadable folder is an error (the caller reports it and renames
/// nothing), never an empty observation.
pub fn read_directory_facts(path: &Path) -> anyhow::Result<DirectoryFacts> {
    let read = filesystem::read_directory(path, MAX_DIRECTORY_ENTRIES + 1)?;
    let truncated = read.len() > MAX_DIRECTORY_ENTRIES;
    let mut entries: Vec<DirectoryEntry> = read
        .into_iter()
        .take(MAX_DIRECTORY_ENTRIES)
        .filter(|entry| !is_noise_file(&entry.name))
        .collect();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    // The counts come from the one existing aggregation.
    let summary = DirectorySummary::new(&entries);
    Ok(DirectoryFacts {
        total_entries: summary.total_entries,
        file_count: summary.file_count,
        directory_count: summary.directory_count,
        truncated,
        files: file_names(&entries, DirectoryFacts::MAX_FILES),
        folders: folder_names(&entries, DirectoryFacts::MAX_FOLDERS),
        extensions: extension_counts(&entries),
    })
}

/// The first `limit` child file names, each cut to the contract budget.
fn file_names(entries: &[DirectoryEntry], limit: usize) -> Vec<String> {
    names(entries, limit, |kind| matches!(kind, EntryKind::File))
}

/// The first `limit` child folder names, each cut to the contract budget.
fn folder_names(entries: &[DirectoryEntry], limit: usize) -> Vec<String> {
    names(entries, limit, |kind| matches!(kind, EntryKind::Directory))
}

/// The first `limit` names of one kind, each cut to the contract budget.
fn names(
    entries: &[DirectoryEntry],
    limit: usize,
    keep: impl Fn(&EntryKind) -> bool,
) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| keep(&entry.kind))
        .take(limit)
        .map(|entry| bound_name(&entry.name))
        .collect()
}

/// Cut one child name to the contract budget (never splitting a character).
fn bound_name(name: &str) -> String {
    name.chars().take(DirectoryFacts::MAX_NAME_CHARS).collect()
}

/// Count file extensions over every entry that was read.
///
/// Files without an extension stay out of the distribution (there is nothing
/// to report); they still appear in `files` and in `file_count`. The result is
/// ordered by count and then by name so the same folder always renders the
/// same line, and is cut to [`DirectoryFacts::MAX_EXTENSIONS`] types.
fn extension_counts(entries: &[DirectoryEntry]) -> Vec<ExtensionCount> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File))
    {
        if let Some(extension) = extension_of(&entry.name) {
            *counts.entry(extension).or_insert(0) += 1;
        }
    }
    let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();
    pairs.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    pairs
        .into_iter()
        .take(DirectoryFacts::MAX_EXTENSIONS)
        .map(|(extension, count)| ExtensionCount { extension, count })
        .collect()
}

/// The lowercase extension of a child name (`.` prefixed, `None` when absent).
fn extension_of(name: &str) -> Option<String> {
    let extension = Path::new(name).extension()?.to_string_lossy();
    let lowered = extension.to_ascii_lowercase();
    (!lowered.is_empty()).then_some(lowered)
}

// ---------------------------------------------------------------------------
// Folder path rules (the shared half of destination eligibility)
// ---------------------------------------------------------------------------

/// File extensions of macOS package / app-managed container directories.
const BUNDLE_EXTENSIONS: [&str; 6] = ["app", "bundle", "framework", "pkg", "plugin", "xcodeproj"];

/// Deepest path component that counts as a macOS library root: `/Library`,
/// `/Users/<user>/Library`, `/Volumes/<volume>/Library`.
const LIBRARY_ROOT_MAX_DEPTH: usize = 2;

/// Path component names that only appear inside cache and temp areas.
const CACHE_COMPONENT_NAMES: [&str; 3] = ["cache", "caches", "temporaryitems"];

/// Hidden system areas that exist on every macOS volume.
const SYSTEM_AREA_COMPONENT_NAMES: [&str; 6] = [
    ".trash",
    ".trashes",
    ".spotlight-v100",
    ".fseventsd",
    ".documentrevisions-v100",
    ".temporaryitems",
];

/// Names of the root-level directories macOS reserves for itself. Only the
/// **first** component of an absolute path is matched (R8): macOS system
/// trees live at the volume root, while `/Users/me/var`, `/Users/me/tmp`,
/// `/Users/me/system-backup` or `/Users/me/private-docs` are folders the
/// user made. Matching any component (or a substring, as an earlier version
/// did with `contains`) silently dropped those user folders from the
/// candidate list.
///
/// This is the **destination** policy list: the trees whose whole subtree can
/// never receive a moved file.
const SYSTEM_TREE_NAMES: [&str; 7] = ["system", "private", "usr", "bin", "sbin", "var", "tmp"];

/// Every directory macOS owns at the volume root, including the ones a rename
/// policy must refuse *themselves* (`/Applications`, `/Library`, `/Users`,
/// `/Volumes`, ...).
///
/// The destination policy only needs [`SYSTEM_TREE_NAMES`] because it asks
/// "is this path inside a system tree?"; the rename policy asks "is this path
/// one of the OS's own roots?", which is a different question with a wider
/// answer.
const PROTECTED_ROOT_NAMES: [&str; 16] = [
    "applications",
    "bin",
    "cores",
    "dev",
    "etc",
    "home",
    "library",
    "opt",
    "private",
    "sbin",
    "system",
    "tmp",
    "users",
    "usr",
    "var",
    "volumes",
];

/// True when the component's extension marks a package: `Eagle.library`,
/// `Photos.photoslibrary`, `App.app`. The extension sits after the last dot,
/// so a plain folder named `mylibrary` or `notes.app-backup` stays.
pub fn is_bundle_container(name: &str) -> bool {
    let lowered = name.to_lowercase();
    let Some((_, extension)) = lowered.rsplit_once('.') else {
        return false;
    };
    extension.ends_with("library") || BUNDLE_EXTENSIONS.contains(&extension)
}

/// True for the standard macOS library trees (`/Library`,
/// `/Users/<user>/Library`, `/Volumes/<volume>/Library`).
///
/// Only the first [`LIBRARY_ROOT_MAX_DEPTH`] components are inspected; a
/// `Library` folder deeper in a user tree can be one the user made.
pub fn is_library_tree(components: &[&str]) -> bool {
    components
        .iter()
        .take(LIBRARY_ROOT_MAX_DEPTH + 1)
        .any(|name| name.eq_ignore_ascii_case("Library"))
}

/// True when any component marks a cache or temporary area.
///
/// This is the destination-eligibility shape: a moved file must never land
/// anywhere inside such an area.
pub fn is_cache_or_temp(components: &[&str]) -> bool {
    components.iter().any(|name| is_cache_or_temp_name(name))
}

/// True when one folder name marks a cache or temporary area.
///
/// This is the rename-eligibility shape: only the folder's own name refuses a
/// rename, so `~/Documents/Cache` stays refused while a folder inside a
/// user-made `Cache` folder (or beside one) is still a rename target. The
/// destination rule above keeps the whole-subtree shape.
pub fn is_cache_or_temp_name(name: &str) -> bool {
    let lowered = name.to_lowercase();
    CACHE_COMPONENT_NAMES.contains(&lowered.as_str()) || lowered.ends_with(".cache")
}

/// True when any component names an OS-reserved area (trash, Spotlight index,
/// volume metadata).
pub fn is_system_area(components: &[&str]) -> bool {
    components
        .iter()
        .any(|name| SYSTEM_AREA_COMPONENT_NAMES.contains(&name.to_lowercase().as_str()))
}

/// True when the destination's own name is hidden (`.Trash`, `.config`).
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

/// True when one path component names a system tree (the destination policy
/// list). The comparison is case-insensitive and expects a bare component.
pub fn is_system_tree_name(name: &str) -> bool {
    let lowered = name.to_lowercase();
    SYSTEM_TREE_NAMES.contains(&lowered.as_str())
}

/// True when one path component names a directory macOS owns at the volume
/// root (the rename policy list).
pub fn is_protected_root_name(name: &str) -> bool {
    let lowered = name.to_lowercase();
    PROTECTED_ROOT_NAMES.contains(&lowered.as_str())
}

// ---------------------------------------------------------------------------
// Folder rename eligibility
// ---------------------------------------------------------------------------

/// The facts about the process that decide the two path comparisons a folder
/// rename must refuse (home and the working directory).
///
/// They are passed in explicitly instead of read inside the predicate so the
/// answer stays a pure function of its inputs and can be tested with fixed
/// values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefusalEnv {
    pub home: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
}

impl RefusalEnv {
    /// Read `$HOME` and the process working directory.
    ///
    /// Both are canonicalized by the caller before they reach [`refuse_reason`]
    /// (the target path is canonicalized too), so a relative target cannot
    /// dodge the comparison (plan-architect A5).
    pub fn from_process() -> Self {
        Self {
            home: dirs::home_dir(),
            cwd: std::env::current_dir().ok(),
        }
    }
}

/// Why a folder may not be renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryRefusal {
    /// The volume root (`/`) itself.
    VolumeRoot,
    /// A directory macOS reserves at the volume root (`/System`, `/usr`, ...).
    SystemRoot,
    /// The home directory itself, or one of its ancestors.
    Home,
    /// The working directory, or one of its ancestors.
    WorkingDirectory,
    /// An app-managed container (`Foo.app`, `Photos.photoslibrary`, ...).
    Bundle,
    /// Inside a macOS library tree (`~/Library/**`).
    Library,
    /// An OS-reserved area (trash, Spotlight index, volume metadata).
    SystemArea,
    /// The folder itself is named like a cache or temporary area.
    CacheOrTemp,
    /// A hidden folder (its own name starts with `.`).
    Hidden,
}

impl std::fmt::Display for DirectoryRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::VolumeRoot => "the volume root cannot be renamed",
            Self::SystemRoot => "a macOS system directory cannot be renamed",
            Self::Home => "the home directory (or one of its parents) cannot be renamed",
            Self::WorkingDirectory => {
                "this is the working directory (or one of its parents); run kaseifu from \
                 somewhere else"
            }
            Self::Bundle => {
                "an app-managed container (an application or library folder) is not a \
                             rename target"
            }
            Self::Library => {
                "folders inside the macOS Library tree are managed by apps, not renamed"
            }
            Self::SystemArea => {
                "this is an area macOS keeps for itself (trash, index, volume data)"
            }
            Self::CacheOrTemp => "folders inside a cache or temporary area are not renamed",
            Self::Hidden => "a hidden folder is not a rename target",
        };
        write!(f, "{reason}")
    }
}

/// Why `dir` may not be renamed, or `None` when it may.
///
/// The rules follow the destination authority for the shapes that are equally
/// unsafe as a rename source (bundles, macOS library trees, OS areas, hidden
/// folders), and diverge in two deliberate ways: the system roots are refused
/// **themselves** (and the volume root, and the home directory with its
/// ancestors, and the working directory with its ancestors) while a folder
/// *inside* a system root is allowed. The destination policy refuses the whole
/// `/var/**` and `/tmp/**` subtree, which would also refuse the folder macOS
/// hands out as `$TMPDIR` (`/var/folders/<xx>/<yy>/T/...`) and every scratch
/// folder a build or test run creates. Renaming such a folder touches nothing
/// but the user's own scratch data, so it stays allowed here. Likewise only
/// the folder's own name refuses a rename for cache and temporary areas
/// ([`is_cache_or_temp_name`]): the destination rule still refuses the whole
/// subtree as a move target.
pub fn refuse_reason(dir: &Path, env: &RefusalEnv) -> Option<DirectoryRefusal> {
    let text = dir.to_string_lossy();
    if text == "/" {
        return Some(DirectoryRefusal::VolumeRoot);
    }
    let components: Vec<&str> = text.split('/').filter(|part| !part.is_empty()).collect();
    let Some(name) = components.last() else {
        return Some(DirectoryRefusal::VolumeRoot);
    };
    if components.len() == 1 && is_protected_root_name(name) {
        return Some(DirectoryRefusal::SystemRoot);
    }
    if env
        .home
        .as_deref()
        .is_some_and(|home| home.starts_with(dir))
    {
        return Some(DirectoryRefusal::Home);
    }
    if env.cwd.as_deref().is_some_and(|cwd| cwd.starts_with(dir)) {
        return Some(DirectoryRefusal::WorkingDirectory);
    }
    if components.iter().any(|part| is_bundle_container(part)) {
        return Some(DirectoryRefusal::Bundle);
    }
    if is_library_tree(&components) {
        return Some(DirectoryRefusal::Library);
    }
    if is_system_area(&components) {
        return Some(DirectoryRefusal::SystemArea);
    }
    // Only the folder's own name refuses the rename here: an ancestor called
    // `Cache` does not make this folder app-managed, and the destination rule
    // (`is_cache_or_temp`) still refuses the whole subtree as a move target.
    if is_cache_or_temp_name(name) {
        return Some(DirectoryRefusal::CacheOrTemp);
    }
    if is_hidden_name(name) {
        return Some(DirectoryRefusal::Hidden);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kaseifu-ctx-dir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn facts_count_sort_and_sample() {
        let dir = work_dir("facts");
        for name in [
            "invoice_002.pdf",
            "invoice_001.PDF",
            "contract.pdf",
            "payment.xlsx",
            "README",
            "archive",
        ] {
            if name == "archive" {
                std::fs::create_dir_all(dir.join(name)).expect("child folder");
            } else {
                std::fs::write(dir.join(name), b"x").expect("child file");
            }
        }
        std::fs::write(dir.join(".DS_Store"), b"x").expect("noise file");
        let facts = read_directory_facts(&dir).expect("facts");
        assert_eq!(facts.total_entries, 6, "noise must not be counted");
        assert_eq!(facts.file_count, 5);
        assert_eq!(facts.directory_count, 1);
        assert!(!facts.truncated);
        assert_eq!(facts.folders, vec!["archive".to_string()]);
        assert_eq!(
            facts.files,
            vec![
                "README".to_string(),
                "contract.pdf".to_string(),
                "invoice_001.PDF".to_string(),
                "invoice_002.pdf".to_string(),
                "payment.xlsx".to_string(),
            ]
        );
        assert_eq!(
            facts.extensions,
            vec![
                ExtensionCount {
                    extension: "pdf".to_string(),
                    count: 3,
                },
                ExtensionCount {
                    extension: "xlsx".to_string(),
                    count: 1,
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn samples_are_bounded_and_truncation_is_detected() {
        let dir = work_dir("bounds");
        for index in 0..(MAX_DIRECTORY_ENTRIES + 20) {
            std::fs::write(dir.join(format!("f{index:04}.txt")), b"x").expect("child file");
        }
        let facts = read_directory_facts(&dir).expect("facts");
        assert_eq!(facts.total_entries, MAX_DIRECTORY_ENTRIES);
        assert!(facts.truncated, "220 children must report the read cap");
        assert_eq!(facts.file_count, MAX_DIRECTORY_ENTRIES);
        assert_eq!(facts.files.len(), DirectoryFacts::MAX_FILES);
        assert_eq!(
            facts.extensions,
            vec![ExtensionCount {
                extension: "txt".to_string(),
                count: MAX_DIRECTORY_ENTRIES,
            }],
            "the distribution counts every entry that was read"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_names_are_cut_to_the_budget() {
        let dir = work_dir("names");
        let long = format!("{}.txt", "n".repeat(120));
        std::fs::write(dir.join(&long), b"x").expect("child file");
        let facts = read_directory_facts(&dir).expect("facts");
        assert_eq!(facts.files.len(), 1);
        assert_eq!(
            facts.files[0].chars().count(),
            DirectoryFacts::MAX_NAME_CHARS
        );
        assert!(!facts.truncated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_or_missing_path_is_an_error() {
        let missing = work_dir("missing").join("nope");
        assert!(read_directory_facts(&missing).is_err());
    }

    #[test]
    fn empty_after_exclusions_is_empty() {
        let dir = work_dir("empty");
        std::fs::write(dir.join(".DS_Store"), b"x").expect("noise file");
        let facts = read_directory_facts(&dir).expect("facts");
        assert!(facts.is_empty());
        assert_eq!(facts.total_entries, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_is_a_folder_whatever_its_name() {
        let dir = work_dir("named");
        std::fs::create_dir_all(dir.join("notes.png")).expect("folder");
        std::fs::write(dir.join("notes.txt"), b"x").expect("file");
        assert!(is_directory_target(&dir.join("notes.png")));
        assert!(!is_directory_target(&dir.join("notes.txt")));
        assert!(!is_directory_target(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_path_rules_match_the_destination_authority() {
        assert!(is_bundle_container("App.app"));
        assert!(is_bundle_container("Photos.photoslibrary"));
        assert!(!is_bundle_container("mylibrary"));
        assert!(!is_bundle_container("notes.app-backup"));
        assert!(is_library_tree(&["Users", "me", "Library", "Caches"]));
        assert!(is_library_tree(&["Volumes", "Backup", "Library"]));
        assert!(!is_library_tree(&["Users", "me", "Documents", "Library"]));
        assert!(is_cache_or_temp(&["Users", "me", "dev", ".cache", "img"]));
        assert!(is_cache_or_temp(&["Users", "me", "Pictures", "Caches"]));
        assert!(is_system_area(&["Users", "me", ".Trash", "old"]));
        assert!(is_hidden_name(".config"));
        assert!(!is_hidden_name("."));
        assert!(!is_hidden_name("visible"));
        assert!(is_system_tree_name("usr"));
        assert!(is_system_tree_name("SYSTEM"));
        assert!(!is_system_tree_name("var-backup"));
        assert!(
            !is_system_tree_name("users"),
            "the destination list must not grow: /Users/<me> stays a destination"
        );
        assert!(is_protected_root_name("Users"));
        assert!(is_protected_root_name("Applications"));
        assert!(!is_protected_root_name("var-backup"));
    }

    #[test]
    fn refusals_cover_the_dangerous_shapes() {
        let home = PathBuf::from("/Users/test");
        let env = RefusalEnv {
            home: Some(home.clone()),
            cwd: Some(home.join("dev/repo")),
        };
        let refused = |path: &str| refuse_reason(Path::new(path), &env);
        assert_eq!(refused("/"), Some(DirectoryRefusal::VolumeRoot));
        assert_eq!(refused("/System"), Some(DirectoryRefusal::SystemRoot));
        assert_eq!(refused("/tmp"), Some(DirectoryRefusal::SystemRoot));
        assert_eq!(
            refused("/Users"),
            Some(DirectoryRefusal::SystemRoot),
            "the root itself is refused as an OS root before the home rule"
        );
        assert_eq!(
            refused("/Users/test"),
            Some(DirectoryRefusal::Home),
            "the home directory itself"
        );
        assert_eq!(
            refused("/Users/test/dev"),
            Some(DirectoryRefusal::WorkingDirectory),
            "an ancestor of the working directory"
        );
        assert_eq!(
            refused("/Users/test/dev/repo"),
            Some(DirectoryRefusal::WorkingDirectory),
            "the working directory itself"
        );
        assert_eq!(
            refused("/Users/test/Downloads/Installer.pkg"),
            Some(DirectoryRefusal::Bundle)
        );
        assert_eq!(
            refused("/Users/test/Library/Application Support/Tool/cache"),
            Some(DirectoryRefusal::Library)
        );
        assert_eq!(
            refused("/Volumes/Archive/.Spotlight-V100"),
            Some(DirectoryRefusal::SystemArea)
        );
        assert_eq!(
            refused("/Volumes/Work/TemporaryItems"),
            Some(DirectoryRefusal::CacheOrTemp)
        );
        assert_eq!(
            refused("/Users/test/Downloads/.hidden"),
            Some(DirectoryRefusal::Hidden)
        );
    }

    #[test]
    fn user_folders_stay_renameable_including_scratch_trees() {
        let env = RefusalEnv {
            home: Some(PathBuf::from("/Users/test")),
            cwd: Some(PathBuf::from("/Users/test/dev/repo")),
        };
        for path in [
            "/Users/test/Documents/\u{65b0}\u{3057}\u{3044}\u{30d5}\u{30a9}\u{30eb}\u{30c0}",
            "/tmp/scratch/new-folder",
            "/var/folders/7t/xt8rcl791xzb8qnjnrhbx1tr0000gn/T/kaseifu/work",
            "/Users/test/Documents/Library/images",
            "/Volumes/Work/Projects/Library/Archive",
            "/Users/test/dev/sha256",
            "relative/folder",
        ] {
            assert_eq!(
                refuse_reason(Path::new(path), &env),
                None,
                "must stay renameable: {path}"
            );
        }
    }

    #[test]
    fn a_relative_path_still_cannot_be_an_ancestor_of_the_working_directory() {
        // Canonicalization happens before the comparison (plan-architect A5),
        // so an absolute cwd never matches a relative path here: the caller
        // passes the canonical target. This pins the fallback behaviour.
        let env = RefusalEnv {
            home: Some(PathBuf::from("/Users/test")),
            cwd: Some(PathBuf::from("/tmp/work")),
        };
        assert_eq!(refuse_reason(Path::new("folder"), &env), None);
        assert_eq!(
            refuse_reason(Path::new("/tmp/work"), &env),
            Some(DirectoryRefusal::WorkingDirectory)
        );
    }

    #[test]
    fn only_the_folders_own_name_refuses_a_cache_rename() {
        let env = RefusalEnv {
            home: Some(PathBuf::from("/Users/test")),
            cwd: Some(PathBuf::from("/Users/test/dev/repo")),
        };
        let refused = |path: &str| refuse_reason(Path::new(path), &env);
        // The folder itself is named like a cache area: still refused.
        assert_eq!(
            refused("/tmp/issue-work/Cache"),
            Some(DirectoryRefusal::CacheOrTemp)
        );
        assert_eq!(
            refused("/Volumes/Work/TemporaryItems"),
            Some(DirectoryRefusal::CacheOrTemp)
        );
        // The folder itself keeps its refusal even under a user tree.
        assert_eq!(
            refused("/Users/test/Documents/Cache"),
            Some(DirectoryRefusal::CacheOrTemp)
        );
        // An ancestor called Cache does not make this folder app-managed.
        for path in [
            "/tmp/issue-work/Cache/project",
            "/tmp/issue-work/my-cache/project",
        ] {
            assert_eq!(refused(path), None, "must stay renameable: {path}");
        }
        // The destination rule keeps the whole-subtree shape.
        assert!(is_cache_or_temp(&["Users", "me", "Cache", "project"]));
        assert!(!is_cache_or_temp_name("project"));
    }

    #[test]
    fn refusal_messages_are_ascii_and_specific() {
        for refusal in [
            DirectoryRefusal::VolumeRoot,
            DirectoryRefusal::SystemRoot,
            DirectoryRefusal::Home,
            DirectoryRefusal::WorkingDirectory,
            DirectoryRefusal::Bundle,
            DirectoryRefusal::Library,
            DirectoryRefusal::SystemArea,
            DirectoryRefusal::CacheOrTemp,
            DirectoryRefusal::Hidden,
        ] {
            let text = refusal.to_string();
            assert!(text.is_ascii(), "not ASCII: {text}");
            assert!(!text.contains("\u{2014}"), "em dash in: {text}");
        }
    }
}

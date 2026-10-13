//! v0.5.1 system-directory rule tests (R8).
//!
//! `is_system_directory` used to match substrings (`/var`, `/tmp`,
//! `/system`, `/private`, ...) against the whole hit path, so user folders
//! such as `~/var/photos`, `~/tmp/mail`, `~/system-backup` or
//! `~/private-docs` were silently dropped from the candidate list. The rules
//! now live in `discovery::destinations` and treat only the *first* component
//! of an absolute path as a macOS system tree; this file pins both sides of
//! that boundary, at the predicate and at the candidate level.

use std::path::{Path, PathBuf};

use kaseifu::discovery::{aggregate_by_directory, discover_with, is_excluded_destination};
use kaseifu::models::SimilarFile;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixture_root(tag: &str) -> PathBuf {
    // Not `std::env::temp_dir()`: on macOS that resolves under `/var`, which
    // discovery filters out as a system directory, so hits placed there would
    // never become candidates.
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.push("target");
    root.push(format!("v051-fixture-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("fixture root");
    root
}

fn write_file(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("fixture dir");
    let path = dir.join(name);
    std::fs::write(&path, b"fixture").expect("fixture file");
    path
}

fn context_for(path: &Path) -> NormalizedFileContext {
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    NormalizedFileContext {
        target: TargetFile {
            path: path.to_path_buf(),
            filename,
            extension: Some("jpeg".to_string()),
            size_bytes: 2_390_044,
            modified_at: None,
        },
        semantic_signals: vec![
            "K:JPEG".to_string(),
            "K:image".to_string(),
            "F:IMG".to_string(),
            "F:3020".to_string(),
        ],
        parent_directory: path
            .parent()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default(),
        ..Default::default()
    }
}

fn discover_with_hits(
    normalized: NormalizedFileContext,
    hits: Vec<String>,
) -> Vec<CandidateDestination> {
    discover_with(normalized, move |_query, _limit| Ok(hits.clone()))
        .expect("discover_with injectable search")
}

fn similar_in(dir: &str, file: &str, similarity: f64) -> SimilarFile {
    SimilarFile {
        path: PathBuf::from(format!("{dir}/{file}")),
        parent_directory: dir.to_string(),
        matched_terms: vec!["IMG".to_string()],
        similarity_score: similarity,
        filename_similarity: similarity,
        token_similarity: 0.3,
        extension_match: 1.0,
        modified_at: None,
    }
}

// ---------------------------------------------------------------------------
// Predicate level: the over-exclusion boundary
// ---------------------------------------------------------------------------

#[test]
fn v051_system_root_trees_are_excluded_user_names_are_not() {
    for dir in [
        "/System/Library/CoreServices",
        "/System/Volumes/Data",
        "/usr/share/icons",
        "/private/var/folders/xx/yy/T",
        "/var/log",
        "/tmp/downloads",
        "/bin",
        "/sbin",
    ] {
        assert!(
            is_excluded_destination(dir),
            "macOS system tree must stay excluded: {dir}"
        );
    }

    for dir in [
        "/Users/test/var/photos",
        "/Users/test/tmp/mail",
        "/Users/test/system-backup",
        "/Users/test/system-backup/2024",
        "/Users/test/private-docs",
        "/Users/test/private",
        "/Volumes/Work/usr/manuals",
        "/Volumes/Archive/System",
        "var/photos",
        "tmp/mail",
    ] {
        assert!(
            !is_excluded_destination(dir),
            "user folder must stay a destination: {dir}"
        );
    }
}

// ---------------------------------------------------------------------------
// Discovery level: user folders reach the candidate list
// ---------------------------------------------------------------------------

#[test]
fn v051_user_folders_reusing_system_names_reach_the_candidate_list() {
    let root = fixture_root("system-names");
    let target = write_file(&root.join("inbox"), "IMG_3020.jpeg");

    let user_dirs = [
        root.join("var").join("photos"),
        root.join("tmp").join("mail"),
        root.join("system-backup"),
        root.join("private-docs"),
    ];
    let hits: Vec<String> = user_dirs
        .iter()
        .map(|dir| {
            write_file(dir, "IMG_3021.jpeg")
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let candidates = discover_with_hits(context_for(&target), hits);

    let mut found: Vec<PathBuf> = candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect();
    found.sort();
    let mut expected = user_dirs.to_vec();
    expected.sort();
    assert_eq!(
        found, expected,
        "every user folder must stay a candidate (R8 regression)"
    );
}

// ---------------------------------------------------------------------------
// Aggregation level: system trees never become candidates
// ---------------------------------------------------------------------------

#[test]
fn v051_system_trees_are_dropped_at_the_aggregation_gate() {
    let files = vec![
        similar_in("/usr/share/icons", "IMG_1.png", 0.9),
        similar_in("/private/var/folders/x/y", "IMG_2.png", 0.8),
        similar_in("/System/Library/CoreServices", "IMG_3.png", 0.7),
        similar_in("/Users/test/var/photos", "IMG_4.png", 0.6),
    ];

    let candidates = aggregate_by_directory(files).expect("aggregate");

    assert_eq!(
        candidates.len(),
        1,
        "system trees are not destinations: {:?}",
        candidates
            .iter()
            .map(|candidate| &candidate.path)
            .collect::<Vec<_>>()
    );
    assert_eq!(candidates[0].path, PathBuf::from("/Users/test/var/photos"));
}

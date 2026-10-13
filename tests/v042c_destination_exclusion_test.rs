//! v0.4.2c destination-eligibility tests.
//!
//! Discovery must never offer a directory the user could not have picked as a
//! move destination: app-internal stores (`*.library`, `*.app`), macOS
//! library trees (`~/Library`), hidden destinations, cache and OS-reserved
//! areas, and machine-generated folder names. The rules live in
//! `kaseifu::discovery::is_excluded_destination`; these tests pin their
//! effect on the candidate list.
//!
//! Every test uses the injectable Spotlight boundary or a synthetic
//! `SimilarFile` list, so nothing here touches the real index.

use std::path::{Path, PathBuf};

use kaseifu::discovery::{
    aggregate_by_directory, aggregate_by_directory_with_exclusions, discover_with,
};
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
    root.push(format!("v042c-fixture-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("fixture root");
    root
}

/// Write a real file (discovery skips hits that do not exist on disk).
fn write_file(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("fixture dir");
    let path = dir.join(name);
    std::fs::write(&path, b"fixture").expect("fixture file");
    path
}

/// A photo context shaped like production: Spotlight evidence for an iPhone
/// JPEG plus the file-name signals.
fn photo_context(path: &Path) -> NormalizedFileContext {
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

fn hit_paths(dirs: &[&Path], name: &str) -> Vec<String> {
    dirs.iter()
        .map(|dir| write_file(dir, name).to_string_lossy().to_string())
        .collect()
}

/// A `SimilarFile` shaped like a scored hit inside `dir`.
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
// Discovery level: the rules reach the candidate list
// ---------------------------------------------------------------------------

#[test]
fn v042c_app_store_directories_never_become_candidates() {
    let root = fixture_root("app-store");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");

    // Measured shape: Eagle stores one image per `<hash>.info` folder inside
    // `Eagle.library`; those folders filled the photo candidate list before
    // the fix.
    let store = root
        .join("Pictures")
        .join("Eagle.library")
        .join("images")
        .join("LY2TLHNM2EO5K.info");
    let album = root.join("Pictures").join("albums").join("2024");
    let hits = vec![
        write_file(&store, "IMG_5020.jpg")
            .to_string_lossy()
            .to_string(),
        write_file(&store, "IMG_5020_thumbnail.png")
            .to_string_lossy()
            .to_string(),
        write_file(&album, "IMG_3021.jpeg")
            .to_string_lossy()
            .to_string(),
    ];

    let candidates = discover_with_hits(photo_context(&target), hits);

    assert_eq!(
        candidates.len(),
        1,
        "only the album may remain: {:?}",
        candidates.iter().map(|c| &c.path).collect::<Vec<_>>()
    );
    assert_eq!(candidates[0].path, album);
    assert_eq!(
        candidates[0].id, "D1",
        "surviving candidates keep D-numbering"
    );
}

#[test]
fn v042c_every_excluded_class_is_removed_from_candidates() {
    let root = fixture_root("classes");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");

    let picture = root.join("Pictures");
    let kept = picture.join("albums").join("2024");
    let excluded = [
        picture
            .join("Eagle.library")
            .join("images")
            .join("LY2TLHNM2EO5K.info"),
        picture.join("caches").join("thumbs"),
        picture.join(".Trash"),
        picture.join("export").join("20260925T175534"),
    ];

    let mut hits = hit_paths(&[&kept], "IMG_3021.jpeg");
    for dir in &excluded {
        hits.extend(hit_paths(&[dir.as_path()], "IMG_5020.jpg"));
    }

    let candidates = discover_with_hits(photo_context(&target), hits);

    assert_eq!(candidates.len(), 1, "only the album may remain");
    assert_eq!(candidates[0].path, kept);
}

#[test]
fn v042c_target_parent_directory_stays_excluded() {
    let root = fixture_root("own-parent");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");
    let album = root.join("Camera Roll");

    let hits = vec![
        write_file(&root.join("images"), "IMG_3022.jpeg")
            .to_string_lossy()
            .to_string(),
        write_file(&album, "IMG_3021.jpeg")
            .to_string_lossy()
            .to_string(),
    ];

    let candidates = discover_with_hits(photo_context(&target), hits);

    assert!(
        candidates.iter().all(|c| c.path != root.join("images")),
        "the target's own parent never becomes a destination"
    );
    assert_eq!(candidates[0].path, album);
}

#[test]
fn v042c_hit_file_names_do_not_decide_the_destination() {
    let root = fixture_root("file-names");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");
    let album = root.join("Pictures").join("albums");

    // The evidence file carries a generated name; its directory is what a
    // destination is, so the folder stays a candidate.
    let hits = vec![
        write_file(&album, "e368edc30f1c4d7f88489c955fa6569a.jpg")
            .to_string_lossy()
            .to_string(),
    ];

    let candidates = discover_with_hits(photo_context(&target), hits);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].path, album);
}

// ---------------------------------------------------------------------------
// Aggregation level: authoritative gate, ranking and ID numbering
// ---------------------------------------------------------------------------

#[test]
fn v042c_aggregation_drops_excluded_directories_and_keeps_ranking() {
    let store = "/Users/test/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info";
    let album = "/Users/test/Pictures/albums/2024";
    let inbox = "/Users/test/Desktop/inbox";

    let files = vec![
        // The app store scores highest and must still be dropped.
        similar_in(store, "IMG_5020.jpg", 0.9),
        similar_in(album, "IMG_3021.jpeg", 0.8),
        similar_in(inbox, "IMG_3022.jpeg", 0.4),
    ];

    let candidates = aggregate_by_directory(files).expect("aggregate");

    assert_eq!(
        candidates.len(),
        2,
        "the store directory is not a destination"
    );
    assert_eq!(candidates[0].path, PathBuf::from(album));
    assert_eq!(candidates[0].id, "D1");
    assert_eq!(candidates[1].path, PathBuf::from(inbox));
    assert_eq!(candidates[1].id, "D2");
    assert!(
        (candidates[0].score - 0.8).abs() < 1e-9,
        "scores of surviving directories are unchanged: {}",
        candidates[0].score
    );
}

#[test]
fn v042c_aggregation_returns_nothing_when_every_destination_is_excluded() {
    let files = vec![
        similar_in(
            "/Users/test/Pictures/Eagle.library/images/LG2SE60NHCWDP.info",
            "IMG_3267.png",
            0.9,
        ),
        similar_in(
            "/Users/test/Library/Application Support/Anki2/User 1/collection.media",
            "IMG_3268.png",
            0.8,
        ),
    ];

    let candidates = aggregate_by_directory(files).expect("aggregate");

    assert!(candidates.is_empty(), "no usable destination exists");
}

#[test]
fn v042c_explicit_exclusion_still_applies_next_to_the_rules() {
    let files = vec![similar_in(
        "/Users/test/Pictures/albums/2024",
        "IMG_3021.jpeg",
        0.8,
    )];

    let candidates = aggregate_by_directory_with_exclusions(
        files,
        &["/Users/test/Pictures/albums/2024".to_string()],
    )
    .expect("aggregate");

    assert!(
        candidates.is_empty(),
        "the caller's exclusion list still wins"
    );
}

// ---------------------------------------------------------------------------
// PDF regression: the exact match still wins, its library copy is refused
// ---------------------------------------------------------------------------

#[test]
fn v042c_pdf_exact_match_keeps_ranking_first() {
    let root = fixture_root("pdf-regression");
    let pdf_dir = root.join("inbox");
    let target = write_file(&pdf_dir, "THCO-COA-Non-Detect Updated.pdf");

    let mut context = photo_context(&target);
    context.target.extension = Some("pdf".to_string());
    context.target.filename = "THCO-COA-Non-Detect Updated.pdf".to_string();
    context.semantic_signals = ["Non", "Detect", "THCO", "Updated", "COA"]
        .iter()
        .map(|term| format!("F:{term}"))
        .collect();

    let exact = root.join("State Of Mind Labs");
    let mut hits = hit_paths(&[&root.join("notes")], "THCO notes.pdf");
    hits.push(
        write_file(&exact, "THCO-COA-Non-Detect Updated.pdf")
            .to_string_lossy()
            .to_string(),
    );

    let candidates = discover_with_hits(context, hits);

    assert_eq!(candidates[0].path, exact);
    assert!((candidates[0].score - 1.0).abs() < 1e-9);
    assert!(
        candidates[0]
            .reasons
            .iter()
            .any(|reason| reason == "exact normalized filename"),
        "reasons keep the strong signal: {:?}",
        candidates[0].reasons
    );
}

#[test]
fn v042c_library_copy_of_the_target_is_not_a_candidate() {
    let files = vec![
        similar_in(
            "/Users/test/Library/Application Support/Code/User/workspaceStorage/e368edc30f1c4d7f88489c955fa6569a/chatSessions",
            "THCO-COA-Non-Detect Updated.pdf",
            1.0,
        ),
        similar_in(
            "/Users/test/Documents/State Of Mind Labs",
            "THCO-COA-Non-Detect Updated.pdf",
            1.0,
        ),
    ];

    let candidates = aggregate_by_directory(files).expect("aggregate");

    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].path,
        PathBuf::from("/Users/test/Documents/State Of Mind Labs")
    );
}

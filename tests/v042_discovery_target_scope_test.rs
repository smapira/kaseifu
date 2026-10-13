//! v0.4.2 Discovery target-scope tests.
//!
//! Two behaviours are protected here:
//!
//! 1. Image targets search with a type scope inside the mdfind query itself
//!    (`kMDItemContentTypeTree == 'public.image'` AND file-name conditions),
//!    because a Rust-side filter would only ever see the truncated hit list.
//! 2. Truncation happens after scoring and directory aggregation: the
//!    ranking, not mdfind's index order, decides which candidates survive.
//!
//! Every test uses the injectable Spotlight boundary (in-memory search), so
//! no test touches the real index or the `mdfind` binary.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use kaseifu::discovery::{MAX_CANDIDATE_DESTINATIONS, MAX_SPOTLIGHT_HITS, discover_with};
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::models::file_context::{NormalizedFileContext, TargetFile};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixture_root(tag: &str) -> PathBuf {
    // Not `std::env::temp_dir()`: on macOS that resolves under `/var`,
    // which discovery filters out as a system directory, so hits placed
    // there would never become candidates.
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.push("target");
    root.push(format!("v042-fixture-{tag}-{}", std::process::id()));
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
/// JPEG (kind noise `K:image` included) plus the file-name signals.
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

/// The PDF used by the regression baseline (`State Of Mind Labs` fixture).
fn pdf_context(path: &Path) -> NormalizedFileContext {
    NormalizedFileContext {
        target: TargetFile {
            path: path.to_path_buf(),
            filename: "THCO-COA-Non-Detect Updated.pdf".to_string(),
            extension: Some("pdf".to_string()),
            ..Default::default()
        },
        semantic_signals: ["Non", "Detect", "THCO", "Updated", "COA"]
            .iter()
            .map(|term| format!("F:{term}"))
            .collect(),
        parent_directory: path
            .parent()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default(),
        ..Default::default()
    }
}

/// Capture the query string and limit the injected search receives.
fn captured_query(normalized: NormalizedFileContext, hits: Vec<String>) -> (String, usize) {
    let captured: RefCell<(String, usize)> = RefCell::new((String::new(), 0));
    let result = discover_with(normalized, |query, limit| {
        *captured.borrow_mut() = (query.to_string(), limit);
        Ok(hits.clone())
    });
    result.expect("discover_with injectable search");
    captured.into_inner()
}

fn discover_with_hits(
    normalized: NormalizedFileContext,
    hits: Vec<String>,
) -> Vec<CandidateDestination> {
    discover_with(normalized, move |_query, _limit| Ok(hits.clone()))
        .expect("discover_with injectable search")
}

fn word_set(query: &str) -> Vec<String> {
    let mut words: Vec<String> = query.split(' ').map(ToString::to_string).collect();
    words.sort();
    words
}

// ---------------------------------------------------------------------------
// T1: image targets search with a type scope inside the query
// ---------------------------------------------------------------------------

#[test]
fn v042_image_query_carries_the_content_type_scope_and_name_terms() {
    let root = fixture_root("image-query");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");

    let (query, limit) = captured_query(photo_context(&target), Vec::new());

    assert_eq!(limit, MAX_SPOTLIGHT_HITS, "every hit is scored: {query}");
    assert!(
        query.starts_with("kMDItemContentTypeTree == 'public.image' && ("),
        "image scope must be part of the query: {query}"
    );
    assert!(query.contains("kMDItemFSName == '*IMG*'c"), "{query}");
    assert!(query.contains("kMDItemFSName == '*3020*'c"), "{query}");
    assert!(
        !query.contains("'*image*'"),
        "kind noise must never become a name condition: {query}"
    );
    // The scope is applied by Spotlight itself; no bare word sequence is left.
    assert!(!query.contains("IMG image 3020"), "{query}");
}

#[test]
fn v042_image_query_is_deterministic() {
    let root = fixture_root("image-query-repeat");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");

    let (first, _) = captured_query(photo_context(&target), Vec::new());
    let (second, _) = captured_query(photo_context(&target), Vec::new());

    assert_eq!(first, second, "same input must produce the same query");
}

#[test]
fn v042_image_query_skips_terms_with_query_metacharacters() {
    let root = fixture_root("image-query-unsafe");
    let target = write_file(&root.join("images"), "IMG it's.jpeg");
    let mut normalized = photo_context(&target);
    normalized.semantic_signals = vec!["F:IMG".to_string(), "F:it's".to_string()];

    let (query, _) = captured_query(normalized, Vec::new());

    assert!(query.starts_with("kMDItemContentTypeTree"), "{query}");
    assert!(query.contains("kMDItemFSName == '*IMG*'c"), "{query}");
    assert!(
        !query.contains("it's"),
        "terms that could break the query language are skipped: {query}"
    );
}

#[test]
fn v042_image_without_name_terms_falls_back_to_the_word_query() {
    let root = fixture_root("image-query-fallback");
    let target = write_file(&root.join("images"), "photo.jpeg");
    let mut normalized = photo_context(&target);
    // Only kind noise remains: no usable file-name term exists.
    normalized.semantic_signals = vec!["K:image".to_string()];

    let (query, _) = captured_query(normalized, Vec::new());

    assert_eq!(query, "image", "word query fallback: {query}");
}

// ---------------------------------------------------------------------------
// T2: non-image targets keep the historical word query (PDF regression)
// ---------------------------------------------------------------------------

#[test]
fn v042_pdf_query_stays_word_based() {
    let root = fixture_root("pdf-query");
    let target = write_file(&root, "THCO-COA-Non-Detect Updated.pdf");

    let (query, limit) = captured_query(pdf_context(&target), Vec::new());

    assert_eq!(limit, MAX_SPOTLIGHT_HITS);
    assert!(
        !query.contains("kMDItem"),
        "non-image targets must not gain type predicates: {query}"
    );
    assert_eq!(
        word_set(&query),
        word_set("Non Detect THCO Updated COA"),
        "term set unchanged: {query}"
    );
}

// ---------------------------------------------------------------------------
// T3: truncation happens after aggregation and ranking
// ---------------------------------------------------------------------------

#[test]
fn v042_candidates_beyond_the_old_hit_window_still_rank() {
    let root = fixture_root("hit-window");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");

    // 100 unrelated hits first (mdfind index order), the photo directory last.
    let mut hits = Vec::new();
    for index in 0..100 {
        hits.push(
            write_file(&root.join("logs"), &format!("run-{index}.json"))
                .to_string_lossy()
                .to_string(),
        );
    }
    let photo_dir = root.join("Camera Roll");
    for name in ["IMG_3020.HEIC", "IMG_3021.HEIC", "IMG_2999.HEIC"] {
        hits.push(write_file(&photo_dir, name).to_string_lossy().to_string());
    }

    let candidates = discover_with_hits(photo_context(&target), hits);

    assert!(!candidates.is_empty(), "photo directory must be found");
    assert_eq!(
        candidates[0].path,
        photo_dir,
        "the ranking, not mdfind order, decides: {:?}",
        candidates.iter().map(|c| &c.path).collect::<Vec<_>>()
    );
}

#[test]
fn v042_pdf_exact_match_beyond_the_old_hit_window_still_wins() {
    let root = fixture_root("pdf-regression");
    let target = write_file(&root.join("inbox"), "THCO-COA-Non-Detect Updated.pdf");

    let mut hits = Vec::new();
    for index in 0..60 {
        hits.push(
            write_file(&root.join("notes"), &format!("note-{index}.md"))
                .to_string_lossy()
                .to_string(),
        );
    }
    let exact_dir = root.join("State Of Mind Labs");
    hits.push(
        write_file(&exact_dir, "THCO-COA-Non-Detect Updated.pdf")
            .to_string_lossy()
            .to_string(),
    );

    let candidates = discover_with_hits(pdf_context(&target), hits);

    assert_eq!(candidates[0].path, exact_dir);
    assert!(
        (candidates[0].score - 1.0).abs() < 1e-9,
        "exact match scores 1.0"
    );
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
fn v042_candidate_destinations_are_capped_after_ranking() {
    let root = fixture_root("candidate-cap");
    let target = write_file(&root.join("images"), "IMG_3020.jpeg");

    let total = MAX_CANDIDATE_DESTINATIONS + 20;
    let mut hits = Vec::new();
    for index in 0..total {
        let dir = root.join(format!("roll-{index:03}"));
        hits.push(
            write_file(&dir, "IMG_3020.jpeg")
                .to_string_lossy()
                .to_string(),
        );
    }

    let candidates = discover_with_hits(photo_context(&target), hits);

    assert_eq!(candidates.len(), MAX_CANDIDATE_DESTINATIONS);
    assert_eq!(candidates[0].id, "D1");
    assert!(
        candidates
            .iter()
            .all(|c| c.score >= candidates[MAX_CANDIDATE_DESTINATIONS - 1].score),
        "the returned head is the ranked head"
    );
}

use kaseifu::discovery::{aggregate_by_directory, normalize_name, score_file};
use kaseifu::models::SimilarFile;
use std::path::PathBuf;

const TARGET: &str = "THCO-COA-Non-Detect Updated.pdf";

fn terms() -> Vec<String> {
    vec!["THCO", "COA", "Non", "Detect", "Updated"]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

fn similar(dir: &str, file: &str) -> SimilarFile {
    let path = format!("{}/{}", dir, file);
    let s = score_file(TARGET, Some("pdf"), &path, &terms());
    SimilarFile {
        path: PathBuf::from(&path),
        parent_directory: dir.to_string(),
        matched_terms: s.matched_terms,
        similarity_score: s.score,
        filename_similarity: s.filename_similarity,
        token_similarity: s.token_similarity,
        extension_match: s.extension_match,
        modified_at: None,
    }
}

#[test]
fn normalization_unifies_separators() {
    let a = normalize_name("THCO-COA-Non-Detect Updated.pdf");
    let b = normalize_name("THCO COA Non-Detect Updated.pdf");
    let c = normalize_name("THCO-COA-Non-Detect-Updated.pdf");
    assert_eq!(a.normalized, "thco coa non detect updated");
    assert_eq!(a.normalized, b.normalized);
    assert_eq!(a.normalized, c.normalized);
}

#[test]
fn exact_match_scores_one() {
    let s = score_file(
        TARGET,
        Some("pdf"),
        "/x/THCO-COA-Non-Detect Updated.pdf",
        &terms(),
    );
    assert_eq!(s.filename_similarity, 1.0);
    assert_eq!(s.token_similarity, 1.0);
    assert_eq!(s.extension_match, 1.0);
    assert!((s.score - 1.0).abs() < 1e-9);
}

#[test]
fn extension_match_values() {
    let pdf = score_file(TARGET, Some("pdf"), "/x/other.pdf", &terms());
    let md = score_file(TARGET, Some("pdf"), "/x/other.md", &terms());
    assert_eq!(pdf.extension_match, 1.0);
    assert_eq!(md.extension_match, 0.0);
}

#[test]
fn matched_terms_never_full_path() {
    let s = score_file(
        TARGET,
        Some("pdf"),
        "/some/dir/THCO-COA-Non-Detect Updated.pdf",
        &terms(),
    );
    for t in &s.matched_terms {
        assert!(!t.contains('/'), "matched term must not be a path: {}", t);
    }
    assert!(!s.matched_terms.is_empty());
}

#[test]
fn pdf_candidates_outrank_unrelated_markdown() {
    let files = vec![
        similar("/data/d4", "THCO-COA-Non-Detect Updated.pdf"),
        similar("/data/d5", "THCO-COA-Non-Detect Updated.pdf"),
        similar("/data/d11", "THCO-COA-Non-Detect-Updated.pdf"),
        similar("/data/d15", "weekly-notes.md"),
        similar("/data/d15", "old-report.md"),
    ];
    let candidates = aggregate_by_directory(files).unwrap();
    assert_eq!(candidates.len(), 4);
    for c in &candidates {
        assert!(
            (0.0..=1.0).contains(&c.score),
            "score out of range: {}",
            c.score
        );
    }
    let d15 = candidates
        .iter()
        .find(|c| c.path.to_string_lossy().contains("d15"))
        .unwrap();
    let pdf_best = candidates
        .iter()
        .filter(|c| !c.path.to_string_lossy().contains("d15"))
        .map(|c| c.score)
        .fold(0.0, f64::max);
    assert!(
        pdf_best > d15.score,
        "pdf dirs ({}) must outrank d15 ({})",
        pdf_best,
        d15.score
    );
    assert!((pdf_best - 1.0).abs() < 1e-9);
}

#[test]
fn count_alone_does_not_win() {
    let files = vec![
        similar("/data/one", "THCO-COA-Non-Detect Updated.pdf"),
        similar("/data/many", "a.md"),
        similar("/data/many", "b.md"),
        similar("/data/many", "c.md"),
        similar("/data/many", "d.md"),
    ];
    let candidates = aggregate_by_directory(files).unwrap();
    let one = candidates
        .iter()
        .find(|c| c.path.to_string_lossy().contains("one"))
        .unwrap();
    let many = candidates
        .iter()
        .find(|c| c.path.to_string_lossy().contains("many"))
        .unwrap();
    assert!(one.score > many.score);
}

#[test]
fn tie_order_is_deterministic() {
    let files = vec![
        similar("/data/b-dir", "THCO-COA-Non-Detect Updated.pdf"),
        similar("/data/a-dir", "THCO-COA-Non-Detect Updated.pdf"),
    ];
    let first = aggregate_by_directory(files.clone()).unwrap();
    let second = aggregate_by_directory(files).unwrap();
    assert_eq!(first[0].path, second[0].path);
    assert_eq!(first[0].id, "D1");
    assert!(first[0].path.to_string_lossy().contains("a-dir"));
}

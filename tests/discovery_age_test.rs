use kaseifu::discovery::{aggregate_by_directory, score_file};
use kaseifu::models::SimilarFile;
use std::path::PathBuf;

const TARGET: &str = "THCO-COA-Non-Detect Updated.pdf";

fn terms() -> Vec<String> {
    vec!["THCO", "COA", "Non", "Detect", "Updated"]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

fn rfc3339(days_ago: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dt: chrono::DateTime<chrono::Utc> =
        chrono::DateTime::from_timestamp(now - days_ago * 86_400, 0).unwrap();
    dt.to_rfc3339()
}

fn similar_aged(dir: &str, file: &str, days_ago: Option<i64>) -> SimilarFile {
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
        modified_at: days_ago.map(rfc3339),
    }
}

#[test]
fn older_directory_wins_tie() {
    let files = vec![
        similar_aged("/data/new-dir", TARGET, Some(5)),
        similar_aged("/data/old-dir", TARGET, Some(400)),
    ];
    let candidates = aggregate_by_directory(files).unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates[0].path.to_string_lossy().contains("old-dir"),
        "older dir must rank first: {:?}",
        candidates.iter().map(|c| &c.path).collect::<Vec<_>>()
    );
    assert!(candidates[0].score >= candidates[1].score);
}

#[test]
fn established_reason_present_for_old_dirs() {
    let files = vec![similar_aged("/data/old-dir", TARGET, Some(400))];
    let candidates = aggregate_by_directory(files).unwrap();
    assert!(
        candidates[0]
            .reasons
            .iter()
            .any(|r| r.starts_with("established:")),
        "reasons: {:?}",
        candidates[0].reasons
    );
}

#[test]
fn missing_dates_fall_back_to_path_order() {
    let files = vec![
        similar_aged("/data/b-dir", TARGET, None),
        similar_aged("/data/a-dir", TARGET, None),
    ];
    let first = aggregate_by_directory(files.clone()).unwrap();
    assert!(first[0].path.to_string_lossy().contains("a-dir"));
    assert!(
        first[0]
            .reasons
            .iter()
            .all(|r| !r.starts_with("established:"))
    );
    let second = aggregate_by_directory(files).unwrap();
    assert_eq!(first[0].path, second[0].path);
}

#[test]
fn age_bonus_never_beats_exact_match() {
    let files = vec![
        similar_aged("/data/old-weak", "weekly-notes.md", Some(900)),
        similar_aged("/data/new-exact", TARGET, Some(1)),
    ];
    let candidates = aggregate_by_directory(files).unwrap();
    let top = &candidates[0];
    assert!(
        top.path.to_string_lossy().contains("new-exact"),
        "exact match must win despite age: {:?}",
        candidates.iter().map(|c| &c.path).collect::<Vec<_>>()
    );
}

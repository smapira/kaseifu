use std::path::PathBuf;

use crate::discovery::SimilarFile;
use crate::discovery::scoring::{adjust_for_age, aggregate_directory_score, median_timestamp};
use crate::models::candidate::CandidateDestination;

pub fn aggregate_by_directory(
    similar_files: Vec<SimilarFile>,
) -> anyhow::Result<Vec<CandidateDestination>> {
    aggregate_by_directory_with_exclusions(similar_files, &[])
}

pub fn aggregate_by_directory_with_exclusions(
    similar_files: Vec<SimilarFile>,
    excluded_dirs: &[String],
) -> anyhow::Result<Vec<CandidateDestination>> {
    if similar_files.is_empty() {
        return Ok(Vec::new());
    }

    let mut dir_files: std::collections::HashMap<String, Vec<SimilarFile>> =
        std::collections::HashMap::new();

    for file in similar_files {
        let dir = file.parent_directory.clone();
        if excluded_dirs.iter().any(|e| e == &dir) {
            continue;
        }
        dir_files.entry(dir).or_default().push(file);
    }

    let mut dirs: Vec<_> = dir_files.keys().cloned().collect();
    dirs.sort();

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut scored: Vec<ScoredDir> = Vec::new();
    for dir in dirs {
        let files = dir_files.get(&dir).unwrap();
        let evidence_count = files.len();
        let scores: Vec<f64> = files.iter().map(|f| f.similarity_score).collect();
        let base = aggregate_directory_score(scores);
        let timestamps: Vec<i64> = files.iter().filter_map(parse_mtime).collect();
        let median_ts = median_timestamp(timestamps);
        // Sub-day ages are ignored: avoids "~0 days" noise and keeps
        // fresh fixtures reason-identical. The raw timestamps still
        // order ties older-first.
        let age_days = median_ts
            .map(|ts| (now_unix - ts).max(0) as f64 / 86_400.0)
            .filter(|d| *d >= 1.0);
        let (score, bonus) = adjust_for_age(base, age_days);
        let reasons = build_reasons(files, evidence_count, age_days, bonus);
        crate::debug!(
            "  dir={} base={:.2} age_days={:?} bonus={:.2} score={:.2}",
            dir,
            base,
            age_days.map(|d| d.round() as i64),
            bonus,
            score
        );
        scored.push(ScoredDir {
            dir,
            score,
            evidence_count,
            median_ts,
            reasons,
        });
    }

    // Score desc, then older evidence first, then path asc (deterministic).
    // Directories without dates sort as newest (last among ties).
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let ae = a.median_ts.unwrap_or(i64::MAX);
                let be = b.median_ts.unwrap_or(i64::MAX);
                ae.cmp(&be)
            })
            .then_with(|| a.dir.cmp(&b.dir))
    });

    let candidates: Vec<CandidateDestination> = scored
        .into_iter()
        .enumerate()
        .map(|(idx, s)| CandidateDestination {
            id: format!("D{}", idx + 1),
            path: PathBuf::from(s.dir),
            score: s.score,
            evidence_count: s.evidence_count,
            reasons: s.reasons,
        })
        .collect();

    Ok(candidates)
}

struct ScoredDir {
    dir: String,
    score: f64,
    evidence_count: usize,
    median_ts: Option<i64>,
    reasons: Vec<String>,
}

fn parse_mtime(f: &SimilarFile) -> Option<i64> {
    let s = f.modified_at.as_ref()?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp())
}

fn build_reasons(
    files: &[SimilarFile],
    evidence_count: usize,
    age_days: Option<f64>,
    bonus: f64,
) -> Vec<String> {
    let mut reasons = Vec::new();
    reasons.push(format!("found {} items", evidence_count));
    push_age_reason(age_days, bonus, &mut reasons);
    push_filename_reason(files, &mut reasons);
    push_extension_reason(files, &mut reasons);
    push_term_reasons(files, &mut reasons);
    reasons
}

/// Established-directory note: only when the age bonus actually applied.
fn push_age_reason(age_days: Option<f64>, bonus: f64, reasons: &mut Vec<String>) {
    if bonus > 0.0
        && let Some(days) = age_days
    {
        reasons.push(format!("established: ~{} days", days.round() as i64));
    }
}

/// Strongest filename signal wins: exact match outranks a similarity score.
fn push_filename_reason(files: &[SimilarFile], reasons: &mut Vec<String>) {
    let best_filename = files
        .iter()
        .map(|f| f.filename_similarity)
        .fold(0.0, f64::max);
    if best_filename >= 1.0 {
        reasons.push("exact normalized filename".to_string());
    } else if best_filename >= 0.5 {
        reasons.push(format!("filename similarity {:.2}", best_filename));
    }
}

/// Extension match is reported with the representative extension.
fn push_extension_reason(files: &[SimilarFile], reasons: &mut Vec<String>) {
    let best_ext = files.iter().map(|f| f.extension_match).fold(0.0, f64::max);
    if best_ext >= 1.0
        && let Some(ext) = files.iter().find_map(|f| {
            std::path::Path::new(&f.path)
                .extension()
                .map(|s| s.to_string_lossy().to_lowercase())
        })
    {
        reasons.push(format!("extension: {ext}"));
    }
}

/// Up to five deduplicated search-term matches, sorted for determinism.
fn push_term_reasons(files: &[SimilarFile], reasons: &mut Vec<String>) {
    let mut terms: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in files {
        for t in &f.matched_terms {
            if !t.trim().is_empty() {
                terms.insert(t.clone());
            }
        }
    }
    for t in terms.into_iter().take(5) {
        reasons.push(format!("match: {t}"));
    }
}

pub fn breakdown_line(id: &str, dir: &str, files: &[SimilarFile], score: f64) -> String {
    let mut out = format!("{} — {}\n", id, dir);
    let best = files.iter().max_by(|a, b| {
        a.similarity_score
            .partial_cmp(&b.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(b) = best {
        out.push_str(&format!(
            "Filename similarity  {:.2}\nToken similarity     {:.2}\nExtension match      {:.2}\nEvidence count       {}\nDirectory score      {:.2}\n",
            b.filename_similarity, b.token_similarity, b.extension_match,
            files.len(), score
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_similar_files() {
        let similar_files: Vec<SimilarFile> = vec![];
        let candidates = aggregate_by_directory(similar_files).unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_aggregate_by_directory() {
        let similar_files = vec![
            SimilarFile {
                path: PathBuf::from("/Users/test/file1.png"),
                parent_directory: "/Users/test".to_string(),
                matched_terms: vec!["CBX".to_string()],
                similarity_score: 0.9,
                filename_similarity: 0.9,
                token_similarity: 0.9,
                extension_match: 1.0,
                modified_at: None,
            },
            SimilarFile {
                path: PathBuf::from("/Users/test/file2.pdf"),
                parent_directory: "/Users/test".to_string(),
                matched_terms: vec![],
                similarity_score: 0.8,
                filename_similarity: 0.8,
                token_similarity: 0.8,
                extension_match: 1.0,
                modified_at: None,
            },
            SimilarFile {
                path: PathBuf::from("/Users/docs/doc1.txt"),
                parent_directory: "/Users/docs".to_string(),
                matched_terms: vec![],
                similarity_score: 0.1,
                filename_similarity: 0.1,
                token_similarity: 0.1,
                extension_match: 0.0,
                modified_at: None,
            },
        ];

        let candidates = aggregate_by_directory(similar_files).unwrap();

        assert_eq!(candidates.len(), 2);

        // Check that IDs are sequential
        let first_id = &candidates[0].id;
        let second_id = &candidates[1].id;

        println!("Candidate IDs: {} and {}", first_id, second_id);

        // ID format should be D1, D2, etc.
        assert!(first_id.starts_with("D"));
        assert!(second_id.starts_with("D"));
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn test_score_ordering() {
        let similar_files = vec![
            // D1: 2 files with 2 matched terms
            SimilarFile {
                path: PathBuf::from("/Users/test1/f1.png"),
                parent_directory: "/Users/test1".to_string(),
                matched_terms: vec!["A".to_string(), "B".to_string()],
                similarity_score: 0.9,
                filename_similarity: 0.9,
                token_similarity: 0.9,
                extension_match: 1.0,
                modified_at: None,
            },
            SimilarFile {
                path: PathBuf::from("/Users/test1/f2.pdf"),
                parent_directory: "/Users/test1".to_string(),
                matched_terms: vec!["C".to_string()],
                similarity_score: 0.7,
                filename_similarity: 0.7,
                token_similarity: 0.7,
                extension_match: 1.0,
                modified_at: None,
            },
            // D2: 1 file with 0 matched terms
            SimilarFile {
                path: PathBuf::from("/Users/test2/f1.pdf"),
                parent_directory: "/Users/test2".to_string(),
                matched_terms: vec![],
                similarity_score: 0.1,
                filename_similarity: 0.1,
                token_similarity: 0.1,
                extension_match: 0.0,
                modified_at: None,
            },
        ];

        let candidates = aggregate_by_directory(similar_files).unwrap();

        // D1 should have higher score than D2
        assert!(candidates[0].score >= candidates[1].score);
        assert_eq!(candidates[0].evidence_count, 2);
        assert_eq!(candidates[1].evidence_count, 1);
    }
}

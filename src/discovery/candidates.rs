use std::path::PathBuf;

use crate::discovery::SimilarFile;
use crate::discovery::scoring::aggregate_directory_score;
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

    let mut scored: Vec<(String, f64, usize, Vec<String>)> = Vec::new();
    for dir in dirs {
        let files = dir_files.get(&dir).unwrap();
        let evidence_count = files.len();
        let scores: Vec<f64> = files.iter().map(|f| f.similarity_score).collect();
        let score = aggregate_directory_score(scores);
        let reasons = build_reasons(files, evidence_count);
        scored.push((dir, score, evidence_count, reasons));
    }

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let candidates: Vec<CandidateDestination> = scored
        .into_iter()
        .enumerate()
        .map(
            |(idx, (dir, score, evidence_count, reasons))| CandidateDestination {
                id: format!("D{}", idx + 1),
                path: PathBuf::from(dir),
                score,
                evidence_count,
                reasons,
            },
        )
        .collect();

    Ok(candidates)
}

fn build_reasons(files: &[SimilarFile], evidence_count: usize) -> Vec<String> {
    let mut reasons = Vec::new();
    reasons.push(format!("found {} items", evidence_count));
    let best_filename = files
        .iter()
        .map(|f| f.filename_similarity)
        .fold(0.0, f64::max);
    let best_ext = files.iter().map(|f| f.extension_match).fold(0.0, f64::max);
    if best_filename >= 1.0 {
        reasons.push("exact normalized filename".to_string());
    } else if best_filename >= 0.5 {
        reasons.push(format!("filename similarity {:.2}", best_filename));
    }
    if best_ext >= 1.0
        && let Some(ext) = files.iter().find_map(|f| {
            std::path::Path::new(&f.path)
                .extension()
                .map(|s| s.to_string_lossy().to_lowercase())
        })
    {
        reasons.push(format!("extension: {}", ext));
    }
    let mut terms: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in files {
        for t in &f.matched_terms {
            if !t.trim().is_empty() {
                terms.insert(t.clone());
            }
        }
    }
    for t in terms.into_iter().take(5) {
        reasons.push(format!("match: {}", t));
    }
    reasons
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
            },
            SimilarFile {
                path: PathBuf::from("/Users/test/file2.pdf"),
                parent_directory: "/Users/test".to_string(),
                matched_terms: vec![],
                similarity_score: 0.8,
                filename_similarity: 0.8,
                token_similarity: 0.8,
                extension_match: 1.0,
            },
            SimilarFile {
                path: PathBuf::from("/Users/docs/doc1.txt"),
                parent_directory: "/Users/docs".to_string(),
                matched_terms: vec![],
                similarity_score: 0.1,
                filename_similarity: 0.1,
                token_similarity: 0.1,
                extension_match: 0.0,
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
            },
            SimilarFile {
                path: PathBuf::from("/Users/test1/f2.pdf"),
                parent_directory: "/Users/test1".to_string(),
                matched_terms: vec!["C".to_string()],
                similarity_score: 0.7,
                filename_similarity: 0.7,
                token_similarity: 0.7,
                extension_match: 1.0,
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
            },
        ];

        let candidates = aggregate_by_directory(similar_files).unwrap();

        // D1 should have higher score than D2
        assert!(candidates[0].score >= candidates[1].score);
        assert_eq!(candidates[0].evidence_count, 2);
        assert_eq!(candidates[1].evidence_count, 1);
    }
}

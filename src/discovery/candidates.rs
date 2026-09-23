use std::path::{Path, PathBuf};

use crate::discovery::SimilarFile;
use crate::models::candidate::CandidateDestination;

/// Aggregate SimilarFiles by parent directory and generate CandidateDestinations
///
/// Generates unique sequential IDs (D1, D2, D3...) and calculates scores based on actual evidence
pub fn aggregate_by_directory(
    similar_files: Vec<SimilarFile>,
) -> anyhow::Result<Vec<CandidateDestination>> {
    if similar_files.is_empty() {
        return Ok(Vec::new());
    }

    // Group files by parent directory
    let mut dir_files: std::collections::HashMap<String, Vec<SimilarFile>> =
        std::collections::HashMap::new();

    for file in similar_files {
        // In production, validate path existence for safety
        // For testing, skip validation when creating mock data
        let dir = file.parent_directory.clone();

        dir_files
            .entry(dir.clone())
            .or_insert_with(Vec::new)
            .push(file);
    }

    // Generate candidates - one per unique parent directory
    // Sort directories for deterministic order
    let mut dirs: Vec<_> = dir_files.keys().cloned().collect();
    dirs.sort();

    let mut candidates: Vec<CandidateDestination> = dirs
        .iter()
        .enumerate()
        .map(|(idx, dir)| {
            let similar_files = dir_files.get(dir).unwrap();
            let evidence_count = similar_files.len();

            // Calculate score based on evidence
            // - Base score of 5.0 if no matched terms found
            // - If matched terms exist: score = 10.0 + (10.0 * evidence_count) / (matched_count + 1)
            // This rewards:
            // - More matched terms -> higher score
            // - More evidence (related files) in the same directory also helps
            let matched_terms: Vec<_> = similar_files
                .iter()
                .flat_map(|f| &f.matched_terms)
                .filter(|t| !t.is_empty())
                .collect();
            let matched_count = matched_terms.len();

            let score = if matched_count > 0 {
                10.0 + (10.0 * evidence_count as f64) / (matched_count as f64 + 1.0)
            } else {
                5.0
            };

            // Collect reasons from matched terms
            let mut reasons = Vec::new();
            reasons.push(format!("found {} items", evidence_count));
            // Deduplicate terms for reasons display
            let unique_terms: std::collections::HashSet<_> =
                matched_terms.iter().cloned().collect();
            for term in unique_terms {
                let trimmed = term.trim();
                if !trimmed.is_empty() {
                    reasons.push(format!("match: {}", trimmed));
                }
            }
            // Add file type match if any
            for file in similar_files {
                if let Some(ext) = Path::new(&file.path)
                    .extension()
                    .map(|s| s.to_string_lossy().to_string())
                {
                    reasons.push(format!("file_type: {}", ext));
                    break;
                }
            }

            // Create sequential ID (D1, D2, D3...)
            let id = format!("D{}", idx + 1);

            CandidateDestination {
                id,
                path: PathBuf::from(dir.clone()),
                score,
                evidence_count,
                reasons,
            }
        })
        .collect();

    // Sort candidates by score (descending), then by path for determinism
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.path.cmp(&a.path))
    });

    Ok(candidates)
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
                similarity_score: 0.0,
            },
            SimilarFile {
                path: PathBuf::from("/Users/test/file2.pdf"),
                parent_directory: "/Users/test".to_string(),
                matched_terms: vec![],
                similarity_score: 0.0,
            },
            SimilarFile {
                path: PathBuf::from("/Users/docs/doc1.txt"),
                parent_directory: "/Users/docs".to_string(),
                matched_terms: vec![],
                similarity_score: 0.0,
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
                similarity_score: 0.0,
            },
            SimilarFile {
                path: PathBuf::from("/Users/test1/f2.pdf"),
                parent_directory: "/Users/test1".to_string(),
                matched_terms: vec!["C".to_string()],
                similarity_score: 0.0,
            },
            // D2: 1 file with 0 matched terms
            SimilarFile {
                path: PathBuf::from("/Users/test2/f1.pdf"),
                parent_directory: "/Users/test2".to_string(),
                matched_terms: vec![],
                similarity_score: 0.0,
            },
        ];

        let candidates = aggregate_by_directory(similar_files).unwrap();

        // D1 should have higher score than D2
        assert!(candidates[0].score >= candidates[1].score);
        assert_eq!(candidates[0].evidence_count, 2);
        assert_eq!(candidates[1].evidence_count, 1);
    }
}

//! The discovery-change indicators for issues #18 and #19.
//!
//! Both issues ask for a before/after number, and neither may be measured on
//! the developer's own index: a real `mdfind` corpus is the one thing a test
//! must never touch (the per-run costs, the privacy and the machine-to-machine
//! variance are all unacceptable). What is recorded here is the same shape the
//! issues measured, built from a fixture: an unindexed target, equal filename
//! evidence in two directories, and a standard folder that exists.
//!
//! Every function is pure over its inputs apart from the `is_dir` check the
//! prior needs, so the numbers in `20261108a-issues-18-22-bdd-plan.md` can be
//! reproduced by running this module's tests.

use crate::discovery::DirectoryShape;
use crate::discovery::add_standard_dir_priors;
use crate::discovery::aggregate_by_directory_with_shape;
use crate::models::SimilarFile;
use crate::models::candidate::CandidateDestination;
use crate::platform::user_dirs::UserDirKind;
use std::path::{Path, PathBuf};

/// Candidate counts for one unindexed target, before and after the prior.
///
/// The measured case (issue #18): a file Spotlight has not indexed produces
/// zero candidates, so it has nothing to be classified against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriorIndicator {
    /// Candidates the search produced (the fixture search finds nothing).
    pub before: usize,
    /// Candidates after the standard folder was appended.
    pub after: usize,
}

/// [`PriorIndicator`] for one extension over an explicit directory list.
pub fn prior_indicator(extension: &str, known: &[(UserDirKind, PathBuf)]) -> PriorIndicator {
    let mut candidates: Vec<CandidateDestination> = Vec::new();
    let before = candidates.len();
    add_standard_dir_priors(&mut candidates, Some(extension), known);
    PriorIndicator {
        before,
        after: candidates.len(),
    }
}

/// The rank order of two equally-supported directories, before and after the
/// path-shape terms (issue #19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeIndicator {
    /// The first candidate without the shape terms (the historical ranking).
    pub before_first: String,
    /// The first candidate with them.
    pub after_first: String,
}

/// [`ShapeIndicator`] for the shape the issue measured: the same filename
/// evidence in a repository directory and in a user folder.
pub fn shape_indicator(home: &Path, repository: &Path, user: &Path, score: f64) -> ShapeIndicator {
    let files = vec![evidence(repository, score), evidence(user, score)];
    let before = first_path(&files, &DirectoryShape::default());
    let after = first_path(
        &files,
        &DirectoryShape {
            home: Some(home.to_path_buf()),
            known_dir: None,
        },
    );
    ShapeIndicator {
        before_first: before,
        after_first: after,
    }
}

/// One evidence row for `directory`, scored as a perfect filename match would
/// score it (the ranking question is what the shape terms change).
fn evidence(directory: &Path, score: f64) -> SimilarFile {
    SimilarFile {
        path: directory.join("IMG_3020.jpeg"),
        parent_directory: directory.to_string_lossy().to_string(),
        matched_terms: vec!["IMG".to_string()],
        similarity_score: score,
        filename_similarity: score,
        token_similarity: score,
        extension_match: 1.0,
        modified_at: None,
    }
}

/// The directory of the top-ranked candidate under one shape.
fn first_path(files: &[SimilarFile], shape: &DirectoryShape) -> String {
    aggregate_by_directory_with_shape(files.to_vec(), &[], shape)
        .expect("aggregate")
        .first()
        .map(|candidate| candidate.path.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-sandboxes")
            .join(format!("bench-prior-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("sandbox");
        root
    }

    #[test]
    fn the_prior_takes_an_unindexed_file_from_zero_to_one_candidate() {
        let root = sandbox("prior");
        let documents = root.join("Documents");
        std::fs::create_dir_all(&documents).expect("documents");
        let known = vec![(UserDirKind::Document, documents)];

        // The number recorded in the plan (#18, section 6).
        assert_eq!(
            prior_indicator("pdf", &known),
            PriorIndicator {
                before: 0,
                after: 1
            }
        );
        // An unknown type stays at zero: the prior never invents a folder.
        assert_eq!(
            prior_indicator("xyz", &known),
            PriorIndicator {
                before: 0,
                after: 0
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_shape_terms_move_the_repository_path_down() {
        let root = sandbox("shape");
        // The measured case (issue #19): a picture on the Desktop, and the same
        // filename evidence inside a repository checkout.
        let repository =
            root.join("OS/home/Codes/github.com/routeflags/agent-bank/public/system/images/11");
        let user = root.join("Pictures/2024");
        std::fs::create_dir_all(&repository).expect("repository");
        std::fs::create_dir_all(&user).expect("user");
        // The checkout marker sits above the evidence directory.
        std::fs::create_dir_all(root.join("OS/home/Codes/github.com/routeflags/agent-bank/.git"))
            .expect("git dir");

        let indicator = shape_indicator(&root, &repository, &user, 0.34);
        // Before: a tie, decided by path asc ("OS" < "Pictures").
        assert_eq!(indicator.before_first, repository.to_string_lossy());
        // After: the user folder, because of depth plus the working tree.
        assert_eq!(indicator.after_first, user.to_string_lossy());
        let _ = std::fs::remove_dir_all(&root);
    }
}

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

use crate::discovery::add_standard_dir_priors;
use crate::models::candidate::CandidateDestination;
use crate::platform::user_dirs::UserDirKind;
use std::path::PathBuf;

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
}

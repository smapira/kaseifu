#![allow(clippy::bool_assert_comparison)]
//! v0.13.2 issue #19: the Spotlight scope and the path-shape terms.
//!
//! Measured: `mdfind` searched the whole index, so a picture on the Desktop
//! ranked a repository checkout first, and the ranking had no term for depth,
//! "inside a worktree" or "this is the standard folder".
//!
//! No test here runs `mdfind`: the scope is asserted on the argument vector and
//! on an injected runner, and the ranking is asserted on synthetic evidence.

use kaseifu::discovery::{
    DirectoryShape, adjust_for_shape, aggregate_by_directory_with_shape, depth_below_home,
};
use kaseifu::models::SimilarFile;
use kaseifu::models::candidate::CandidateDestination;
use kaseifu::platform::macos::spotlight;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn sandbox(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-sandboxes")
        .join(format!("v119-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("sandbox");
    root
}

/// One evidence row in `directory`, scored as if the file name matched.
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

fn ranked(files: Vec<SimilarFile>, shape: &DirectoryShape) -> Vec<CandidateDestination> {
    aggregate_by_directory_with_shape(files, &[], shape).expect("aggregate")
}

// ---------------------------------------------------------------------------
// The mdfind invocation is scoped
// ---------------------------------------------------------------------------

#[test]
fn v119_the_default_scope_is_the_home_directory() {
    let home = spotlight::default_roots();
    let Some(home) = home.first() else {
        // No home to scope to: the search runs unscoped, exactly as before.
        let arguments = spotlight::search_arguments("IMG", None);
        assert_eq!(arguments.len(), 1);
        return;
    };
    let arguments = spotlight::search_arguments("IMG", Some(home));
    assert_eq!(arguments[0], "-onlyin");
    assert_eq!(Path::new(&arguments[1]), home.as_path());
    assert_eq!(arguments[2], "IMG");
}

#[test]
fn v119_extra_roots_are_each_scoped() {
    let arguments = spotlight::search_arguments("IMG", Some(Path::new("/Volumes/Archive")));
    assert_eq!(arguments[0], "-onlyin");
    assert_eq!(Path::new(&arguments[1]), Path::new("/Volumes/Archive"));
}

#[test]
fn v119_a_search_runs_once_per_root_and_stays_bounded() {
    let scopes: Mutex<Vec<Option<PathBuf>>> = Mutex::new(Vec::new());
    let roots = vec![PathBuf::from("/data/one"), PathBuf::from("/data/two")];

    let hits = spotlight::search_in_with("IMG", 3, &roots, |_query, scope, limit| {
        scopes
            .lock()
            .expect("scopes")
            .push(scope.map(Path::to_path_buf));
        Ok((0..limit.min(2))
            .map(|index| format!("hit-{index}"))
            .collect())
    })
    .expect("search");

    // One invocation per root, each carrying its scope, and the limit is shared
    // across them (a wide scope cannot spend the whole budget).
    assert_eq!(
        *scopes.lock().expect("scopes"),
        vec![Some(roots[0].clone()), Some(roots[1].clone())]
    );
    assert_eq!(hits.len(), 3);
}

#[test]
fn v119_the_scope_reaches_the_search_the_pipeline_uses() {
    // `--roots` is a closure over the search seam; the seam is what discovery
    // calls, so a hit outside every scope cannot enter the candidate list.
    let seen: Mutex<Vec<Option<PathBuf>>> = Mutex::new(Vec::new());
    let roots = vec![PathBuf::from("/data/one")];
    let search = |query: &str, limit: usize| {
        spotlight::search_in_with(query, limit, &roots, |_q, scope, _l| {
            seen.lock()
                .expect("seen")
                .push(scope.map(Path::to_path_buf));
            Ok(Vec::new())
        })
    };
    let _ = search("IMG", 10).expect("search");
    assert_eq!(seen.lock().expect("seen").len(), 1);
    assert_eq!(
        seen.lock().expect("seen")[0],
        Some(PathBuf::from("/data/one"))
    );
}

// ---------------------------------------------------------------------------
// Path shape: depth, worktrees, the standard folder
// ---------------------------------------------------------------------------

#[test]
fn v119_depth_is_counted_below_the_home() {
    let home = Path::new("/Users/test");
    assert_eq!(depth_below_home("/Users/test/Pictures/2024", Some(home)), 2);
    assert_eq!(depth_below_home("/Users/test", Some(home)), 0);
    // Outside the home the whole component count is the depth.
    assert_eq!(depth_below_home("/Volumes/Archive/photos", Some(home)), 3);
    // Without a home nothing is claimed.
    assert_eq!(depth_below_home("/Users/test/Pictures", None), 0);
}

#[test]
fn v119_depth_penalty_orders_otherwise_equal_directories() {
    let root = sandbox("depth");
    let shallow = root.join("Pictures");
    let deep = root.join("a/b/c/d/e/f/g/h/i/Pictures");
    std::fs::create_dir_all(&shallow).expect("shallow");
    std::fs::create_dir_all(&deep).expect("deep");
    // The home is the sandbox, so the depths are 1 and 11.
    let shape = DirectoryShape {
        home: Some(root.clone()),
        known_dir: None,
    };

    let candidates = ranked(vec![evidence(&deep, 0.5), evidence(&shallow, 0.5)], &shape);
    assert_eq!(
        candidates[0].path, shallow,
        "the shallow folder wins the tie"
    );
    assert!(
        candidates[1]
            .reasons
            .iter()
            .any(|reason| reason.starts_with("depth penalty")),
        "the penalty is reported: {:?}",
        candidates[1].reasons
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v119_a_worktree_path_does_not_outrank_a_user_folder() {
    let root = sandbox("worktree");
    let checkout = root.join("checkout");
    let repository_images = checkout.join("public/system/images/11/email");
    let user_images = root.join("Pictures/2024");
    std::fs::create_dir_all(&repository_images).expect("repo");
    std::fs::create_dir_all(&user_images).expect("user");
    std::fs::create_dir_all(checkout.join(".git")).expect("git dir");
    let shape = DirectoryShape {
        home: Some(root.clone()),
        known_dir: None,
    };

    // Equal filename evidence in both, the shape of the measured case.
    let candidates = ranked(
        vec![
            evidence(&repository_images, 0.4),
            evidence(&user_images, 0.4),
        ],
        &shape,
    );
    assert_eq!(
        candidates[0].path,
        user_images,
        "a worktree path must not outrank a user folder: {:?}",
        candidates
            .iter()
            .map(|c| (&c.path, c.score))
            .collect::<Vec<_>>()
    );
    assert!(
        candidates
            .iter()
            .find(|candidate| candidate.path == repository_images)
            .expect("the repository path stays a candidate")
            .reasons
            .iter()
            .any(|reason| reason == "inside a git working tree"),
        "the worktree reason is reported"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v119_the_standard_folder_gets_the_prior_bonus() {
    let root = sandbox("bonus");
    let pictures = root.join("Pictures");
    let albums = root.join("albums");
    std::fs::create_dir_all(&pictures).expect("pictures");
    std::fs::create_dir_all(&albums).expect("albums");
    let shape = DirectoryShape {
        home: Some(root.clone()),
        known_dir: Some(pictures.clone()),
    };

    let candidates = ranked(
        vec![evidence(&albums, 0.5), evidence(&pictures, 0.5)],
        &shape,
    );
    assert_eq!(
        candidates[0].path, pictures,
        "the standard folder wins the tie"
    );
    assert!(
        candidates[0]
            .reasons
            .iter()
            .any(|reason| reason == "standard folder for this file type"),
        "reasons: {:?}",
        candidates[0].reasons
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v119_shape_terms_are_penalties_not_exclusions() {
    let root = sandbox("floor");
    let deep = root.join("checkout/a/b/c/d/e/f/g/h/i/j/k/images");
    std::fs::create_dir_all(&deep).expect("deep");
    std::fs::create_dir_all(root.join("checkout/.git")).expect("git dir");

    let (score, reasons) = adjust_for_shape(
        0.05,
        &deep.to_string_lossy(),
        &DirectoryShape {
            home: Some(root.clone()),
            known_dir: None,
        },
    );
    assert!(score >= 0.0, "a score never goes negative: {score}");
    assert!(reasons.contains(&"inside a git working tree".to_string()));
    // The directory is still a candidate: a penalty is a subtraction.
    let candidates = ranked(
        vec![evidence(&deep, 0.05)],
        &DirectoryShape {
            home: Some(root.clone()),
            known_dir: None,
        },
    );
    assert_eq!(candidates.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn v119_a_shape_without_facts_changes_no_score() {
    let root = sandbox("neutral");
    let plain = root.join("notes");
    std::fs::create_dir_all(&plain).expect("notes");
    let baseline = ranked(vec![evidence(&plain, 0.5)], &DirectoryShape::default());
    let with_home = ranked(
        vec![evidence(&plain, 0.5)],
        &DirectoryShape {
            home: Some(root.clone()),
            known_dir: None,
        },
    );
    assert_eq!(
        baseline[0].score, with_home[0].score,
        "no penalty at depth 1"
    );
    let _ = std::fs::remove_dir_all(&root);
}

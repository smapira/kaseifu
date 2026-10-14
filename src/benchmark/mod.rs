//! TREC benchmark support (research / evaluation interface).
//!
//! This module implements the evaluation harness requested by
//! `documents/plans/user/20261014b.md` (kaseifu TREC Benchmark): derive a
//! document-to-document (D2D) ground truth from the official TREC Robust04
//! qrels, run kaseifu's existing Discovery to pick one related document, and
//! measure Hit@1.
//!
//! Design boundaries (see `documents/plans/dev/20261014g-trec-benchmark-d2d.md`):
//!
//! * the document level exit of Discovery is reused as-is (`extract_terms` ->
//!   `normalize_terms` -> `find_similar_files_with`); the folder aggregation
//!   layer is deliberately **not** used, because a folder ranking is not a
//!   document ranking;
//! * every search is scoped with `mdfind -onlyin <corpus root>` and every hit
//!   is re-checked against the root after path normalization;
//! * the harness is read-only with respect to the corpus: no code path here
//!   moves, deletes, renames or rewrites a corpus document;
//! * the corpus is never the user's normal file tree.

pub mod cli;
pub mod gate;
pub mod perf;
pub mod qrels;
pub mod run;
pub mod scatter;
pub mod search;
pub mod systems;

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Benchmark id written into every report.
pub const BENCHMARK_ID: &str = "trec-robust04-derived-d2d";

/// Default seed for every deterministic choice (sampling, scattering,
/// Random baseline).
pub const DEFAULT_SEED: u64 = 42;

/// Default number of evaluated inputs (see `prepare --max-inputs`).
pub const DEFAULT_MAX_INPUTS: usize = 200;

/// Default per-`mdfind` timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Default number of corpus documents probed by `check-index`.
pub const DEFAULT_INDEX_SAMPLES: usize = 5;

/// Ground-truth document ids are embedded in a manifest only up to this size;
/// larger sets are pinned by `ground_truth_size` + `ground_truth_sha256`.
pub const GROUND_TRUTH_EMBED_LIMIT: usize = 256;

/// Derived D2D evaluation is not the official TREC Q2D task, and the source of
/// the signal differs per system. These strings are written into every run
/// JSON (`constraints`) and printed by `compare`.
pub const CONSTRAINTS: [&str; 5] = [
    "qrels are not an exhaustive ground truth: unjudged documents may be relevant, so a non-hit can be an unjudged relevant document",
    "relatedness derived from shared queries is not direct document-to-document similarity: this measures the ability to pick another document that serves the same information need",
    "date, file name and other metadata can confound the signal; treat results as metadata-free only after a separate check",
    "kaseifu scores documents from file name and extension only (src/discovery/scoring.rs), so the BM25 comparison uses a different information source and is not a same-information algorithm comparison",
    "on a corpus whose file names carry no content words, kaseifu has no body-text query terms (src/normalize/mod.rs:51-68) and coverage can be 0 by construction; hit@1 = 0 then means no query terms rather than a missed retrieval",
];

/// Fixed modification time for scattered documents (2020-01-01T00:00:00Z).
///
/// The scattering requirement asks for one fixed timestamp on every file so
/// the benchmark result cannot depend on when a document was copied.
pub const FIXED_MTIME_UNIX: i64 = 1_577_836_800;

/// Evaluation workspace layout (`~/Documents/kaseifu-benchmark` by default).
///
/// The original request names `~/Library/Application Support/kaseifu/benchmark/`,
/// but that tree is dropped by kaseifu's own destination rules
/// (`is_excluded_destination`, `src/discovery/destinations.rs:112`): every hit
/// inside it is discarded, so a run there would score a rule, not retrieval.
/// `~/Documents/kaseifu-benchmark/` is Spotlight-indexed (measured) and passes
/// those rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// Default root: `<home>/Documents/kaseifu-benchmark`.
    pub fn default_root() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot resolve the home directory")?;
        Ok(home.join("Documents").join("kaseifu-benchmark"))
    }

    pub fn from_default_root() -> Result<Self> {
        Ok(Self::from_root(Self::default_root()?))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn datasets_dir(&self) -> PathBuf {
        self.root.join("datasets").join("robust04")
    }

    pub fn qrels_path(&self) -> PathBuf {
        self.datasets_dir().join("qrels.robust2004.txt")
    }

    /// Raw corpus as delivered by NIST (never modified by the harness).
    pub fn corpus_dir(&self) -> PathBuf {
        self.root.join("corpus")
    }

    /// Scattered read-only working copy of the corpus.
    pub fn scattered_dir(&self) -> PathBuf {
        self.root.join("workspace").join("scattered")
    }

    pub fn scatter_manifest_path(&self) -> PathBuf {
        self.root
            .join("workspace")
            .join("manifests")
            .join("scattering.json")
    }

    pub fn manifests_dir(&self) -> PathBuf {
        self.root.join("manifests")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("reports")
    }

    /// Create the directory skeleton. Existing directories are left alone.
    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.datasets_dir(),
            self.corpus_dir(),
            self.scattered_dir(),
            self.manifests_dir(),
            self.runs_dir(),
            self.reports_dir(),
            self.scatter_manifest_path()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone()),
        ] {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        Ok(())
    }
}

/// Absolute, symlink-resolved path.
///
/// `canonicalize` needs the whole path to exist, which breaks for the very
/// common case of a target that is about to be created (or was just deleted).
/// The longest existing ancestor is therefore resolved and the remaining
/// components are appended: `/var/folders/x/new.txt` resolves to
/// `/private/var/folders/x/new.txt` on macOS, so a containment check never
/// fails merely because one side exists and the other does not.
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    match split_existing_ancestor(path) {
        Some((ancestor, tail)) => {
            let mut out = ancestor;
            for component in tail {
                out.push(component);
            }
            lexical_normalize(&out)
        }
        None => lexical_normalize(path),
    }
}

/// Longest existing ancestor of `path`, resolved, plus the missing tail
/// (nearest component first).
fn split_existing_ancestor(path: &Path) -> Option<(PathBuf, Vec<std::ffi::OsString>)> {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            let ancestor = std::fs::canonicalize(&current).ok()?;
            tail.reverse();
            return Some((ancestor, tail));
        }
        let name = current.file_name()?.to_os_string();
        tail.push(name);
        if !current.pop() {
            return None;
        }
    }
}

/// Resolve `.` and `..` textually without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// True when `candidate` is `root` itself or lives below it.
///
/// The comparison happens on normalized paths, so `..`, duplicate slashes and
/// symlinked spellings of the same location cannot escape the root.
pub fn is_inside(root: &Path, candidate: &Path) -> bool {
    let root = normalize_path(root);
    let candidate = normalize_path(candidate);
    candidate == root || candidate.starts_with(&root)
}

/// Containment check that reports what went wrong instead of returning false.
pub fn ensure_inside(root: &Path, candidate: &Path) -> Result<()> {
    if is_inside(root, candidate) {
        Ok(())
    } else {
        anyhow::bail!(
            "path is outside the benchmark corpus: {} (root {})",
            candidate.display(),
            root.display()
        )
    }
}

/// Lowercase hex sha256 of a byte slice.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Lowercase hex sha256 of a file's contents.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

/// sha256 of a JSON value that must serialize deterministically (structs and
/// `BTreeMap` fields only, never a `HashMap`).
pub fn sha256_json<T: Serialize>(value: &T) -> Result<String> {
    let text = serde_json::to_string(value).context("cannot serialize JSON")?;
    Ok(sha256_bytes(text.as_bytes()))
}

/// Pretty-print a value to a file, creating parent directories.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value).context("cannot serialize JSON")?;
    std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

/// Read and parse a JSON file.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

/// Current UTC time as RFC3339 with a trailing `Z`.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Timestamp tag for output file names (`20261014T091600Z`).
pub fn timestamp_tag() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// macOS version and build, or empty strings off macOS.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MacOsVersion {
    pub product_version: String,
    pub build_version: String,
    pub arch: String,
}

/// `sw_vers` / `std::env::consts::ARCH`; missing values stay empty.
pub fn macos_version() -> MacOsVersion {
    MacOsVersion {
        product_version: sw_vers_field("-productVersion"),
        build_version: sw_vers_field("-buildVersion"),
        arch: std::env::consts::ARCH.to_string(),
    }
}

fn sw_vers_field(flag: &str) -> String {
    std::process::Command::new("sw_vers")
        .arg(flag)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Build-time-ish reproducibility data, read at run time (no `build.rs`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Toolchain {
    pub kaseifu_commit: String,
    pub kaseifu_commit_dirty: bool,
    pub rust_version: String,
}

pub fn toolchain() -> Toolchain {
    Toolchain {
        kaseifu_commit: command_line("git", &["rev-parse", "HEAD"]).unwrap_or_default(),
        kaseifu_commit_dirty: !command_line("git", &["status", "--porcelain"])
            .unwrap_or_default()
            .is_empty(),
        rust_version: command_line("rustc", &["--version"]).unwrap_or_default(),
    }
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// SplitMix64: the one PRNG every deterministic choice uses.
///
/// Chosen over a table-driven or clock-seeded generator because a 64-bit
/// counter alone fully determines the stream: the same seed gives the same
/// sequence on every machine and every run.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `0..bound` (0 when `bound == 0`).
    pub fn next_below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % bound as u64) as usize
        }
    }
}

/// FNV-1a 64 of a string, used to turn an identifier into a seed.
pub fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Deterministic per-key stream: seed and key both matter, so results never
/// depend on the order the inputs are visited in.
pub fn keyed_rng(seed: u64, key: &str) -> SplitMix64 {
    SplitMix64::new(seed ^ fnv1a64(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_is_reproducible_and_varies_by_seed() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(1);
        let mut c = SplitMix64::new(2);
        let first: Vec<u64> = (0..4).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..4).map(|_| b.next_u64()).collect();
        assert_eq!(first, second);
        assert_ne!(first[0], c.next_u64());
    }

    #[test]
    fn keyed_rng_is_order_independent() {
        let one = keyed_rng(42, "DOC-A").next_u64();
        let again = keyed_rng(42, "DOC-A").next_u64();
        let other = keyed_rng(42, "DOC-B").next_u64();
        assert_eq!(one, again);
        assert_ne!(one, other);
    }

    #[test]
    fn lexical_normalize_resolves_parent_and_curdir() {
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
    }

    #[test]
    fn containment_rejects_outside_and_accepts_inside() {
        let root = Path::new("/tmp/kaseifu-bench-test-root");
        assert!(is_inside(root, Path::new("/tmp/kaseifu-bench-test-root")));
        assert!(is_inside(
            root,
            Path::new("/tmp/kaseifu-bench-test-root/dir_1/f.txt")
        ));
        assert!(!is_inside(
            root,
            Path::new("/tmp/kaseifu-bench-test-rootish/f.txt")
        ));
        assert!(!is_inside(root, Path::new("/tmp/other/f.txt")));
        assert!(!is_inside(
            root,
            Path::new("/tmp/kaseifu-bench-test-root/../other/f")
        ));
        assert!(!is_inside(root, Path::new("/Users/other/Documents/f.txt")));
    }

    #[test]
    fn a_not_yet_created_target_resolves_against_its_existing_ancestor() {
        let dir = std::env::temp_dir().join(format!("kaseifu-bench-norm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let future = dir.join("dir_0001").join("not-created-yet.txt");
        let resolved = normalize_path(&future);
        assert!(resolved.is_absolute());
        assert_eq!(resolved.file_name().unwrap(), "not-created-yet.txt");
        assert!(
            is_inside(&dir, &future),
            "a future file inside the root must be inside: {} vs {}",
            resolved.display(),
            normalize_path(&dir).display()
        );
        assert!(!is_inside(&dir, &dir.join("../outside.txt")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn workspace_paths_follow_the_documented_layout() {
        let ws = Workspace::from_root(PathBuf::from("/base"));
        assert_eq!(
            ws.qrels_path(),
            PathBuf::from("/base/datasets/robust04/qrels.robust2004.txt")
        );
        assert_eq!(
            ws.scattered_dir(),
            PathBuf::from("/base/workspace/scattered")
        );
        assert_eq!(ws.runs_dir(), PathBuf::from("/base/runs"));
        assert_eq!(
            ws.scatter_manifest_path(),
            PathBuf::from("/base/workspace/manifests/scattering.json")
        );
    }

    #[test]
    fn sha256_matches_the_known_vector() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

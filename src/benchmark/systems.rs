//! The three D2D systems: kaseifu's own Discovery, a BM25 baseline, and a
//! seeded Random baseline.
//!
//! kaseifu is not reimplemented here: the search terms, the Spotlight query,
//! the per-document score and the self-exclusion all come from the product
//! code. What this module adds is the *exit point* (documents, before the
//! folder aggregation) and one deterministic ordering, because Discovery has
//! no document-level ordering of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::scatter::ScatterManifest;
use super::{keyed_rng, normalize_path};
use crate::models::SimilarFile;
use crate::models::file_context::NormalizedFileContext;

/// Which system a run evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Kaseifu,
    Bm25,
    Random,
}

impl System {
    pub fn parse(text: &str) -> Result<Self> {
        match text.to_ascii_lowercase().as_str() {
            "kaseifu" => Ok(Self::Kaseifu),
            "bm25" => Ok(Self::Bm25),
            "random" => Ok(Self::Random),
            other => anyhow::bail!("unknown system: {other} (expected kaseifu, bm25 or random)"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Kaseifu => "kaseifu",
            Self::Bm25 => "bm25",
            Self::Random => "random",
        }
    }

    /// What the system may look at, recorded next to the score so the
    /// comparison is never read as a same-information comparison.
    pub fn information_source(&self) -> &'static str {
        match self {
            Self::Kaseifu => {
                "kaseifu Discovery: Spotlight query from file name and metadata terms, document score from file name and extension"
            }
            Self::Bm25 => "document body text of every corpus document (inverted index, BM25)",
            Self::Random => "no content; seeded uniform choice over the corpus",
        }
    }
}

/// One corpus document with its real path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRef {
    pub doc_id: String,
    pub path: PathBuf,
}

/// The scattered corpus as a lookup table (doc id <-> path).
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    root: PathBuf,
    docs: Vec<DocRef>,
    by_id: BTreeMap<String, usize>,
    by_path: BTreeMap<String, usize>,
}

impl Corpus {
    pub fn from_manifest(manifest: &ScatterManifest) -> Self {
        let root = PathBuf::from(&manifest.root);
        let mut docs: Vec<DocRef> = manifest
            .files
            .iter()
            .map(|file| DocRef {
                doc_id: file.doc_id.clone(),
                path: root.join(&file.relative_path),
            })
            .collect();
        docs.sort_by(|a, b| a.doc_id.cmp(&b.doc_id));
        let mut by_id = BTreeMap::new();
        let mut by_path = BTreeMap::new();
        for (index, doc) in docs.iter().enumerate() {
            by_id.insert(doc.doc_id.clone(), index);
            by_path.insert(
                normalize_path(&doc.path).to_string_lossy().to_string(),
                index,
            );
        }
        Self {
            root,
            docs,
            by_id,
            by_path,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn docs(&self) -> &[DocRef] {
        &self.docs
    }

    pub fn doc_id_of(&self, doc_id: &str) -> Option<&DocRef> {
        self.by_id.get(doc_id).map(|index| &self.docs[*index])
    }

    /// doc id of a real path, after normalization (symlinked spellings map to
    /// the same document).
    pub fn doc_id_of_path(&self, path: &Path) -> Option<&str> {
        let key = normalize_path(path).to_string_lossy().to_string();
        self.by_path
            .get(&key)
            .map(|index| self.docs[*index].doc_id.as_str())
    }

    pub fn paths_for(&self, doc_ids: &[String]) -> Vec<(String, Option<PathBuf>)> {
        doc_ids
            .iter()
            .map(|doc_id| {
                (
                    doc_id.clone(),
                    self.doc_id_of(doc_id).map(|doc| doc.path.clone()),
                )
            })
            .collect()
    }
}

/// One top-1 decision.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prediction {
    /// `None` when the system returned no usable candidate.
    pub doc_id: Option<String>,
    pub candidate_count: usize,
    /// Search terms produced for this input (diagnostics).
    pub terms: usize,
}

/// kaseifu's D2D prediction, taken from the document level of Discovery.
///
/// The steps mirror `discovery::discover_with` exactly
/// (`src/discovery/mod.rs:68-85`): the same term extraction, the same search
/// boundary signature, the same per-document scoring, and the same
/// self-exclusion. Only the return point differs: this stops before the folder
/// aggregation, because a folder ranking is not a document ranking.
pub fn predict_kaseifu<F>(input: &Path, corpus: &Corpus, search: F) -> Result<Prediction>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    let observed = crate::context::builder::build(input)?;
    let normalized: NormalizedFileContext = crate::normalize::normalize_with_options(
        observed,
        &crate::context::image_metadata::MetadataOptions::default(),
    )?;
    let raw_terms = crate::discovery::extract_terms(&normalized);
    let terms = crate::discovery::normalize_terms(&raw_terms);
    // The hit window is enforced inside `find_similar_files_with` through the
    // search closure's limit (the product passes MAX_SPOTLIGHT_HITS there).
    let files = crate::discovery::find_similar_files_with(
        &terms,
        &normalized.target.filename,
        normalized.target.extension.as_deref(),
        &normalized.target.path,
        &normalized.parent_directory,
        search,
    )?;
    let candidates: Vec<SimilarFile> = corpus_candidates(files, corpus);
    let ranked = rank_documents(candidates);
    let predicted = ranked
        .first()
        .and_then(|file| corpus.doc_id_of_path(&file.path).map(str::to_string));
    Ok(Prediction {
        doc_id: predicted,
        candidate_count: ranked.len(),
        terms: terms.len(),
    })
}

/// Keep only documents of this corpus, without the input document itself.
///
/// The containment test is the isolation guarantee of the request (section 6):
/// a hit is only evidence when, after normalization, it lives inside the
/// corpus.
fn corpus_candidates(files: Vec<SimilarFile>, corpus: &Corpus) -> Vec<SimilarFile> {
    let root = corpus.root();
    files
        .into_iter()
        .filter(|file| super::is_inside(root, &file.path))
        .filter(|file| corpus.doc_id_of_path(&file.path).is_some())
        .collect()
}

/// Document order: score descending, then older evidence, then path.
///
/// This is the ordering rule the product already uses for directories
/// (`src/discovery/candidates.rs:89-101`) applied to documents. No score is
/// recomputed and nothing is reweighted; the sort is the only thing this
/// harness defines, because Discovery returns documents in `mdfind` order.
pub fn rank_documents(mut files: Vec<SimilarFile>) -> Vec<SimilarFile> {
    files.sort_by(|a, b| {
        b.similarity_score
            .partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| mtime_key(a).cmp(&mtime_key(b)))
            .then_with(|| a.path.cmp(&b.path))
    });
    files
}

fn mtime_key(file: &SimilarFile) -> i64 {
    file.modified_at
        .as_ref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|time| time.timestamp())
        .unwrap_or(i64::MAX)
}

/// BM25 parameters. The request asks for a standard implementation; these are
/// the standard defaults (k1 = 1.2, b = 0.75), documented in the plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bm25Params {
    pub k1: f64,
    pub b: f64,
}

impl Default for Bm25Params {
    fn default() -> Self {
        Self { k1: 1.2, b: 0.75 }
    }
}

/// In-memory inverted index over the scattered corpus.
pub struct Bm25Index {
    doc_ids: Vec<String>,
    lengths: Vec<u32>,
    postings: BTreeMap<String, Vec<(u32, u32)>>,
    avgdl: f64,
    params: Bm25Params,
    indexed_docs: usize,
}

impl Bm25Index {
    /// Read and index the corpus. `max_docs` bounds the index (0 = all).
    pub fn build(corpus: &Corpus, max_docs: usize, params: Bm25Params) -> Result<Self> {
        let mut postings: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
        let mut lengths: Vec<u32> = Vec::new();
        let mut doc_ids: Vec<String> = Vec::new();
        let limit = if max_docs == 0 {
            corpus.len()
        } else {
            max_docs.min(corpus.len())
        };
        for doc in corpus.docs().iter().take(limit) {
            let text = std::fs::read_to_string(&doc.path)
                .with_context(|| format!("cannot read corpus document {}", doc.path.display()))?;
            let tokens = tokenize(&text);
            let index = doc_ids.len() as u32;
            lengths.push(tokens.len() as u32);
            doc_ids.push(doc.doc_id.clone());
            let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
            for token in &tokens {
                *counts.entry(token.as_str()).or_insert(0) += 1;
            }
            for (token, count) in counts {
                postings
                    .entry(token.to_string())
                    .or_default()
                    .push((index, count));
            }
        }
        let total: u64 = lengths.iter().map(|length| u64::from(*length)).sum();
        let avgdl = if lengths.is_empty() {
            0.0
        } else {
            total as f64 / lengths.len() as f64
        };
        Ok(Self {
            indexed_docs: doc_ids.len(),
            doc_ids,
            lengths,
            postings,
            avgdl,
            params,
        })
    }

    pub fn indexed_docs(&self) -> usize {
        self.indexed_docs
    }

    pub fn terms(&self) -> usize {
        self.postings.len()
    }

    /// Top-1 by BM25 score over the query document's own body terms.
    pub fn predict(&self, source_doc_id: &str, corpus: &Corpus) -> Prediction {
        let Some(source) = corpus.doc_id_of(source_doc_id) else {
            return Prediction::default();
        };
        let Ok(text) = std::fs::read_to_string(&source.path) else {
            return Prediction::default();
        };
        let mut query: BTreeSet<String> = BTreeSet::new();
        for token in tokenize(&text) {
            query.insert(token);
        }
        let mut scores: BTreeMap<u32, f64> = BTreeMap::new();
        let total = self.doc_ids.len() as f64;
        for term in &query {
            let Some(postings) = self.postings.get(term) else {
                continue;
            };
            let document_frequency = postings.len() as f64;
            let idf = (1.0 + (total - document_frequency + 0.5) / (document_frequency + 0.5)).ln();
            for (index, frequency) in postings {
                let length = f64::from(self.lengths[*index as usize]);
                let denominator = f64::from(*frequency)
                    + self.params.k1 * (1.0 - self.params.b + self.params.b * length / self.avgdl);
                if denominator <= 0.0 {
                    continue;
                }
                let score = idf * (f64::from(*frequency) * (self.params.k1 + 1.0)) / denominator;
                *scores.entry(*index).or_insert(0.0) += score;
            }
        }

        let mut best: Option<(f64, &str)> = None;
        for (index, score) in &scores {
            if *score <= 0.0 {
                continue;
            }
            let doc_id = self.doc_ids[*index as usize].as_str();
            if doc_id == source_doc_id {
                continue;
            }
            match best {
                Some((best_score, best_id)) => {
                    if *score > best_score || (*score == best_score && doc_id < best_id) {
                        best = Some((*score, doc_id));
                    }
                }
                None => best = Some((*score, doc_id)),
            }
        }
        let candidate_count = scores
            .iter()
            .filter(|(index, score)| {
                **score > 0.0 && self.doc_ids[**index as usize] != source_doc_id
            })
            .count();
        Prediction {
            doc_id: best.map(|(_, doc_id)| doc_id.to_string()),
            candidate_count,
            terms: query.len(),
        }
    }
}

/// Split on anything that is not an ASCII alphanumeric and lowercase it.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

/// Seeded Random baseline: one uniform choice over the other corpus documents.
///
/// The stream is keyed by the source doc id, so the choice does not depend on
/// the order inputs are visited in.
pub fn predict_random(corpus: &Corpus, source_doc_id: &str, seed: u64) -> Prediction {
    let pool: Vec<&str> = corpus
        .docs()
        .iter()
        .map(|doc| doc.doc_id.as_str())
        .filter(|doc_id| *doc_id != source_doc_id)
        .collect();
    if pool.is_empty() {
        return Prediction::default();
    }
    let mut rng = keyed_rng(seed, source_doc_id);
    let index = rng.next_below(pool.len());
    Prediction {
        doc_id: Some(pool[index].to_string()),
        candidate_count: pool.len(),
        terms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn similar(path: &str, score: f64, modified: Option<&str>) -> SimilarFile {
        SimilarFile {
            path: PathBuf::from(path),
            parent_directory: String::new(),
            matched_terms: Vec::new(),
            similarity_score: score,
            filename_similarity: score,
            token_similarity: score,
            extension_match: 1.0,
            modified_at: modified.map(str::to_string),
        }
    }

    #[test]
    fn system_parsing_is_strict() {
        assert_eq!(System::parse("kaseifu").unwrap(), System::Kaseifu);
        assert_eq!(System::parse("BM25").unwrap(), System::Bm25);
        assert_eq!(System::parse("random").unwrap(), System::Random);
        assert!(System::parse("oracle").is_err());
        assert_eq!(System::Random.as_str(), "random");
    }

    #[test]
    fn document_ranking_is_score_then_age_then_path() {
        let ranked = rank_documents(vec![
            similar("/c/b.txt", 0.50, Some("2020-01-01T00:00:00Z")),
            similar("/c/a.txt", 0.50, Some("2019-01-01T00:00:00Z")),
            similar("/c/z.txt", 0.90, Some("2021-01-01T00:00:00Z")),
            similar("/c/c.txt", 0.50, None),
        ]);
        let order: Vec<String> = ranked
            .iter()
            .map(|file| file.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            order,
            vec!["/c/z.txt", "/c/a.txt", "/c/b.txt", "/c/c.txt"],
            "highest score first, then older, then path, then missing date last"
        );
    }

    #[test]
    fn tokenizer_is_ascii_alphanumeric_lowercase() {
        assert_eq!(tokenize("Hello, World! 42\n"), vec!["hello", "world", "42"]);
        assert!(tokenize("...").is_empty());
    }

    #[test]
    fn random_is_seeded_and_excludes_the_source() {
        let manifest = test_manifest("");
        let corpus = Corpus::from_manifest(&manifest);
        let first = predict_random(&corpus, "DOC-001", 42);
        let second = predict_random(&corpus, "DOC-001", 42);
        let other = predict_random(&corpus, "DOC-001", 43);
        assert_eq!(first, second);
        assert_ne!(first.doc_id, other.doc_id);
        assert_ne!(first.doc_id.as_deref(), Some("DOC-001"));
        assert_eq!(first.candidate_count, 2);
    }

    fn test_manifest(root: &str) -> ScatterManifest {
        ScatterManifest {
            benchmark: "test".to_string(),
            algorithm: "test".to_string(),
            strategy: "flat".to_string(),
            seed: 42,
            directory_count: 1,
            filename_strategy: "test".to_string(),
            timestamp_strategy: "fixed".to_string(),
            fixed_mtime_unix: 0,
            birthtime_settable: false,
            birthtime_note: String::new(),
            birthtime_observed_unix: None,
            root: root.to_string(),
            selector: "test".to_string(),
            files: (1..=3)
                .map(|index| super::super::scatter::ScatterFile {
                    doc_id: format!("DOC-{index:03}"),
                    relative_path: format!("f{index}.txt"),
                    sha256: "x".to_string(),
                    bytes: 1,
                    mtime_unix: 0,
                })
                .collect(),
            missing_doc_ids: Vec::new(),
            total_bytes: 3,
            tree_sha256: "x".to_string(),
            manifest_sha256: "x".to_string(),
        }
    }

    #[test]
    fn corpus_lookup_survives_symlinked_spellings() {
        let dir = std::env::temp_dir().join(format!("kaseifu-bench-corpus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f1.txt"), "body\n").unwrap();
        let manifest = test_manifest(&dir.to_string_lossy());
        let corpus = Corpus::from_manifest(&manifest);
        assert_eq!(corpus.len(), 3);
        let direct = dir.join("f1.txt");
        assert_eq!(corpus.doc_id_of_path(&direct), Some("DOC-001"));
        let indirect = dir.join(".").join("f1.txt");
        assert_eq!(corpus.doc_id_of_path(&indirect), Some("DOC-001"));
        assert_eq!(corpus.doc_id_of_path(Path::new("/elsewhere/f1.txt")), None);
        assert!(corpus.doc_id_of("DOC-002").is_some());
        assert_eq!(corpus.paths_for(&["DOC-002".to_string()]).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bm25_indexes_and_prefers_the_body_overlap() {
        let dir = std::env::temp_dir().join(format!("kaseifu-bench-bm25-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f1.txt"), "apple banana cherry\n").unwrap();
        std::fs::write(dir.join("f2.txt"), "apple banana\n").unwrap();
        std::fs::write(dir.join("f3.txt"), "zebra yak xylophone\n").unwrap();
        let mut manifest = test_manifest(&dir.to_string_lossy());
        manifest.files[1].relative_path = "f2.txt".to_string();
        let corpus = Corpus::from_manifest(&manifest);
        let index = Bm25Index::build(&corpus, 0, Bm25Params::default()).unwrap();
        assert_eq!(index.indexed_docs(), 3);
        assert!(index.terms() > 0);
        let prediction = index.predict("DOC-002", &corpus);
        assert_eq!(prediction.doc_id.as_deref(), Some("DOC-001"));
        assert!(prediction.candidate_count >= 1);
        // Deterministic: same index, same corpus, same answer.
        assert_eq!(prediction, index.predict("DOC-002", &corpus));
        // An unknown source yields nothing rather than a fabricated answer.
        assert_eq!(index.predict("DOC-999", &corpus), Prediction::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bm25_without_overlap_returns_no_candidate() {
        let dir = std::env::temp_dir().join(format!("kaseifu-bench-bm25n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f1.txt"), "alpha one\n").unwrap();
        std::fs::write(dir.join("f2.txt"), "zulu two\n").unwrap();
        let mut manifest = test_manifest(&dir.to_string_lossy());
        manifest.files.truncate(2);
        let corpus = Corpus::from_manifest(&manifest);
        let index = Bm25Index::build(&corpus, 0, Bm25Params::default()).unwrap();
        let prediction = index.predict("DOC-001", &corpus);
        assert_eq!(prediction.doc_id, None);
        assert_eq!(prediction.candidate_count, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! Official TREC Robust04 qrels -> D2D ground truth -> reproducible manifest.
//!
//! Ground-truth rule (request, section 3): documents A and B are a related
//! pair when they share at least one topic on which both were judged relevant.
//! Only positive judgments are read, an unjudged document is never treated as
//! irrelevant, and A is never its own ground truth. This is a derivation from
//! shared topics, not a direct human document-to-document judgment.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{BENCHMARK_ID, GROUND_TRUTH_EMBED_LIMIT, SplitMix64, fnv1a64, sha256_bytes};

/// Source of the official judgments.
pub const QRELS_URL: &str = "https://trec.nist.gov/data/robust/qrels.robust2004.txt";

/// Tolerated malformed lines before parsing stops.
const MAX_REPORTED_ERRORS: usize = 10;

/// Robust04 topic ids: 301-450 plus 601-700 (250 expected, 672 has no
/// judgments in the distributed file).
pub fn robust04_topic_ids() -> BTreeSet<u32> {
    (301..=450).chain(601..=700).collect()
}

/// Counts recorded next to every manifest so a run can be traced back to the
/// exact input file (not just its checksum).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct QrelsSummary {
    pub qrels_lines: u64,
    pub topics_total: u64,
    pub topics_missing: Vec<u32>,
    pub rel_histogram: BTreeMap<String, u64>,
    pub judged_docs: u64,
    pub positive_judgments: u64,
    pub positive_docs: u64,
}

/// Parsed qrels: positive judgments only.
#[derive(Debug, Clone, Default)]
pub struct Qrels {
    /// doc id -> topic -> relevance grade (positive judgments only)
    by_doc: BTreeMap<String, BTreeMap<u32, u8>>,
    /// topic -> positively judged doc ids
    by_topic: BTreeMap<u32, BTreeSet<String>>,
    pub summary: QrelsSummary,
}

impl Qrels {
    /// Parse qrels text. Every data line has exactly four fields:
    /// `topic iteration docno relevance`.
    pub fn parse(text: &str) -> Result<Self> {
        let mut by_doc: BTreeMap<String, BTreeMap<u32, u8>> = BTreeMap::new();
        let mut by_topic: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
        let mut judged_docs: BTreeSet<&str> = BTreeSet::new();
        let mut topics: BTreeSet<u32> = BTreeSet::new();
        let mut histogram: BTreeMap<String, u64> = BTreeMap::new();
        let mut lines: u64 = 0;
        let mut positive_judgments: u64 = 0;
        let mut errors: Vec<String> = Vec::new();

        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            lines += 1;
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 4 {
                push_error(
                    &mut errors,
                    format!(
                        "line {}: expected 4 fields, found {}",
                        index + 1,
                        fields.len()
                    ),
                );
                continue;
            }
            let Some(topic) = fields[0].parse::<u32>().ok() else {
                push_error(
                    &mut errors,
                    format!("line {}: topic is not an integer: {}", index + 1, fields[0]),
                );
                continue;
            };
            if fields[1].parse::<u32>().is_err() {
                push_error(
                    &mut errors,
                    format!(
                        "line {}: iteration is not an integer: {}",
                        index + 1,
                        fields[1]
                    ),
                );
                continue;
            }
            let Some(grade) = fields[3].parse::<u8>().ok() else {
                push_error(
                    &mut errors,
                    format!(
                        "line {}: relevance is not an integer: {}",
                        index + 1,
                        fields[3]
                    ),
                );
                continue;
            };
            let doc = fields[2];
            if doc.is_empty() {
                push_error(&mut errors, format!("line {}: empty doc id", index + 1));
                continue;
            }
            topics.insert(topic);
            judged_docs.insert(doc);
            *histogram.entry(grade.to_string()).or_insert(0) += 1;
            if grade == 0 {
                continue;
            }
            positive_judgments += 1;
            by_doc
                .entry(doc.to_string())
                .or_default()
                .insert(topic, grade);
            by_topic.entry(topic).or_default().insert(doc.to_string());
        }

        if let Some(first) = errors.first() {
            anyhow::bail!(
                "qrels parse failed ({} problem line(s)); first: {}",
                errors.len(),
                first
            );
        }
        if lines == 0 {
            anyhow::bail!("qrels file contains no judgments");
        }

        let expected = robust04_topic_ids();
        let topics_missing: Vec<u32> = expected.difference(&topics).copied().collect();
        let summary = QrelsSummary {
            qrels_lines: lines,
            topics_total: topics.len() as u64,
            topics_missing,
            rel_histogram: histogram,
            judged_docs: judged_docs.len() as u64,
            positive_judgments,
            positive_docs: by_doc.len() as u64,
        };
        Ok(Self {
            by_doc,
            by_topic,
            summary,
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read qrels: {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("cannot parse qrels: {}", path.display()))
    }

    /// Ground truth of one input document: every other document that shares at
    /// least one positively judged topic with it.
    pub fn ground_truth(&self, doc_id: &str) -> BTreeSet<&str> {
        let mut out: BTreeSet<&str> = BTreeSet::new();
        let Some(topics) = self.by_doc.get(doc_id) else {
            return out;
        };
        for topic in topics.keys() {
            if let Some(docs) = self.by_topic.get(topic) {
                for candidate in docs {
                    if candidate != doc_id {
                        out.insert(candidate.as_str());
                    }
                }
            }
        }
        out
    }

    /// Positively judged topics of one document, with their grades.
    pub fn positive_topics(&self, doc_id: &str) -> Vec<TopicGrade> {
        self.by_doc
            .get(doc_id)
            .map(|topics| {
                topics
                    .iter()
                    .map(|(topic, rel)| TopicGrade {
                        topic: *topic,
                        rel: *rel,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Size of every eligible input's ground truth, without materializing all
    /// sets at once (one set at a time is bounded by the corpus size).
    pub fn eligible_input_ids(&self) -> Vec<String> {
        self.by_doc
            .keys()
            .filter(|doc_id| !self.ground_truth(doc_id).is_empty())
            .cloned()
            .collect()
    }

    pub fn ground_truth_size(&self, doc_id: &str) -> usize {
        self.ground_truth(doc_id).len()
    }

    /// Every positively judged document id (the D2D candidate universe).
    pub fn positive_doc_ids(&self) -> Vec<String> {
        self.by_doc.keys().cloned().collect()
    }
}

fn push_error(errors: &mut Vec<String>, message: String) {
    if errors.len() < MAX_REPORTED_ERRORS {
        errors.push(message);
    }
}

/// Grade of one positively judged topic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopicGrade {
    pub topic: u32,
    pub rel: u8,
}

/// One input's ground truth, kept compact: the ids themselves are embedded
/// only while they stay small, otherwise the checksum plus the size pin them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputGroundTruth {
    pub doc_id: String,
    truth: BTreeSet<String>,
}

impl InputGroundTruth {
    pub fn new(doc_id: &str, truth: BTreeSet<&str>) -> Self {
        Self {
            doc_id: doc_id.to_string(),
            truth: truth.into_iter().map(str::to_string).collect(),
        }
    }

    pub fn size(&self) -> usize {
        self.truth.len()
    }

    pub fn checksum(&self) -> String {
        let joined: Vec<&str> = self.truth.iter().map(String::as_str).collect();
        sha256_bytes(joined.join("\n").as_bytes())
    }

    pub fn embedded_ids(&self) -> Option<Vec<String>> {
        if self.truth.len() <= GROUND_TRUTH_EMBED_LIMIT {
            Some(self.truth.iter().cloned().collect())
        } else {
            None
        }
    }

    pub fn contains(&self, doc_id: &str) -> bool {
        self.truth.contains(doc_id)
    }
}

/// Dataset identity, recorded in both the manifest and every run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetInfo {
    pub name: String,
    pub version: String,
    pub source_url: String,
    pub sha256: String,
    pub bytes: u64,
    pub topics_total: u64,
    pub topics_missing: Vec<u32>,
    pub qrels_lines: u64,
    pub positive_judgments: u64,
    pub rel_histogram: BTreeMap<String, u64>,
    pub judged_docs: u64,
    pub positive_docs: u64,
}

impl DatasetInfo {
    pub fn from_file(path: &Path, qrels: &Qrels) -> Result<Self> {
        let bytes = std::fs::metadata(path)
            .with_context(|| format!("cannot stat qrels: {}", path.display()))?
            .len();
        Ok(Self {
            name: "TREC Robust04 qrels (robust2004)".to_string(),
            version: "robust2004".to_string(),
            source_url: QRELS_URL.to_string(),
            sha256: super::sha256_file(path)?,
            bytes,
            topics_total: qrels.summary.topics_total,
            topics_missing: qrels.summary.topics_missing.clone(),
            qrels_lines: qrels.summary.qrels_lines,
            positive_judgments: qrels.summary.positive_judgments,
            rel_histogram: qrels.summary.rel_histogram.clone(),
            judged_docs: qrels.summary.judged_docs,
            positive_docs: qrels.summary.positive_docs,
        })
    }
}

/// How the D2D ground truth was derived (the request asks for this to be
/// explicit in the output).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Derivation {
    pub rule: String,
    pub positive_only: bool,
    pub self_excluded: bool,
    pub negative_judgments_used: bool,
    pub graded_values_saved: bool,
    pub note: String,
}

impl Default for Derivation {
    fn default() -> Self {
        Self {
            rule: "A and B are related when both are positively judged on at least one shared topic".to_string(),
            positive_only: true,
            self_excluded: true,
            negative_judgments_used: false,
            graded_values_saved: true,
            note: "derived from shared judged topics, not a direct human document-to-document judgment; unjudged documents are not treated as irrelevant".to_string(),
        }
    }
}

/// Which inputs are evaluated, and how they were chosen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sampling {
    pub seed: u64,
    /// `None` means every eligible input is evaluated.
    pub requested: Option<usize>,
    pub selected: usize,
    pub eligible_inputs: usize,
    pub algorithm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestEntry {
    pub source_doc_id: String,
    pub positive_topics: Vec<TopicGrade>,
    pub ground_truth_size: usize,
    pub ground_truth_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ground_truth_doc_ids: Option<Vec<String>>,
}

/// The frozen evaluation set: same input file, same settings, same bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct D2dManifest {
    pub benchmark: String,
    pub schema_version: u32,
    pub dataset: DatasetInfo,
    pub derivation: Derivation,
    pub sampling: Sampling,
    pub excluded_no_ground_truth: u64,
    pub entries: Vec<ManifestEntry>,
    pub manifest_sha256: String,
}

impl D2dManifest {
    pub fn entry(&self, doc_id: &str) -> Option<&ManifestEntry> {
        self.entries.iter().find(|e| e.source_doc_id == doc_id)
    }

    pub fn source_doc_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| e.source_doc_id.clone())
            .collect()
    }
}

/// Deterministic subset: inputs are ranked by a keyed hash, the head is taken,
/// and the result is sorted by doc id. Order-independent and stable across
/// machines (no clock, no global RNG state).
pub fn select_inputs(eligible: &[String], seed: u64, max_inputs: usize) -> Vec<String> {
    if max_inputs == 0 || eligible.len() <= max_inputs {
        let mut all = eligible.to_vec();
        all.sort();
        return all;
    }
    let mut ranked: Vec<(u64, &String)> = eligible
        .iter()
        .map(|doc| {
            let mut rng = SplitMix64::new(seed ^ fnv1a64(doc));
            (rng.next_u64(), doc)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    let mut selected: Vec<String> = ranked
        .into_iter()
        .take(max_inputs)
        .map(|(_, doc)| doc.clone())
        .collect();
    selected.sort();
    selected
}

/// Build the manifest for the selected inputs.
pub fn build_manifest(
    qrels: &Qrels,
    dataset: DatasetInfo,
    seed: u64,
    max_inputs: usize,
) -> Result<D2dManifest> {
    let eligible = qrels.eligible_input_ids();
    let (ground_truths, excluded) = collect_ground_truths(qrels, &eligible);
    let eligible_ids: Vec<String> = ground_truths.iter().map(|g| g.doc_id.clone()).collect();
    let selected = select_inputs(&eligible_ids, seed, max_inputs);

    let mut entries: Vec<ManifestEntry> = Vec::with_capacity(selected.len());
    for doc_id in &selected {
        let Some(truth) = ground_truths.iter().find(|g| &g.doc_id == doc_id) else {
            continue;
        };
        entries.push(ManifestEntry {
            source_doc_id: doc_id.clone(),
            positive_topics: qrels.positive_topics(doc_id),
            ground_truth_size: truth.size(),
            ground_truth_sha256: truth.checksum(),
            ground_truth_doc_ids: truth.embedded_ids(),
        });
    }

    let mut manifest = D2dManifest {
        benchmark: BENCHMARK_ID.to_string(),
        schema_version: 1,
        dataset,
        derivation: Derivation::default(),
        sampling: Sampling {
            seed,
            requested: if max_inputs == 0 {
                None
            } else {
                Some(max_inputs)
            },
            selected: entries.len(),
            eligible_inputs: eligible_ids.len(),
            algorithm: "splitmix64-ranked-selection-v1".to_string(),
        },
        excluded_no_ground_truth: excluded,
        entries,
        manifest_sha256: String::new(),
    };
    finalize_manifest(&mut manifest)?;
    Ok(manifest)
}

/// Compute ground truths for the given ids (one set at a time).
fn collect_ground_truths(qrels: &Qrels, ids: &[String]) -> (Vec<InputGroundTruth>, u64) {
    let mut out = Vec::with_capacity(ids.len());
    let mut excluded = 0u64;
    for doc_id in ids {
        let truth = qrels.ground_truth(doc_id);
        if truth.is_empty() {
            excluded += 1;
            continue;
        }
        out.push(InputGroundTruth::new(doc_id, truth));
    }
    (out, excluded)
}

/// Compute the manifest checksum: sha256 over the canonical JSON with the
/// checksum field itself empty.
pub fn finalize_manifest(manifest: &mut D2dManifest) -> Result<()> {
    manifest.manifest_sha256 = String::new();
    manifest.manifest_sha256 = super::sha256_json(manifest)?;
    Ok(())
}

/// Recompute the checksum and reject a manifest that does not match.
pub fn verify_manifest(manifest: &D2dManifest) -> Result<()> {
    let recorded = manifest.manifest_sha256.clone();
    if recorded.is_empty() {
        anyhow::bail!("manifest has no checksum");
    }
    let mut copy = manifest.clone();
    finalize_manifest(&mut copy)?;
    if copy.manifest_sha256 != recorded {
        anyhow::bail!(
            "manifest checksum mismatch: recorded {}, recomputed {}",
            recorded,
            copy.manifest_sha256
        );
    }
    Ok(())
}

pub fn load_manifest(path: &Path) -> Result<D2dManifest> {
    let manifest: D2dManifest = super::read_json(path)?;
    verify_manifest(&manifest)
        .with_context(|| format!("manifest integrity check failed: {}", path.display()))?;
    Ok(manifest)
}

/// Newest manifest in the manifests directory, if any.
///
/// "Newest" means the latest modification time: a name order would prefer
/// `d2d-seed42-99.json` over `d2d-seed42-200.json`. Ties and unreadable
/// metadata fall back to the path order, so the choice stays deterministic.
pub fn newest_manifest(dir: &Path) -> Option<std::path::PathBuf> {
    let candidates: Vec<(std::path::PathBuf, std::time::SystemTime)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "json")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("d2d-"))
        })
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (path, modified)
        })
        .collect();
    select_newest(candidates)
}

/// Latest `(path, mtime)` pair; ties break toward the greater path.
fn select_newest(
    candidates: Vec<(std::path::PathBuf, std::time::SystemTime)>,
) -> Option<std::path::PathBuf> {
    candidates
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
        .map(|(path, _)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "301 0 DOC-A 1\n301 0 DOC-B 1\n301 0 DOC-C 0\n302 0 DOC-B 2\n302 0 DOC-D 1\n303 0 DOC-E 0\n";

    #[test]
    fn parses_counts_and_missing_topics() {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        assert_eq!(qrels.summary.qrels_lines, 6);
        assert_eq!(qrels.summary.positive_judgments, 4);
        assert_eq!(qrels.summary.judged_docs, 5);
        assert_eq!(qrels.summary.positive_docs, 3);
        assert_eq!(qrels.summary.topics_total, 3);
        // 672 is the one topic the distributed file has no judgments for.
        assert!(qrels.summary.topics_missing.contains(&672));
        assert_eq!(qrels.summary.rel_histogram.get("0"), Some(&2));
        assert_eq!(qrels.summary.rel_histogram.get("2"), Some(&1));
    }

    #[test]
    fn rejects_malformed_lines_with_the_line_number() {
        let error = Qrels::parse("301 0 DOC-A\n").unwrap_err().to_string();
        assert!(error.contains("line 1"), "{error}");
        let error = Qrels::parse("301 0 DOC-A x\n").unwrap_err().to_string();
        assert!(error.contains("relevance"), "{error}");
        assert!(Qrels::parse("").is_err());
    }

    #[test]
    fn ground_truth_is_many_to_many_and_excludes_self() {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        let truth = qrels.ground_truth("DOC-B");
        assert!(truth.contains("DOC-A"), "shared topic 301");
        assert!(truth.contains("DOC-D"), "shared topic 302");
        assert!(!truth.contains("DOC-B"), "self is never ground truth");
        assert!(!truth.contains("DOC-C"), "judged 0 is not used");
        assert!(!truth.contains("DOC-E"), "judged 0 only");
        assert!(qrels.ground_truth("DOC-D").contains("DOC-B"));
    }

    #[test]
    fn input_without_ground_truth_is_not_eligible() {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        assert!(qrels.ground_truth("DOC-A").contains("DOC-B"));
        assert!(qrels.ground_truth("DOC-C").is_empty());
        assert!(qrels.ground_truth("DOC-E").is_empty());
        let eligible = qrels.eligible_input_ids();
        assert!(eligible.contains(&"DOC-A".to_string()));
        assert!(!eligible.contains(&"DOC-C".to_string()));
        assert!(!eligible.contains(&"DOC-E".to_string()));
    }

    #[test]
    fn graded_values_are_kept_for_future_evaluation() {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        let topics = qrels.positive_topics("DOC-B");
        assert_eq!(
            topics,
            vec![
                TopicGrade { topic: 301, rel: 1 },
                TopicGrade { topic: 302, rel: 2 }
            ]
        );
    }

    fn dataset() -> DatasetInfo {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        DatasetInfo {
            name: "test".to_string(),
            version: "test".to_string(),
            source_url: "test".to_string(),
            sha256: sha256_bytes(SAMPLE.as_bytes()),
            bytes: SAMPLE.len() as u64,
            topics_total: qrels.summary.topics_total,
            topics_missing: qrels.summary.topics_missing.clone(),
            qrels_lines: qrels.summary.qrels_lines,
            positive_judgments: qrels.summary.positive_judgments,
            rel_histogram: qrels.summary.rel_histogram.clone(),
            judged_docs: qrels.summary.judged_docs,
            positive_docs: qrels.summary.positive_docs,
        }
    }

    #[test]
    fn same_input_gives_the_same_manifest_and_checksum() {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        let first = build_manifest(&qrels, dataset(), 42, 0).unwrap();
        let second = build_manifest(&qrels, dataset(), 42, 0).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.manifest_sha256, second.manifest_sha256);
        verify_manifest(&first).unwrap();
        assert_eq!(first.sampling.algorithm, "splitmix64-ranked-selection-v1");
        assert!(first.derivation.positive_only);
        assert!(!first.derivation.negative_judgments_used);
        // Small ground truths are embedded, so the set itself is pinned.
        let entry = first.entry("DOC-B").unwrap();
        assert_eq!(entry.ground_truth_size, 2);
        assert!(entry.ground_truth_doc_ids.is_some());
    }

    #[test]
    fn a_changed_manifest_is_rejected() {
        let qrels = Qrels::parse(SAMPLE).unwrap();
        let mut manifest = build_manifest(&qrels, dataset(), 42, 0).unwrap();
        manifest.entries[0].ground_truth_size += 1;
        assert!(verify_manifest(&manifest).is_err());
    }

    #[test]
    fn sampling_is_seeded_sorted_and_bounded() {
        let eligible: Vec<String> = (0..50).map(|i| format!("DOC-{i:03}")).collect();
        let first = select_inputs(&eligible, 42, 10);
        let second = select_inputs(&eligible, 42, 10);
        let other = select_inputs(&eligible, 43, 10);
        assert_eq!(first, second, "same seed, same input set");
        assert_ne!(first, other, "the seed matters");
        assert_eq!(first.len(), 10);
        let mut sorted = first.clone();
        sorted.sort();
        assert_eq!(first, sorted, "selected inputs are sorted by doc id");
        assert_eq!(select_inputs(&eligible, 42, 0).len(), 50);
        assert_eq!(select_inputs(&eligible, 42, 99).len(), 50);
    }

    #[test]
    fn newest_manifest_prefers_mtime_over_name_order() {
        use std::time::{Duration, UNIX_EPOCH};
        let older = (
            std::path::PathBuf::from("d2d-seed42-99.json"),
            UNIX_EPOCH + Duration::from_secs(1),
        );
        let newer = (
            std::path::PathBuf::from("d2d-seed42-200.json"),
            UNIX_EPOCH + Duration::from_secs(2),
        );
        assert_eq!(
            select_newest(vec![older, newer]),
            Some(std::path::PathBuf::from("d2d-seed42-200.json"))
        );
        assert_eq!(select_newest(vec![]), None);
    }
}

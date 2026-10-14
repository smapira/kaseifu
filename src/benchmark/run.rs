//! Evaluation run: manifest + corpus -> predictions -> Hit@1 -> JSON/CSV.
//!
//! Nothing here writes to the corpus. The only files this module creates are
//! the run report and its per-input CSV, both under the workspace `runs/`
//! directory.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::gate::IndexReport;
use super::qrels::{D2dManifest, DatasetInfo, Qrels};
use super::scatter::ScatterManifest;
use super::systems::{
    Bm25Index, Bm25Params, Corpus, Prediction, System, predict_kaseifu, predict_random,
};
use super::{
    BENCHMARK_ID, CONSTRAINTS, MacOsVersion, Toolchain, Workspace, ensure_inside, macos_version,
    now_rfc3339, timestamp_tag, toolchain,
};

/// One evaluated input, in CSV column order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputResult {
    pub source_doc_id: String,
    /// Empty when the system returned no candidate.
    pub predicted_doc_id: String,
    pub hit: bool,
    pub latency_ms: u64,
    pub candidate_count: usize,
}

/// Summary statistics recorded for latency and candidate counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub mean: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

/// The four keys the request fixes for `results`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResults {
    pub evaluated: usize,
    pub hits: usize,
    pub hit_at_1: f64,
    pub coverage: f64,
}

/// The secondary metrics of the request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Auxiliary {
    pub no_result_rate: f64,
    pub latency_ms: Distribution,
    pub candidate_count: Distribution,
}

/// How the corpus was laid out, recorded next to the score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScatteringInfo {
    pub strategy: String,
    pub seed: u64,
    pub directory_count: usize,
    pub filename_strategy: String,
    pub timestamp_strategy: String,
}

/// Counts that explain a low coverage without guessing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    pub inputs: usize,
    pub inputs_with_empty_terms: usize,
    pub inputs_with_zero_candidates: usize,
    pub skipped_missing_input: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestRef {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusInfo {
    pub files: usize,
    pub bytes: u64,
    pub tree_sha256: String,
    pub missing_documents: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotlightRecord {
    pub ready: bool,
    pub checked_at: String,
    pub indexing_enabled: Option<bool>,
    pub mdutil_volume_root: String,
    pub mdutil_data_volume: String,
    pub samples_total: usize,
    pub samples_found_by_name: usize,
    pub samples_found_by_content: usize,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bm25Settings {
    pub k1: f64,
    pub b: f64,
    pub indexed_docs: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub seed: u64,
    pub timeout_ms: u64,
    pub max_spotlight_hits: usize,
    pub search_scope: String,
    pub image_metadata_spotlight: bool,
    pub image_metadata_exif_fallback: bool,
    pub bm25: Bm25Settings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reproducibility {
    pub kaseifu_commit: String,
    pub kaseifu_commit_dirty: bool,
    pub rust_version: String,
    pub dataset: DatasetInfo,
    pub corpus: CorpusInfo,
    pub manifest: ManifestRef,
    pub macos: MacOsVersion,
    pub spotlight: SpotlightRecord,
    pub settings: Settings,
    pub information_source: String,
    pub started_at: String,
    pub finished_at: String,
    pub timeouts: usize,
}

/// The whole run report (the JSON file written per run).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub benchmark: String,
    pub metric: String,
    pub system: String,
    pub results: RunResults,
    pub auxiliary: Auxiliary,
    pub scattering: ScatteringInfo,
    pub diagnostics: Diagnostics,
    pub reproducibility: Reproducibility,
    pub constraints: Vec<String>,
}

/// Everything a run needs, passed in so tests can drive it with fixtures.
pub struct RunRequest<'a> {
    pub workspace: &'a Workspace,
    pub manifest: &'a D2dManifest,
    /// File the manifest was read from, recorded in the report.
    pub manifest_path: PathBuf,
    pub scatter: &'a ScatterManifest,
    pub qrels: &'a Qrels,
    pub gate: &'a IndexReport,
    pub system: System,
    pub seed: u64,
    pub bm25: Bm25Params,
    pub bm25_max_docs: usize,
    pub timeout_ms: u64,
    /// Timeouts counted by the caller's search, read when the report is built.
    /// `None` when the search does not count them (injected test searches).
    pub timeout_counter: Option<&'a AtomicUsize>,
}

/// Report plus per-input rows.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub report: RunReport,
    pub rows: Vec<InputResult>,
}

/// Run one system over the frozen manifest.
///
/// The gate is checked here as well as by the caller: a run against an
/// incomplete index is refused, not scored.
pub fn evaluate<F>(request: &RunRequest<'_>, search: F) -> Result<RunOutcome>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    if !request.gate.ready {
        anyhow::bail!(
            "refusing to evaluate: the Spotlight index is not ready ({} reason(s)); run check-index first",
            request.gate.reasons.len()
        );
    }
    let started_at = now_rfc3339();
    let corpus = Corpus::from_manifest(request.scatter);
    let bm25_index = build_bm25_if_needed(request, &corpus)?;

    let mut rows: Vec<InputResult> = Vec::new();
    let mut skipped_missing_input = 0usize;
    let mut inputs_with_empty_terms = 0usize;
    for entry in &request.manifest.entries {
        let Some(path) = corpus
            .doc_id_of(&entry.source_doc_id)
            .map(|doc| doc.path.clone())
        else {
            skipped_missing_input += 1;
            continue;
        };
        ensure_inside(corpus.root(), &path)?;
        let (prediction, latency_ms) = predict_one(request, &corpus, &bm25_index, &path, &search)?;
        if request.system == System::Kaseifu && prediction.terms == 0 {
            inputs_with_empty_terms += 1;
        }
        let truth_contains = ground_truth_contains(request.qrels, entry, &prediction)?;
        let predicted_doc_id = prediction.doc_id.clone().unwrap_or_default();
        let hit = !predicted_doc_id.is_empty() && truth_contains;
        rows.push(InputResult {
            source_doc_id: entry.source_doc_id.clone(),
            predicted_doc_id,
            hit,
            latency_ms,
            candidate_count: prediction.candidate_count,
        });
    }

    if rows.is_empty() {
        anyhow::bail!(
            "nothing to evaluate: no manifest input has a file in the corpus ({} skipped). Scatter the corpus first.",
            skipped_missing_input
        );
    }

    let report = build_report(
        request,
        &corpus,
        &bm25_index,
        rows.clone(),
        RunCounts {
            skipped_missing_input,
            inputs_with_empty_terms,
        },
        Timestamps {
            started_at,
            finished_at: now_rfc3339(),
        },
    );
    Ok(RunOutcome { report, rows })
}

fn build_bm25_if_needed(request: &RunRequest<'_>, corpus: &Corpus) -> Result<Option<Bm25Index>> {
    if request.system != System::Bm25 {
        return Ok(None);
    }
    Ok(Some(Bm25Index::build(
        corpus,
        request.bm25_max_docs,
        request.bm25,
    )?))
}

/// One prediction, timed. The timer covers the real work of the system for
/// this input (observation, search, ranking).
fn predict_one<F>(
    request: &RunRequest<'_>,
    corpus: &Corpus,
    bm25: &Option<Bm25Index>,
    path: &Path,
    search: &F,
) -> Result<(Prediction, u64)>
where
    F: Fn(&str, usize) -> Result<Vec<String>>,
{
    if request.system == System::Kaseifu {
        let started = Instant::now();
        let prediction = predict_kaseifu(path, corpus, search)?;
        return Ok((prediction, started.elapsed().as_millis() as u64));
    }
    let source_doc_id = corpus
        .doc_id_of_path(path)
        .context("input document is not part of the corpus")?
        .to_string();
    let started = Instant::now();
    let prediction = match request.system {
        System::Bm25 => match bm25 {
            Some(index) => index.predict(&source_doc_id, corpus),
            None => Prediction::default(),
        },
        _ => predict_random(corpus, &source_doc_id, request.seed),
    };
    Ok((prediction, started.elapsed().as_millis() as u64))
}

/// Ground-truth membership for the prediction.
///
/// When the manifest embedded the ids, they are the authority; otherwise the
/// set is recomputed from the qrels the manifest names. Either way the size
/// recorded in the manifest must match, so a manifest cannot silently drift
/// away from the qrels it claims to come from.
pub fn ground_truth_contains(
    qrels: &Qrels,
    entry: &super::qrels::ManifestEntry,
    prediction: &Prediction,
) -> Result<bool> {
    let Some(predicted) = prediction.doc_id.as_deref() else {
        return Ok(false);
    };
    if let Some(ids) = &entry.ground_truth_doc_ids {
        return Ok(ids.iter().any(|doc_id| doc_id == predicted));
    }
    let truth = qrels.ground_truth(&entry.source_doc_id);
    if truth.len() != entry.ground_truth_size {
        anyhow::bail!(
            "ground truth size mismatch for {}: manifest {} vs qrels {}",
            entry.source_doc_id,
            entry.ground_truth_size,
            truth.len()
        );
    }
    Ok(truth.contains(predicted))
}

/// Inputs that were not evaluated, plus the diagnostic that explains a low
/// coverage without guessing at it.
struct RunCounts {
    skipped_missing_input: usize,
    inputs_with_empty_terms: usize,
}

/// Report timestamps, kept together so the report builder stays readable.
struct Timestamps {
    started_at: String,
    finished_at: String,
}

fn build_report(
    request: &RunRequest<'_>,
    corpus: &Corpus,
    bm25: &Option<Bm25Index>,
    rows: Vec<InputResult>,
    counts: RunCounts,
    stamps: Timestamps,
) -> RunReport {
    let inputs = rows.len();
    let search_scope = corpus.root().to_string_lossy().to_string();
    let chain = toolchain();
    RunReport {
        benchmark: BENCHMARK_ID.to_string(),
        metric: "hit@1".to_string(),
        system: request.system.as_str().to_string(),
        results: run_results(&rows),
        auxiliary: auxiliary_of(&rows),
        scattering: scattering_of(request.scatter),
        diagnostics: Diagnostics {
            inputs,
            inputs_with_empty_terms: counts.inputs_with_empty_terms,
            inputs_with_zero_candidates: rows.iter().filter(|row| row.candidate_count == 0).count(),
            skipped_missing_input: counts.skipped_missing_input,
        },
        reproducibility: reproduce(
            request,
            bm25,
            &chain,
            search_scope,
            stamps.started_at,
            stamps.finished_at,
        ),
        constraints: CONSTRAINT_LINES
            .iter()
            .map(|line| (*line).to_string())
            .collect(),
    }
}

/// Constraint texts, owned once so JSON, `compare` and tests agree.
pub const CONSTRAINT_LINES: [&str; 5] = CONSTRAINTS;

pub fn run_results(rows: &[InputResult]) -> RunResults {
    let evaluated = rows.len();
    let hits = rows.iter().filter(|row| row.hit).count();
    let with_result = rows
        .iter()
        .filter(|row| !row.predicted_doc_id.is_empty())
        .count();
    RunResults {
        evaluated,
        hits,
        hit_at_1: ratio(hits, evaluated),
        coverage: ratio(with_result, evaluated),
    }
}

pub fn auxiliary_of(rows: &[InputResult]) -> Auxiliary {
    let latency: Vec<f64> = rows.iter().map(|row| row.latency_ms as f64).collect();
    let candidates: Vec<f64> = rows.iter().map(|row| row.candidate_count as f64).collect();
    Auxiliary {
        no_result_rate: 1.0 - run_results(rows).coverage,
        latency_ms: distribution(&latency),
        candidate_count: distribution(&candidates),
    }
}

/// `hits / evaluated`, and 0.0 when nothing was evaluated (never NaN).
fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

/// Mean, median, nearest-rank p95 and max; all zero for an empty slice.
pub fn distribution(values: &[f64]) -> Distribution {
    if values.is_empty() {
        return Distribution {
            mean: 0.0,
            median: 0.0,
            p95: 0.0,
            max: 0.0,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sum: f64 = sorted.iter().sum();
    let rank = ((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len()) - 1;
    Distribution {
        mean: sum / sorted.len() as f64,
        median: sorted[sorted.len() / 2],
        p95: sorted[rank],
        max: sorted[sorted.len() - 1],
    }
}

fn scattering_of(scatter: &ScatterManifest) -> ScatteringInfo {
    ScatteringInfo {
        strategy: scatter.strategy.clone(),
        seed: scatter.seed,
        directory_count: scatter.directory_count,
        filename_strategy: scatter.filename_strategy.clone(),
        timestamp_strategy: scatter.timestamp_strategy.clone(),
    }
}

fn reproduce(
    request: &RunRequest<'_>,
    bm25: &Option<Bm25Index>,
    chain: &Toolchain,
    search_scope: String,
    started_at: String,
    finished_at: String,
) -> Reproducibility {
    let timeouts = request
        .timeout_counter
        .map(|counter| counter.load(Ordering::Relaxed))
        .unwrap_or(0);
    Reproducibility {
        kaseifu_commit: chain.kaseifu_commit.clone(),
        kaseifu_commit_dirty: chain.kaseifu_commit_dirty,
        rust_version: chain.rust_version.clone(),
        dataset: request.manifest.dataset.clone(),
        corpus: corpus_info(request.scatter),
        manifest: ManifestRef {
            path: request.manifest_path.to_string_lossy().to_string(),
            sha256: request.manifest.manifest_sha256.clone(),
        },
        macos: macos_version(),
        spotlight: spotlight_record(request.gate),
        settings: Settings {
            seed: request.seed,
            timeout_ms: request.timeout_ms,
            max_spotlight_hits: crate::discovery::MAX_SPOTLIGHT_HITS,
            search_scope,
            image_metadata_spotlight: true,
            image_metadata_exif_fallback: true,
            bm25: Bm25Settings {
                k1: request.bm25.k1,
                b: request.bm25.b,
                indexed_docs: bm25.as_ref().map(Bm25Index::indexed_docs).unwrap_or(0),
            },
        },
        information_source: request.system.information_source().to_string(),
        started_at,
        finished_at,
        timeouts,
    }
}

fn corpus_info(scatter: &ScatterManifest) -> CorpusInfo {
    CorpusInfo {
        files: scatter.files.len(),
        bytes: scatter.total_bytes,
        tree_sha256: scatter.tree_sha256.clone(),
        missing_documents: scatter.missing_doc_ids.len(),
    }
}

fn spotlight_record(gate: &IndexReport) -> SpotlightRecord {
    SpotlightRecord {
        ready: gate.ready,
        checked_at: gate.checked_at.clone(),
        indexing_enabled: gate.indexing_enabled,
        mdutil_volume_root: gate.mdutil_volume_root.clone(),
        mdutil_data_volume: gate.mdutil_data_volume.clone(),
        samples_total: gate.samples.len(),
        samples_found_by_name: gate
            .samples
            .iter()
            .filter(|probe| probe.found_by_name)
            .count(),
        samples_found_by_content: gate
            .samples
            .iter()
            .filter(|probe| probe.found_by_content == Some(true))
            .count(),
        timeout_ms: gate.timeout_ms,
    }
}

/// A run output path that never overwrites an earlier run.
///
/// `<system>-<tag>.<ext>` is used when free; otherwise `-2`, `-3`, ... is
/// appended (the JSON and CSV of one run share a tag, and either one may
/// collide with an earlier run's pair).
pub fn unique_output_path(dir: &Path, system: &str, tag: &str, extension: &str) -> PathBuf {
    let base = format!("{system}-{tag}");
    let mut candidate = dir.join(format!("{base}.{extension}"));
    let mut sequence = 2;
    while candidate.exists() && sequence < 10000 {
        candidate = dir.join(format!("{base}-{sequence}.{extension}"));
        sequence += 1;
    }
    candidate
}

/// Write the run JSON and the per-input CSV. Returns both paths.
pub fn write_outputs(
    workspace: &Workspace,
    report: &RunReport,
    rows: &[InputResult],
) -> Result<(PathBuf, PathBuf)> {
    let tag = timestamp_tag();
    let json_path = unique_output_path(&workspace.runs_dir(), &report.system, &tag, "json");
    let csv_path = unique_output_path(&workspace.runs_dir(), &report.system, &tag, "csv");
    super::write_json(&json_path, report)?;
    std::fs::create_dir_all(workspace.runs_dir())?;
    std::fs::write(&csv_path, csv_text(rows))
        .with_context(|| format!("cannot write {}", csv_path.display()))?;
    Ok((json_path, csv_path))
}

/// CSV with exactly the five columns of the request, in that order.
pub fn csv_text(rows: &[InputResult]) -> String {
    let mut out = String::from("source_doc_id,predicted_doc_id,hit,latency_ms,candidate_count\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            row.source_doc_id,
            row.predicted_doc_id,
            if row.hit { 1 } else { 0 },
            row.latency_ms,
            row.candidate_count
        ));
    }
    out
}

/// Collected runs, one per system (the newest file wins).
pub fn collect_runs(dir: &Path) -> Result<Vec<RunReport>> {
    let mut newest: std::collections::BTreeMap<String, (String, RunReport)> =
        std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("cannot read run directory {}", dir.display()))?
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(report) = super::read_json::<RunReport>(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        match newest.get(&report.system) {
            Some((existing, _)) if existing >= &name => {}
            _ => {
                newest.insert(report.system.clone(), (name, report));
            }
        }
    }
    Ok(newest.into_values().map(|(_, report)| report).collect())
}

/// ASCII comparison table (no colour, no box drawing beyond `+--` and `|`).
pub fn comparison_table(runs: &[RunReport]) -> String {
    let mut out = String::new();
    out.push_str("+----------+--------+----------+----------------+---------+------------+\n");
    out.push_str("| system   |  hit@1 | coverage | no result rate | mean ms | candidates |\n");
    out.push_str("+----------+--------+----------+----------------+---------+------------+\n");
    for report in runs {
        out.push_str(&format!(
            "| {:<8} | {:>6.3} | {:>8.3} | {:>14.3} | {:>7.1} | {:>10.1} |\n",
            report.system,
            report.results.hit_at_1,
            report.results.coverage,
            report.auxiliary.no_result_rate,
            report.auxiliary.latency_ms.mean,
            report.auxiliary.candidate_count.mean
        ));
    }
    out.push_str("+----------+--------+----------+----------------+---------+------------+\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, predicted: &str, hit: bool, latency: u64, candidates: usize) -> InputResult {
        InputResult {
            source_doc_id: id.to_string(),
            predicted_doc_id: predicted.to_string(),
            hit,
            latency_ms: latency,
            candidate_count: candidates,
        }
    }

    #[test]
    fn hit_at_1_uses_the_evaluated_denominator() {
        let rows = vec![
            row("A", "B", true, 10, 3),
            row("B", "F", false, 20, 2),
            row("C", "A", true, 30, 1),
        ];
        let results = run_results(&rows);
        assert_eq!(results.evaluated, 3);
        assert_eq!(results.hits, 2);
        assert!((results.hit_at_1 - 2.0 / 3.0).abs() < 1e-12);
        assert!((results.coverage - 1.0).abs() < 1e-12);
    }

    #[test]
    fn no_result_counts_as_a_miss_and_stays_in_the_denominator() {
        let rows = vec![row("A", "", false, 5, 0), row("B", "C", true, 5, 4)];
        let results = run_results(&rows);
        assert_eq!(results.evaluated, 2, "the empty result is not dropped");
        assert_eq!(results.hits, 1);
        assert!((results.hit_at_1 - 0.5).abs() < 1e-12);
        assert!((results.coverage - 0.5).abs() < 1e-12);
        let auxiliary = auxiliary_of(&rows);
        assert!((auxiliary.no_result_rate - 0.5).abs() < 1e-12);
        assert_eq!(auxiliary.candidate_count.mean, 2.0);
    }

    #[test]
    fn empty_run_is_zero_instead_of_nan() {
        let results = run_results(&[]);
        assert_eq!(results.evaluated, 0);
        assert_eq!(results.hit_at_1, 0.0);
        assert!(results.hit_at_1.is_finite());
        let distribution = distribution(&[]);
        assert_eq!(distribution.max, 0.0);
        assert!(distribution.mean.is_finite());
    }

    #[test]
    fn distribution_reports_mean_median_p95_and_max() {
        let values: Vec<f64> = (1..=100).map(|value| value as f64).collect();
        let stats = distribution(&values);
        assert!((stats.mean - 50.5).abs() < 1e-9);
        assert_eq!(stats.median, 51.0);
        assert_eq!(stats.p95, 95.0);
        assert_eq!(stats.max, 100.0);
    }

    #[test]
    fn csv_has_the_five_requested_columns() {
        let text = csv_text(&[
            row("DOC001", "DOC042", true, 120, 15),
            row("DOC002", "", false, 98, 0),
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "source_doc_id,predicted_doc_id,hit,latency_ms,candidate_count"
        );
        assert_eq!(lines[1], "DOC001,DOC042,1,120,15");
        assert_eq!(lines[2], "DOC002,,0,98,0");
    }

    #[test]
    fn comparison_table_is_ascii_and_lists_every_system() {
        let runs = vec![sample_report("kaseifu", 0.5), sample_report("bm25", 0.25)];
        let table = comparison_table(&runs);
        assert!(table.is_ascii());
        assert!(table.contains("kaseifu"));
        assert!(table.contains("bm25"));
        assert_eq!(
            table.lines().count(),
            6,
            "three borders, a header and two rows"
        );
        let widths: Vec<usize> = table.lines().map(|line| line.chars().count()).collect();
        assert!(
            widths.windows(2).all(|pair| pair[0] == pair[1]),
            "every table row shares one width: {widths:?}"
        );
    }

    #[test]
    fn output_paths_do_not_overwrite_an_earlier_run() {
        let dir = std::env::temp_dir().join(format!("kaseifu-bench-out-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_output_path(&dir, "kaseifu", "tag", "json");
        assert_eq!(first.file_name().unwrap(), "kaseifu-tag.json");
        std::fs::write(&first, "{}").unwrap();
        let second = unique_output_path(&dir, "kaseifu", "tag", "json");
        assert_eq!(second.file_name().unwrap(), "kaseifu-tag-2.json");
        // The CSV of the same run keeps its own extension, unaffected.
        let csv = unique_output_path(&dir, "kaseifu", "tag", "csv");
        assert_eq!(csv.file_name().unwrap(), "kaseifu-tag.csv");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn sample_report(system: &str, hit_at_1: f64) -> RunReport {
        RunReport {
            benchmark: BENCHMARK_ID.to_string(),
            metric: "hit@1".to_string(),
            system: system.to_string(),
            results: RunResults {
                evaluated: 4,
                hits: 2,
                hit_at_1,
                coverage: 0.75,
            },
            auxiliary: Auxiliary {
                no_result_rate: 0.25,
                latency_ms: distribution(&[10.0, 20.0]),
                candidate_count: distribution(&[1.0, 2.0]),
            },
            scattering: ScatteringInfo {
                strategy: "uniform".to_string(),
                seed: 42,
                directory_count: 100,
                filename_strategy: "deterministic-opaque".to_string(),
                timestamp_strategy: "fixed".to_string(),
            },
            diagnostics: Diagnostics {
                inputs: 4,
                inputs_with_empty_terms: 0,
                inputs_with_zero_candidates: 1,
                skipped_missing_input: 0,
            },
            reproducibility: Reproducibility {
                kaseifu_commit: "test".to_string(),
                kaseifu_commit_dirty: false,
                rust_version: "rustc test".to_string(),
                dataset: DatasetInfo {
                    name: "test".to_string(),
                    version: "test".to_string(),
                    source_url: "test".to_string(),
                    sha256: "test".to_string(),
                    bytes: 0,
                    topics_total: 0,
                    topics_missing: Vec::new(),
                    qrels_lines: 0,
                    positive_judgments: 0,
                    rel_histogram: std::collections::BTreeMap::new(),
                    judged_docs: 0,
                    positive_docs: 0,
                },
                corpus: CorpusInfo {
                    files: 4,
                    bytes: 4,
                    tree_sha256: "test".to_string(),
                    missing_documents: 0,
                },
                manifest: ManifestRef {
                    path: "manifests/d2d.json".to_string(),
                    sha256: "test".to_string(),
                },
                macos: MacOsVersion {
                    product_version: "test".to_string(),
                    build_version: "test".to_string(),
                    arch: "test".to_string(),
                },
                spotlight: SpotlightRecord {
                    ready: true,
                    checked_at: "test".to_string(),
                    indexing_enabled: Some(true),
                    mdutil_volume_root: "Indexing enabled.".to_string(),
                    mdutil_data_volume: "Indexing enabled.".to_string(),
                    samples_total: 1,
                    samples_found_by_name: 1,
                    samples_found_by_content: 1,
                    timeout_ms: 30000,
                },
                settings: Settings {
                    seed: 42,
                    timeout_ms: 30000,
                    max_spotlight_hits: 10000,
                    search_scope: "/tmp/scope".to_string(),
                    image_metadata_spotlight: true,
                    image_metadata_exif_fallback: true,
                    bm25: Bm25Settings {
                        k1: 1.2,
                        b: 0.75,
                        indexed_docs: 0,
                    },
                },
                information_source: "test".to_string(),
                started_at: "test".to_string(),
                finished_at: "test".to_string(),
                timeouts: 0,
            },
            constraints: CONSTRAINT_LINES
                .iter()
                .map(|line| (*line).to_string())
                .collect(),
        }
    }

    #[test]
    fn constraints_are_recorded_in_every_report() {
        let report = sample_report("kaseifu", 0.0);
        assert_eq!(report.constraints.len(), 5);
        assert!(report.constraints[0].contains("not an exhaustive ground truth"));
        assert!(report.constraints[3].contains("different information source"));
        let json = serde_json::to_string(&report).unwrap();
        let parsed: RunReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, report, "the report survives a JSON round trip");
        assert!(json.contains("\"results\":{\"evaluated\"") || json.contains("\"evaluated\""));
    }

    #[test]
    fn run_against_an_unready_index_is_refused() {
        let workspace = Workspace::from_root(PathBuf::from("/tmp/kaseifu-bench-refuse"));
        let qrels = Qrels::parse("301 0 A 1\n301 0 B 1\n").unwrap();
        let dataset = DatasetInfo {
            name: "t".to_string(),
            version: "t".to_string(),
            source_url: "t".to_string(),
            sha256: "t".to_string(),
            bytes: 0,
            topics_total: 1,
            topics_missing: Vec::new(),
            qrels_lines: 2,
            positive_judgments: 2,
            rel_histogram: std::collections::BTreeMap::new(),
            judged_docs: 2,
            positive_docs: 2,
        };
        let manifest = super::super::qrels::build_manifest(&qrels, dataset, 42, 0).unwrap();
        let scatter = ScatterManifest {
            benchmark: "t".to_string(),
            algorithm: "t".to_string(),
            strategy: "flat".to_string(),
            seed: 42,
            directory_count: 1,
            filename_strategy: "t".to_string(),
            timestamp_strategy: "fixed".to_string(),
            fixed_mtime_unix: 0,
            birthtime_settable: false,
            birthtime_note: String::new(),
            birthtime_observed_unix: None,
            root: "/tmp/kaseifu-bench-refuse/scattered".to_string(),
            selector: "t".to_string(),
            files: Vec::new(),
            missing_doc_ids: Vec::new(),
            total_bytes: 0,
            tree_sha256: "t".to_string(),
            manifest_sha256: "t".to_string(),
        };
        let gate = IndexReport {
            ready: false,
            reasons: vec!["not indexed".to_string()],
            root: "/tmp".to_string(),
            corpus_present: false,
            documents: 0,
            samples: Vec::new(),
            exclusions: Default::default(),
            mdutil_volume_root: String::new(),
            mdutil_data_volume: String::new(),
            indexing_enabled: None,
            macos: MacOsVersion {
                product_version: String::new(),
                build_version: String::new(),
                arch: String::new(),
            },
            checked_at: String::new(),
            timeout_ms: 10,
        };
        let request = RunRequest {
            workspace: &workspace,
            manifest: &manifest,
            manifest_path: PathBuf::from("/tmp/kaseifu-bench-refuse/manifests/d2d.json"),
            scatter: &scatter,
            qrels: &qrels,
            gate: &gate,
            system: System::Random,
            seed: 42,
            bm25: Bm25Params::default(),
            bm25_max_docs: 0,
            timeout_ms: 10,
            timeout_counter: None,
        };
        let error = evaluate(&request, |_, _| Ok(Vec::new()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not ready"), "{error}");
    }
}

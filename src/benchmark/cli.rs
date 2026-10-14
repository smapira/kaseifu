//! `kaseifu-bench` command line (research / evaluation interface).
//!
//! This is a separate binary on purpose: the request describes a research
//! interface, and keeping it out of the product CLI leaves `kaseifu --help`,
//! the zsh completion and the README contract untouched.
//!
//! Output is plain ASCII, never coloured, and identical whether or not stdout
//! is a terminal: the reports are the product of this tool.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use super::gate::{IndexReport, check_index, check_index_layout_only};
use super::qrels::{
    D2dManifest, DatasetInfo, Qrels, build_manifest, load_manifest, newest_manifest,
};
use super::run::{RunOutcome, RunRequest, collect_runs, comparison_table, evaluate, write_outputs};
use super::scatter::{
    STRATEGY_FLAT, STRATEGY_UNIFORM, ScatterManifest, cleanup, scatter, verify_scatter,
};
use super::search::ScopedSearch;
use super::systems::{Bm25Params, System};
use super::{
    BENCHMARK_ID, DEFAULT_INDEX_SAMPLES, DEFAULT_MAX_INPUTS, DEFAULT_SEED, DEFAULT_TIMEOUT_MS,
    Workspace,
};

/// Output format of the command itself (the run JSON/CSV files are always
/// written; this only controls what the terminal shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "kaseifu-bench",
    version,
    about = "kaseifu TREC benchmark (research interface, not part of the product workflow)",
    long_about = "Evaluates related-document selection (Hit@1) on a document-to-document\n\
ground truth derived from the official TREC Robust04 qrels.\n\
The corpus is only read: this tool never moves, deletes, renames or rewrites\n\
a corpus document, and it never searches outside the benchmark workspace."
)]
pub struct Cli {
    /// Benchmark workspace root.
    #[arg(long, global = true, value_name = "DIR")]
    pub workspace: Option<PathBuf>,

    /// Output format for this command.
    #[arg(long, global = true, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check that the dataset files exist and parse.
    Validate {
        /// Override the qrels path.
        #[arg(long, value_name = "FILE")]
        qrels: Option<PathBuf>,
    },

    /// Derive the D2D ground truth and write a frozen manifest.
    Prepare {
        /// Number of evaluated inputs (0 = every eligible input).
        #[arg(long, default_value_t = DEFAULT_MAX_INPUTS)]
        max_inputs: usize,

        #[arg(long, default_value_t = DEFAULT_SEED)]
        seed: u64,

        /// Override the qrels path.
        #[arg(long, value_name = "FILE")]
        qrels: Option<PathBuf>,
    },

    /// Ask Spotlight whether the scattered corpus is fully indexed.
    CheckIndex {
        /// Number of corpus documents probed.
        #[arg(long, default_value_t = DEFAULT_INDEX_SAMPLES)]
        samples: usize,

        #[arg(long, default_value_t = DEFAULT_TIMEOUT_MS)]
        timeout_ms: u64,
    },

    /// Copy the corpus into the workspace as scattered documents.
    Scatter {
        /// Directory holding one file per document (default: <workspace>/corpus).
        #[arg(long, value_name = "DIR")]
        source: Option<PathBuf>,

        /// flat (one directory) or uniform (seeded random directories).
        #[arg(long, default_value = STRATEGY_UNIFORM)]
        strategy: String,

        #[arg(long, default_value_t = DEFAULT_SEED)]
        seed: u64,

        /// Directory count for --strategy uniform.
        #[arg(long, default_value_t = 100)]
        directories: usize,
    },

    /// Re-check the scattered corpus against its manifest.
    VerifyScatter,

    /// Remove the files this benchmark created (dry run by default).
    Cleanup {
        #[arg(long)]
        execute: bool,

        /// Confirm the deletion (required together with --execute).
        #[arg(long, requires = "execute")]
        yes: bool,
    },

    /// Evaluate one system over the frozen manifest.
    Run {
        /// kaseifu, bm25 or random.
        #[arg(long, default_value = "kaseifu")]
        system: String,

        #[arg(long, default_value_t = DEFAULT_SEED)]
        seed: u64,

        /// Per-mdfind timeout in milliseconds.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_MS)]
        timeout_ms: u64,

        /// BM25 index size guard (0 = index every corpus document).
        #[arg(long, default_value_t = 0)]
        bm25_max_docs: usize,

        /// Override the qrels path.
        #[arg(long, value_name = "FILE")]
        qrels: Option<PathBuf>,
    },

    /// Measure the Phase 9 numbers (FFI, inference, session reuse, folder
    /// sampling, video, memory) on this machine.
    Perf {
        /// Repetitions of every measured call.
        #[arg(long, default_value_t = 3)]
        generations: usize,

        /// Reuse one inference session per instruction set (the product
        /// creates one session per generation).
        #[arg(long)]
        reuse_session: bool,

        /// Also generate for two different documents through one reused
        /// session and report whether the first one bleeds into the second.
        #[arg(long)]
        bleed: bool,

        /// Entries in the synthetic folder used for the sampling measurement.
        #[arg(long, default_value_t = 260)]
        folder_entries: usize,

        /// A PNG or JPEG to time Apple Vision on.
        #[arg(long, value_name = "FILE")]
        image: Option<PathBuf>,

        /// A video to time AVFoundation and Vision on (any duration).
        #[arg(long, value_name = "FILE")]
        video: Option<PathBuf>,
    },

    /// Compare the newest run of every system.
    Compare,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn workspace_of(cli: &Cli) -> Result<Workspace> {
    Ok(match &cli.workspace {
        Some(root) => Workspace::from_root(crate::benchmark::normalize_path(root)),
        None => Workspace::from_default_root()?,
    })
}

fn qrels_path_of(workspace: &Workspace, override_path: Option<&PathBuf>) -> PathBuf {
    override_path
        .cloned()
        .unwrap_or_else(|| workspace.qrels_path())
}

fn dispatch(cli: &Cli) -> Result<ExitCode> {
    let workspace = workspace_of(cli)?;
    match &cli.command {
        Command::Validate { qrels } => validate(cli, &workspace, qrels.as_ref()),
        Command::Prepare {
            max_inputs,
            seed,
            qrels,
        } => prepare(cli, &workspace, qrels.as_ref(), *seed, *max_inputs),
        Command::CheckIndex {
            samples,
            timeout_ms,
        } => check_index_command(cli, &workspace, *samples, *timeout_ms),
        Command::Scatter {
            source,
            strategy,
            seed,
            directories,
        } => scatter_command(
            cli,
            &workspace,
            source.as_ref(),
            strategy,
            *seed,
            *directories,
        ),
        Command::VerifyScatter => verify_scatter_command(cli, &workspace),
        Command::Cleanup { execute, yes } => cleanup_command(cli, &workspace, *execute, *yes),
        Command::Run {
            system,
            seed,
            timeout_ms,
            bm25_max_docs,
            qrels,
        } => run_command(
            cli,
            &workspace,
            System::parse(system)?,
            *seed,
            *timeout_ms,
            *bm25_max_docs,
            qrels.as_ref(),
        ),
        Command::Compare => compare_command(cli, &workspace),
        Command::Perf {
            generations,
            reuse_session,
            bleed,
            folder_entries,
            image,
            video,
        } => perf_command(
            cli,
            super::perf::PerfRequest {
                generations: *generations,
                reuse_session: *reuse_session,
                bleed: *bleed,
                folder_entries: *folder_entries,
                image: image.clone(),
                video: video.clone(),
                timeout_ms: DEFAULT_TIMEOUT_MS,
            },
        ),
    }
}

/// `perf`: measure and print (this command never writes to the workspace).
fn perf_command(cli: &Cli, request: super::perf::PerfRequest) -> Result<ExitCode> {
    let report = super::perf::run(&request)?;
    match cli.format {
        Format::Json => emit_json(&report)?,
        Format::Text => super::perf::print_text(&report),
    }
    Ok(ExitCode::SUCCESS)
}

fn emit_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct ValidationReport {
    benchmark: String,
    workspace: String,
    qrels: QrelsReport,
    corpus: CorpusReport,
}

#[derive(serde::Serialize)]
struct QrelsReport {
    path: String,
    present: bool,
    bytes: u64,
    sha256: String,
    lines: u64,
    topics_total: u64,
    topics_missing: Vec<u32>,
    positive_judgments: u64,
    positive_docs: u64,
    judged_docs: u64,
    rel_histogram: std::collections::BTreeMap<String, u64>,
}

#[derive(serde::Serialize)]
struct CorpusReport {
    path: String,
    present: bool,
    documents: usize,
    root_excluded_by_discovery_rules: bool,
    note: String,
}

fn validate(cli: &Cli, workspace: &Workspace, override_path: Option<&PathBuf>) -> Result<ExitCode> {
    let qrels_path = qrels_path_of(workspace, override_path);
    if !qrels_path.is_file() {
        println!("FAIL qrels: not found: {}", qrels_path.display());
        println!(
            "note: download it from {} and place it in {}",
            super::qrels::QRELS_URL,
            workspace.datasets_dir().display()
        );
        println!("note: this tool never fabricates data; the run stops here");
        return Ok(ExitCode::from(1));
    }
    let qrels = Qrels::load(&qrels_path)?;
    let dataset = DatasetInfo::from_file(&qrels_path, &qrels)?;
    let corpus_path = workspace.corpus_dir();
    let documents = count_documents(&corpus_path);
    let root_excluded =
        crate::discovery::is_excluded_destination(corpus_path.to_string_lossy().as_ref());

    let report = ValidationReport {
        benchmark: BENCHMARK_ID.to_string(),
        workspace: workspace.root().to_string_lossy().to_string(),
        qrels: QrelsReport {
            path: qrels_path.to_string_lossy().to_string(),
            present: true,
            bytes: dataset.bytes,
            sha256: dataset.sha256.clone(),
            lines: qrels.summary.qrels_lines,
            topics_total: qrels.summary.topics_total,
            topics_missing: qrels.summary.topics_missing.clone(),
            positive_judgments: qrels.summary.positive_judgments,
            positive_docs: qrels.summary.positive_docs,
            judged_docs: qrels.summary.judged_docs,
            rel_histogram: qrels.summary.rel_histogram.clone(),
        },
        corpus: CorpusReport {
            path: corpus_path.to_string_lossy().to_string(),
            present: documents > 0,
            documents,
            root_excluded_by_discovery_rules: root_excluded,
            note: if documents == 0 {
                "no documents found; qrels-only work (prepare) can continue".to_string()
            } else {
                "corpus present; scatter it before running an evaluation".to_string()
            },
        },
    };

    if cli.format == Format::Json {
        emit_json(&report)?;
    } else {
        println!("kaseifu-bench validate");
        println!("benchmark: {}", report.benchmark);
        println!("workspace: {}", report.workspace);
        println!(
            "PASS qrels: {} ({} B, sha256 {})",
            qrels_path.display(),
            report.qrels.bytes,
            report.qrels.sha256
        );
        println!(
            "     lines {} / topics {} / missing {:?} / positive judgments {} / positive docs {} / judged docs {}",
            report.qrels.lines,
            report.qrels.topics_total,
            report.qrels.topics_missing,
            report.qrels.positive_judgments,
            report.qrels.positive_docs,
            report.qrels.judged_docs
        );
        println!("     relevance grades: {:?}", report.qrels.rel_histogram);
        if report.corpus.present {
            println!(
                "PASS corpus: {} ({} document(s))",
                report.corpus.path, report.corpus.documents
            );
        } else {
            println!(
                "SKIP corpus: not present at {} (qrels-only work can continue)",
                report.corpus.path
            );
        }
    }

    if root_excluded {
        println!(
            "FAIL corpus root: dropped by discovery exclusion rules (src/discovery/destinations.rs:112): {}",
            corpus_path.display()
        );
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn count_documents(dir: &Path) -> usize {
    if !dir.is_dir() {
        return 0;
    }
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file() && !entry.file_name().to_string_lossy().starts_with('.')
        })
        .count()
}

// ---------------------------------------------------------------------------
// prepare
// ---------------------------------------------------------------------------

fn prepare(
    cli: &Cli,
    workspace: &Workspace,
    override_path: Option<&PathBuf>,
    seed: u64,
    max_inputs: usize,
) -> Result<ExitCode> {
    let qrels_path = qrels_path_of(workspace, override_path);
    if !qrels_path.is_file() {
        println!("FAIL qrels: not found: {}", qrels_path.display());
        return Ok(ExitCode::from(1));
    }
    workspace.ensure_layout()?;
    let qrels = Qrels::load(&qrels_path)?;
    let dataset = DatasetInfo::from_file(&qrels_path, &qrels)?;
    let manifest = build_manifest(&qrels, dataset, seed, max_inputs)?;
    let file_name = if max_inputs == 0 {
        format!("d2d-seed{seed}-all.json")
    } else {
        format!("d2d-seed{seed}-{max_inputs}.json")
    };
    let path = workspace.manifests_dir().join(file_name);
    super::write_json(&path, &manifest)?;

    if cli.format == Format::Json {
        emit_json(&manifest)?;
    } else {
        println!("kaseifu-bench prepare");
        println!(
            "inputs: {} selected / {} eligible (seed {}, algorithm {})",
            manifest.sampling.selected,
            manifest.sampling.eligible_inputs,
            manifest.sampling.seed,
            manifest.sampling.algorithm
        );
        println!(
            "excluded inputs without ground truth: {}",
            manifest.excluded_no_ground_truth
        );
        println!("manifest: {}", path.display());
        println!("manifest sha256: {}", manifest.manifest_sha256);
        println!(
            "derivation: {} (positive only, self excluded, negative judgments unused)",
            manifest.derivation.rule
        );
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// check-index
// ---------------------------------------------------------------------------

fn check_index_command(
    cli: &Cli,
    workspace: &Workspace,
    samples: usize,
    timeout_ms: u64,
) -> Result<ExitCode> {
    let report = load_gate(workspace, samples, timeout_ms)?;
    if cli.format == Format::Json {
        emit_json(&report)?;
    } else {
        println!("kaseifu-bench check-index");
        println!("corpus root: {}", report.root);
        println!(
            "documents: {} (corpus present: {})",
            report.documents, report.corpus_present
        );
        println!(
            "macOS: {} (build {}) {}",
            report.macos.product_version, report.macos.build_version, report.macos.arch
        );
        println!(
            "spotlight indexing: {:?} / mdutil volume root: {} / data volume: {}",
            report.indexing_enabled, report.mdutil_volume_root, report.mdutil_data_volume
        );
        for probe in &report.samples {
            println!(
                "probe {}: by name {} / by content {:?}",
                probe.relative_path, probe.found_by_name, probe.found_by_content
            );
        }
        println!(
            "directories checked for exclusion: {} (excluded {})",
            report.exclusions.checked_directories,
            report.exclusions.excluded_directories.len()
        );
        println!("checked at: {}", report.checked_at);
        if report.ready {
            println!("PASS index: ready (an evaluation may start)");
        } else {
            for reason in &report.reasons {
                println!("FAIL index: {reason}");
            }
            println!("note: results measured against an incomplete index are not results");
        }
    }
    if report.ready {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

/// Read the scattering manifest if it exists, then probe the index.
fn load_gate(workspace: &Workspace, samples: usize, timeout_ms: u64) -> Result<IndexReport> {
    match load_scatter_manifest(workspace)? {
        Some(manifest) => check_index(workspace, &manifest, samples, timeout_ms),
        None => check_index_layout_only(workspace, timeout_ms),
    }
}

fn load_scatter_manifest(workspace: &Workspace) -> Result<Option<ScatterManifest>> {
    let path = workspace.scatter_manifest_path();
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(super::read_json(&path)?))
}

// ---------------------------------------------------------------------------
// scatter / verify-scatter / cleanup
// ---------------------------------------------------------------------------

fn scatter_command(
    cli: &Cli,
    workspace: &Workspace,
    source: Option<&PathBuf>,
    strategy: &str,
    seed: u64,
    directories: usize,
) -> Result<ExitCode> {
    let source = source.cloned().unwrap_or_else(|| workspace.corpus_dir());
    if !source.is_dir() {
        println!(
            "FAIL corpus: not present at {} (place the TREC documents there first)",
            source.display()
        );
        return Ok(ExitCode::from(1));
    }
    if strategy != STRATEGY_FLAT && strategy != STRATEGY_UNIFORM {
        println!("FAIL strategy: {strategy} (expected {STRATEGY_FLAT} or {STRATEGY_UNIFORM})");
        return Ok(ExitCode::from(1));
    }
    workspace.ensure_layout()?;
    let known: Vec<String> = qrels_candidate(workspace)
        .map(|path| Qrels::load(&path).map(|qrels| qrels.positive_doc_ids()))
        .transpose()?
        .unwrap_or_default();
    let manifest = scatter(workspace, &source, strategy, seed, directories, &known)?;
    let path = workspace.scatter_manifest_path();
    super::write_json(&path, &manifest)?;

    if cli.format == Format::Json {
        emit_json(&manifest)?;
    } else {
        println!("kaseifu-bench scatter");
        println!("source: {}", source.display());
        println!("scattered to: {}", manifest.root);
        println!(
            "strategy {} / seed {} / directories {} / files {} / bytes {}",
            manifest.strategy,
            manifest.seed,
            manifest.directory_count,
            manifest.files.len(),
            manifest.total_bytes
        );
        println!(
            "filename {} / timestamp {} (fixed mtime {})",
            manifest.filename_strategy, manifest.timestamp_strategy, manifest.fixed_mtime_unix
        );
        println!("birthtime settable: {}", manifest.birthtime_settable);
        println!("manifest: {}", path.display());
        println!("manifest sha256: {}", manifest.manifest_sha256);
        println!("tree sha256: {}", manifest.tree_sha256);
    }
    Ok(ExitCode::SUCCESS)
}

fn qrels_candidate(workspace: &Workspace) -> Option<PathBuf> {
    let path = workspace.qrels_path();
    path.is_file().then_some(path)
}

fn verify_scatter_command(cli: &Cli, workspace: &Workspace) -> Result<ExitCode> {
    let Some(manifest) = load_scatter_manifest(workspace)? else {
        println!(
            "FAIL scatter manifest: not found at {}",
            workspace.scatter_manifest_path().display()
        );
        return Ok(ExitCode::from(1));
    };
    let report = verify_scatter(&manifest)?;
    if cli.format == Format::Json {
        emit_json(&report)?;
    } else {
        println!("kaseifu-bench verify-scatter");
        println!(
            "checked {} file(s): missing {} / size {} / digest {} / mtime {} / extra {}",
            report.checked,
            report.missing.len(),
            report.size_mismatch.len(),
            report.digest_mismatch.len(),
            report.mtime_mismatch.len(),
            report.extra_files.len()
        );
        for path in report
            .missing
            .iter()
            .chain(&report.size_mismatch)
            .chain(&report.digest_mismatch)
            .chain(&report.mtime_mismatch)
            .chain(&report.extra_files)
            .take(5)
        {
            println!("  {path}");
        }
        if report.ok() {
            println!("PASS scatter: every file matches the manifest");
        } else {
            println!("FAIL scatter: the scattered corpus drifted from the manifest");
        }
    }
    if report.ok() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn cleanup_command(cli: &Cli, workspace: &Workspace, execute: bool, yes: bool) -> Result<ExitCode> {
    let Some(manifest) = load_scatter_manifest(workspace)? else {
        println!(
            "FAIL scatter manifest: not found at {}",
            workspace.scatter_manifest_path().display()
        );
        return Ok(ExitCode::from(1));
    };
    if execute && !yes {
        println!("FAIL cleanup: --execute needs --yes (nothing was deleted)");
        return Ok(ExitCode::from(1));
    }
    let report = cleanup(&manifest, execute)?;
    if cli.format == Format::Json {
        emit_json(&report)?;
    } else {
        println!("kaseifu-bench cleanup");
        println!(
            "targets {} / removed files {} / removed bytes {}",
            report.targets, report.removed_files, report.removed_bytes
        );
        if execute {
            println!("PASS cleanup: only files listed in the manifest were removed");
        } else {
            println!("dry run: nothing was deleted (add --execute --yes to delete)");
        }
        println!("note: the raw corpus is never touched by cleanup");
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// run / compare
// ---------------------------------------------------------------------------

fn run_command(
    cli: &Cli,
    workspace: &Workspace,
    system: System,
    seed: u64,
    timeout_ms: u64,
    bm25_max_docs: usize,
    qrels_override: Option<&PathBuf>,
) -> Result<ExitCode> {
    let (manifest_path, manifest) = load_manifest_for_run(workspace, cli.format)?;
    let Some(scatter_manifest) = load_scatter_manifest(workspace)? else {
        println!(
            "FAIL corpus: no scattering manifest at {} (run scatter first; this needs the TREC corpus)",
            workspace.scatter_manifest_path().display()
        );
        return Ok(ExitCode::from(1));
    };
    let qrels_path = qrels_path_of(workspace, qrels_override);
    let qrels = Qrels::load(&qrels_path)
        .with_context(|| format!("cannot read qrels: {}", qrels_path.display()))?;
    let gate = check_index(
        workspace,
        &scatter_manifest,
        DEFAULT_INDEX_SAMPLES,
        timeout_ms,
    )?;
    if !gate.ready {
        println!(
            "FAIL index: refusing to evaluate ({} reason(s))",
            gate.reasons.len()
        );
        for reason in &gate.reasons {
            println!("  {reason}");
        }
        println!("note: results measured against an incomplete index are not results");
        return Ok(ExitCode::from(1));
    }

    let search = ScopedSearch::new(scatter_manifest.root.clone(), timeout_ms);
    let request = RunRequest {
        workspace,
        manifest: &manifest,
        manifest_path: manifest_path.clone(),
        scatter: &scatter_manifest,
        qrels: &qrels,
        gate: &gate,
        system,
        seed,
        bm25: Bm25Params::default(),
        bm25_max_docs,
        timeout_ms,
        timeout_counter: Some(search.timeout_counter()),
    };
    let outcome = evaluate(&request, |query, limit| search.search(query, limit))?;
    let (json_path, csv_path) = write_outputs(workspace, &outcome.report, &outcome.rows)?;
    print_run(cli, &outcome, &json_path, &csv_path);
    Ok(ExitCode::SUCCESS)
}

fn load_manifest_for_run(workspace: &Workspace, format: Format) -> Result<(PathBuf, D2dManifest)> {
    let path = newest_manifest(&workspace.manifests_dir()).with_context(|| {
        format!(
            "no manifest in {} (run prepare first)",
            workspace.manifests_dir().display()
        )
    })?;
    let manifest = load_manifest(&path)?;
    if format == Format::Text {
        println!("manifest: {}", path.display());
    }
    Ok((path, manifest))
}

fn print_run(cli: &Cli, outcome: &RunOutcome, json_path: &Path, csv_path: &Path) {
    if cli.format == Format::Json {
        let _ = emit_json(&outcome.report);
        return;
    }
    let report = &outcome.report;
    println!("kaseifu-bench run --system {}", report.system);
    println!(
        "evaluated {} / hits {} / hit@1 {:.4} / coverage {:.4} / no result rate {:.4}",
        report.results.evaluated,
        report.results.hits,
        report.results.hit_at_1,
        report.results.coverage,
        report.auxiliary.no_result_rate
    );
    println!(
        "latency ms: mean {:.1} / median {:.1} / p95 {:.1} / max {:.1}",
        report.auxiliary.latency_ms.mean,
        report.auxiliary.latency_ms.median,
        report.auxiliary.latency_ms.p95,
        report.auxiliary.latency_ms.max
    );
    println!(
        "candidates: mean {:.1} / median {:.1} / max {:.1} (zero-candidate inputs {})",
        report.auxiliary.candidate_count.mean,
        report.auxiliary.candidate_count.median,
        report.auxiliary.candidate_count.max,
        report.diagnostics.inputs_with_zero_candidates
    );
    println!(
        "scattering: {} seed {} dirs {} / {} / mtime {}",
        report.scattering.strategy,
        report.scattering.seed,
        report.scattering.directory_count,
        report.scattering.filename_strategy,
        report.scattering.timestamp_strategy
    );
    println!("info source: {}", report.reproducibility.information_source);
    println!("json: {}", json_path.display());
    println!("csv:  {}", csv_path.display());
    println!("constraints:");
    for line in &report.constraints {
        println!("  - {line}");
    }
}

fn compare_command(cli: &Cli, workspace: &Workspace) -> Result<ExitCode> {
    let runs = collect_runs(&workspace.runs_dir())?;
    if runs.is_empty() {
        println!("no runs found in {}", workspace.runs_dir().display());
        return Ok(ExitCode::from(1));
    }
    if cli.format == Format::Json {
        let rows: Vec<serde_json::Value> = runs
            .iter()
            .map(|report| {
                serde_json::json!({
                    "system": report.system,
                    "evaluated": report.results.evaluated,
                    "hits": report.results.hits,
                    "hit_at_1": report.results.hit_at_1,
                    "coverage": report.results.coverage,
                    "no_result_rate": report.auxiliary.no_result_rate,
                    "latency_ms_mean": report.auxiliary.latency_ms.mean,
                    "candidate_count_mean": report.auxiliary.candidate_count.mean,
                    "information_source": report.reproducibility.information_source,
                })
            })
            .collect();
        emit_json(&rows)?;
        return Ok(ExitCode::SUCCESS);
    }
    print!("{}", comparison_table(&runs));
    println!("runs directory: {}", workspace.runs_dir().display());
    println!(
        "note: this is a derived D2D evaluation; the numbers are not comparable to the official TREC Q2D scores"
    );
    for report in &runs {
        println!(
            "  {}: evaluated {} with information source: {}",
            report.system, report.results.evaluated, report.reproducibility.information_source
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_is_ascii_and_lists_every_subcommand() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(help.is_ascii(), "help must stay ASCII: {help}");
        for name in [
            "validate",
            "prepare",
            "check-index",
            "scatter",
            "verify-scatter",
            "cleanup",
            "run",
            "compare",
        ] {
            assert!(help.contains(name), "missing subcommand {name}");
        }
    }

    #[test]
    fn workspace_flag_is_global_and_defaults_to_none() {
        let cli = Cli::try_parse_from(["kaseifu-bench", "validate"]).unwrap();
        assert!(cli.workspace.is_none());
        assert_eq!(cli.format, Format::Text);
        let cli = Cli::try_parse_from([
            "kaseifu-bench",
            "--workspace",
            "/tmp/ws",
            "--format",
            "json",
            "compare",
        ])
        .unwrap();
        assert_eq!(cli.workspace, Some(PathBuf::from("/tmp/ws")));
        assert_eq!(cli.format, Format::Json);
    }

    #[test]
    fn run_defaults_match_the_documented_settings() {
        let cli = Cli::try_parse_from(["kaseifu-bench", "run", "--system", "bm25"]).unwrap();
        match cli.command {
            Command::Run {
                system,
                seed,
                timeout_ms,
                bm25_max_docs,
                qrels,
            } => {
                assert_eq!(system, "bm25");
                assert_eq!(seed, DEFAULT_SEED);
                assert_eq!(timeout_ms, DEFAULT_TIMEOUT_MS);
                assert_eq!(bm25_max_docs, 0);
                assert_eq!(qrels, None);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cleanup_requires_yes_only_with_execute() {
        assert!(Cli::try_parse_from(["kaseifu-bench", "cleanup"]).is_ok());
        assert!(Cli::try_parse_from(["kaseifu-bench", "cleanup", "--execute"]).is_ok());
        assert!(
            Cli::try_parse_from(["kaseifu-bench", "cleanup", "--yes"]).is_err(),
            "--yes has no meaning without --execute"
        );
    }
}

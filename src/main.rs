use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use kaseifu::classifier::jev::{
    JEV_API_KEY_ENV, JEV_API_URL_ENV, JevError, JevIntegration, JevResponse, RealJevIntegration,
};
use kaseifu::classifier::pipeline;
use kaseifu::models::decision::DecisionContext;
use kaseifu::operations::organize::{self, OrganizeOutcome};
use kaseifu::operations::plan::PlanError;
use kaseifu::operations::undo;
use kaseifu::platform::macos::fsevents;
use kaseifu::presentation::renderers::{
    classify as classify_view, discover as discover_view, generic, inspect as inspect_view,
};
use kaseifu::presentation::{OutputFormat, resolve_mode};
use kaseifu::storage::history::{DEFAULT_DATABASE_PATH, OperationHistory};
use kaseifu::watch::coordinator::WatchConfig;
use kaseifu::watch::runner;
use kaseifu::{context, discovery, normalize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[derive(Parser)]
#[command(name = "kaseifu")]
#[command(version, about = "Semantic file organization for macOS")]
struct Cli {
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Auto)]
    format: OutputFormat,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build and display FileContext JSON.
    Inspect {
        path: PathBuf,
        /// Show all directory entries and evidence (default: up to 5).
        #[arg(long)]
        all: bool,
        /// Show the DETAILS layer (full paths, schema, raw metadata).
        #[arg(long)]
        verbose: bool,
    },

    /// Scan a directory.
    Scan { path: PathBuf },

    /// Organize a file: decision -> policy -> plan -> (dry-run or move).
    ///
    /// Dry run is the default; pass `--execute` to actually move the file.
    /// Every executed move is journaled and undoable with `undo`.
    Organize {
        file: PathBuf,
        /// Actually perform the move (default is a dry run).
        #[arg(long, conflicts_with = "dry_run")]
        execute: bool,
        /// Explicitly request a dry run (the default).
        #[arg(long)]
        dry_run: bool,
    },

    /// Undo previous operations.
    Undo {
        #[arg(long, default_value_t = 1)]
        last: usize,
    },

    /// Discover candidate destination directories
    ///
    /// Searches for similar files using Spotlight and aggregates
    /// parent directories with scoring.
    Discover {
        file: PathBuf,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        verbose: bool,
    },

    /// Run the full decision pipeline for one file (Phase 4).
    ///
    /// Observe -> Normalize -> Discover -> Jev -> validated JevDecision.
    /// This command never moves the file (execution is a later phase).
    Classify {
        file: PathBuf,
        /// Show all ranked candidates (default: top 5).
        #[arg(long)]
        all: bool,
        /// Show extra detail (full paths, evidence breakdowns).
        #[arg(long)]
        verbose: bool,
    },

    /// Watch directories and organize changed files automatically (Phase 6).
    ///
    /// Events are filtered, debounced, deduplicated and stability-checked
    /// before the existing pipeline runs. Dry run by default; pass
    /// `--execute` to actually move files.
    Watch {
        /// Directory to watch (repeatable). Never implicit: home-wide
        /// watching is not a default.
        #[arg(long = "path", required = true)]
        paths: Vec<PathBuf>,
        /// Actually perform moves (default is a dry run).
        #[arg(long, conflicts_with = "dry_run")]
        execute: bool,
        /// Explicitly request a dry run (the default).
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let format = cli.format;
    match cli.command {
        Commands::Inspect { path, all, verbose } => handle_inspect(&path, all, verbose, format),
        Commands::Scan { path } => {
            handle_scan(&path, format);
            Ok(())
        }
        Commands::Organize {
            file,
            execute,
            dry_run,
        } => handle_organize(&file, execute, dry_run, format).await,
        Commands::Watch {
            paths,
            execute,
            dry_run,
        } => handle_watch(&paths, execute, dry_run, format).await,
        Commands::Undo { last } => handle_undo(last, format),
        Commands::Discover { file, all, verbose } => handle_discover(&file, all, verbose, format),
        Commands::Classify { file, all, verbose } => {
            handle_classify(&file, all, verbose, format).await
        }
    }
}

/// `inspect`: one observation pass, Rich or JSON (read-only).
fn handle_inspect(path: &Path, all: bool, verbose: bool, format: OutputFormat) -> Result<()> {
    let mode = resolve_mode(format);
    let ctx = context::builder::build(path)?;
    if mode.rich {
        let normalized = normalize::normalize(ctx.clone())?;
        inspect_view::print_rich(&ctx, &normalized, all, verbose, mode.color);
    } else {
        println!("{}", serde_json::to_string_pretty(&ctx)?);
    }
    Ok(())
}

/// `scan`: target banner only (no mutation, no discovery).
fn handle_scan(path: &Path, format: OutputFormat) {
    let mode = resolve_mode(format);
    if mode.rich {
        println!(
            "{} {}",
            generic::header("KASEIFU / SCAN", mode.color),
            generic::status_line("target", &path.display().to_string(), mode.color)
        );
    } else {
        println!("Scanning: {}", path.display());
    }
}

/// `organize`: decision -> policy -> plan -> dry-run or move (journaled).
async fn handle_organize(
    file: &Path,
    execute: bool,
    dry_run: bool,
    format: OutputFormat,
) -> Result<()> {
    let actually_move = execute && !dry_run;

    let jev = build_jev_integration()?;
    let history = OperationHistory::open(DEFAULT_DATABASE_PATH)?;

    let report = organize::organize_file(file, &jev, &history, actually_move).await?;

    println!("== Decision ==");
    println!("  destination: {:?}", report.decision.destination_id);
    println!("  confidence:  {:.3}", report.decision.confidence);
    println!("  margin:      {:.3}", report.decision.margin);
    let mode = resolve_mode(format);
    if mode.rich {
        println!(
            "{} {}",
            generic::header("KASEIFU / ORGANIZE", mode.color),
            generic::dry_run_banner(actually_move, mode.color)
        );
    }
    println!("== Policy ==\n  {:?}", report.policy);

    print_organize_outcome(&report, file);
    Ok(())
}

/// Outcome banner for `organize` (pure display over the report).
fn print_organize_outcome(report: &organize::OrganizeReport, file: &Path) {
    match &report.outcome {
        OrganizeOutcome::Moved {
            operation_id,
            destination,
        } => {
            println!("== Moved ==");
            println!("  {}", destination.display());
            println!("  operation: {operation_id}");
        }
        OrganizeOutcome::WouldMove { destination } => {
            println!("== Dry run ==");
            println!(
                "  would move {} -> {}",
                file.display(),
                destination.display()
            );
        }
        OrganizeOutcome::Refused => {
            println!("== No move ==\n  policy refused");
        }
        OrganizeOutcome::Unplanable(PlanError::Collision(path)) => {
            // docs/design/05-execute.md: a collision goes to REVIEW.
            println!(
                "== REVIEW ==\n  destination already exists: {}",
                path.display()
            );
        }
        OrganizeOutcome::Unplanable(error) => {
            println!("== No move ==\n  {error:?}");
        }
        OrganizeOutcome::MoveFailed(error) => {
            println!("== Move failed ==\n  {error}");
        }
    }
}

/// `watch`: validate roots, then run the debounced FSEvents loop.
async fn handle_watch(
    paths: &[PathBuf],
    execute: bool,
    dry_run: bool,
    format: OutputFormat,
) -> Result<()> {
    let actually_move = execute && !dry_run;

    for path in paths {
        if !path.is_dir() {
            anyhow::bail!("watch path is not a directory: {}", path.display());
        }
    }

    let jev = build_jev_integration()?;
    let history = OperationHistory::open(DEFAULT_DATABASE_PATH)?;

    let (sender, receiver) = std::sync::mpsc::channel();
    let _watcher = fsevents::spawn(paths, sender)?;

    print_watch_start(paths, actually_move, format);
    runner::run_loop(
        receiver,
        &jev,
        &history,
        actually_move,
        WatchConfig::default(),
    )
    .await?;
    Ok(())
}

/// `watch` start banner (dry-run state is always explicit).
fn print_watch_start(paths: &[PathBuf], actually_move: bool, format: OutputFormat) {
    let roots = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mode = resolve_mode(format);
    if mode.rich {
        println!(
            "{}",
            generic::watch_line("start", &roots, !actually_move, mode.color)
        );
    } else {
        println!("watching {roots} (dry-run: {})", !actually_move);
    }
}

/// `undo`: restore up to `last` moves, newest first.
fn handle_undo(last: usize, format: OutputFormat) -> Result<()> {
    let mode = resolve_mode(format);
    if mode.rich {
        println!("{}", generic::header("KASEIFU / UNDO", mode.color));
    }
    let history = OperationHistory::open(DEFAULT_DATABASE_PATH)?;

    for _ in 0..last {
        if !apply_single_undo(&history) {
            break;
        }
    }
    Ok(())
}

/// One undo step; returns false when the loop must stop.
fn apply_single_undo(history: &OperationHistory) -> bool {
    match undo::undo_last_move(history) {
        Ok(outcome) => {
            println!(
                "restored: {}\n  (undid {})",
                outcome.restored_to.display(),
                outcome.undone_move_id
            );
            true
        }
        Err(undo::UndoError::NothingToUndo) => {
            println!("nothing left to undo");
            false
        }
        Err(error) => {
            println!("undo failed: {error:?}");
            false
        }
    }
}

/// `discover`: similar-file search plus scored directory aggregation.
fn handle_discover(file: &Path, all: bool, verbose: bool, format: OutputFormat) -> Result<()> {
    let mode = resolve_mode(format);
    let ctx = context::builder::build(file)?;
    let target_name = ctx.target.filename.clone();
    let normalized = normalize::normalize(ctx)?;
    let candidates = discovery::discover(normalized)?;

    if mode.rich {
        discover_view::print_rich(&target_name, &candidates, all, verbose, mode.color);
    } else {
        println!("{}", serde_json::to_string_pretty(&candidates)?);
    }
    Ok(())
}

/// `classify`: full pipeline without any move (execution is a later phase).
async fn handle_classify(
    file: &Path,
    all: bool,
    verbose: bool,
    format: OutputFormat,
) -> Result<()> {
    let jev = build_jev_integration()?;
    let observer = ObservingJevIntegration {
        inner: &jev,
        verbose,
    };

    let report = pipeline::classify_report(file, &observer).await?;
    // Rust Policy verdict: Jev's `move_to` selection is never shown
    // as a move permission; AutoMove/Review/Leave comes from here.
    let policy = kaseifu::policy::decision::evaluate(&report.decision);
    let mode = resolve_mode(format);
    if mode.rich {
        classify_view::print_rich(&report, &policy, all, verbose, mode.color);
    } else {
        println!("{}", serde_json::to_string_pretty(&report.decision)?);
    }
    Ok(())
}

/// Minimal decision observability for the `classify` entry point: notes the
/// `DecisionContext` handed to Jev and the raw response, then delegates.
/// Contains no business logic. Diagnostics go to stderr and only with
/// `--verbose`, so Rich/JSON on stdout is never polluted.
struct ObservingJevIntegration<'a> {
    inner: &'a dyn JevIntegration,
    verbose: bool,
}

impl JevIntegration for ObservingJevIntegration<'_> {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        if self.verbose {
            eprintln!("== Target ==");
            eprintln!("  filename:  {}", context.target().filename);
            eprintln!("  extension: {:?}", context.target().extension);
            eprintln!("  file_type: {:?}", context.target().file_type);

            eprintln!("== Semantic signals ==");
            for signal in context.semantic_signals() {
                eprintln!("  {signal}");
            }

            eprintln!("== Candidates sent to Jev ==");
            for candidate in context.candidates() {
                eprintln!(
                    "  {}: {} ({} related files)",
                    candidate.id, candidate.path, candidate.evidence_count
                );
            }
        }

        let inner = self.inner;
        let context = context.clone();

        Box::pin(async move {
            let response = inner.decide(&context).await;

            match &response {
                Ok(response) => {
                    if self.verbose {
                        eprintln!("== Jev response ==");
                        eprintln!("  selection:     {:?}", response.destination_id);
                        eprintln!("  probabilities: {:?}", response.probabilities);
                    }
                }
                Err(error) => eprintln!("== Jev error ==\n  {error}"),
            }

            response
        })
    }
}

/// Build the real Jev adapter from configuration.
///
/// Values are read from the process environment, falling back to the
/// project-local `.env` (which is git-ignored). Credentials are never
/// printed and never stored anywhere else.
fn build_jev_integration() -> Result<RealJevIntegration> {
    let url = config_value(JEV_API_URL_ENV)
        .or_else(|| config_value("JEV_API_BASE_URL"))
        .context("JEV_API_URL is not set (environment or .env)")?;
    let api_key = config_value(JEV_API_KEY_ENV);

    Ok(RealJevIntegration::new(url, api_key))
}

fn config_value(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| dotenv_value(name))
}

fn dotenv_value(name: &str) -> Option<String> {
    let contents = std::fs::read_to_string(".env").ok()?;

    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        if key.trim() != name {
            return None;
        }
        Some(
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        )
    })
}

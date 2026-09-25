use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use semantic_file_organizer::classifier::jev::{
    JEV_API_KEY_ENV, JEV_API_URL_ENV, JevError, JevIntegration, JevResponse, RealJevIntegration,
};
use semantic_file_organizer::classifier::pipeline;
use semantic_file_organizer::models::decision::DecisionContext;
use semantic_file_organizer::operations::{mover, plan, undo};
use semantic_file_organizer::policy;
use semantic_file_organizer::storage::history::{DEFAULT_DATABASE_PATH, OperationHistory};
use semantic_file_organizer::{context, discovery, normalize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

#[derive(Parser)]
#[command(name = "semantic-file-organizer")]
#[command(version, about = "Semantic file organization for macOS")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build and display FileContext JSON.
    Inspect { path: PathBuf },

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
    Discover { file: PathBuf },

    /// Run the full decision pipeline for one file (Phase 4).
    ///
    /// Observe -> Normalize -> Discover -> Jev -> validated JevDecision.
    /// This command never moves the file (execution is a later phase).
    Classify { file: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { path } => {
            let ctx = context::builder::build(&path)?;
            println!("{}", serde_json::to_string_pretty(&ctx)?);
        }

        Commands::Scan { path } => {
            println!("Scanning: {}", path.display());
        }

        Commands::Organize {
            file,
            execute,
            dry_run,
        } => {
            let actually_move = execute && !dry_run;

            let jev = build_jev_integration()?;
            let history = OperationHistory::open(DEFAULT_DATABASE_PATH)?;

            // Phase 4: decision (Observe -> Normalize -> Discover -> Jev -> validate).
            let (decision, candidates) = pipeline::classify_with_candidates(&file, &jev).await?;
            println!("== Decision ==");
            println!("  destination: {:?}", decision.destination_id);
            println!("  confidence:  {:.3}", decision.confidence);
            println!("  margin:      {:.3}", decision.margin);

            // Phase 5: policy -> plan.
            let action = policy::decision::evaluate(&decision);
            println!("== Policy ==\n  {action:?}");

            match plan::build_move_plan(&file, &decision, &candidates) {
                Ok(plan) => {
                    println!("== Plan ==");
                    println!(
                        "  {} -> {}",
                        plan.source.display(),
                        plan.destination_path.display()
                    );
                    println!("  candidate: {}", plan.candidate_id);

                    if actually_move {
                        match mover::execute_move(&plan, &history) {
                            Ok(mover::MoveOutcome::Moved { operation_id }) => {
                                println!("== Moved ==\n  operation: {operation_id}");
                            }
                            Ok(mover::MoveOutcome::WouldMove) => {
                                println!("== Would move (no mutation) ==");
                            }
                            Err(error) => {
                                println!("== Move failed ==\n  {error:?}");
                            }
                        }
                    } else {
                        match mover::dry_run(&plan) {
                            Ok(_) => println!(
                                "== Dry run ==\n  would move {} -> {}",
                                plan.source.display(),
                                plan.destination_path.display()
                            ),
                            Err(error) => println!("== Dry run rejected ==\n  {error:?}"),
                        }
                    }
                }
                Err(plan::PlanError::Collision(path)) => {
                    // docs/design/05-execute.md: a collision goes to REVIEW.
                    println!(
                        "== REVIEW ==\n  destination already exists: {}",
                        path.display()
                    );
                }
                Err(error) => {
                    println!("== No move ==\n  {error:?}");
                }
            }
        }

        Commands::Undo { last } => {
            let history = OperationHistory::open(DEFAULT_DATABASE_PATH)?;

            for _ in 0..last {
                match undo::undo_last_move(&history) {
                    Ok(outcome) => println!(
                        "restored: {}\n  (undid {})",
                        outcome.restored_to.display(),
                        outcome.undone_move_id
                    ),
                    Err(undo::UndoError::NothingToUndo) => {
                        println!("nothing left to undo");
                        break;
                    }
                    Err(error) => {
                        println!("undo failed: {error:?}");
                        break;
                    }
                }
            }
        }

        Commands::Discover { file } => {
            let ctx = context::builder::build(&file)?;
            let normalized = normalize::normalize(ctx)?;
            let candidates = discovery::discover(normalized)?;

            // Output as pretty JSON
            println!("{}", serde_json::to_string_pretty(&candidates)?);
        }

        Commands::Classify { file } => {
            let jev = build_jev_integration()?;
            let observer = ObservingJevIntegration { inner: &jev };

            let decision = pipeline::classify(&file, &observer).await?;

            println!();
            println!("== JevDecision ==");
            println!("{}", serde_json::to_string_pretty(&decision)?);
        }
    }

    Ok(())
}

/// Minimal decision observability for the `classify` entry point: prints the
/// `DecisionContext` handed to Jev and the raw response, then delegates.
/// Contains no business logic.
struct ObservingJevIntegration<'a> {
    inner: &'a dyn JevIntegration,
}

impl JevIntegration for ObservingJevIntegration<'_> {
    fn decide(
        &self,
        context: &DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Result<JevResponse, JevError>> + Send + '_>> {
        println!("== Target ==");
        println!("  filename:  {}", context.target().filename);
        println!("  extension: {:?}", context.target().extension);
        println!("  file_type: {:?}", context.target().file_type);

        println!("== Semantic signals ==");
        for signal in context.semantic_signals() {
            println!("  {signal}");
        }

        println!("== Candidates sent to Jev ==");
        for candidate in context.candidates() {
            println!(
                "  {}: {} ({} related files)",
                candidate.id, candidate.path, candidate.evidence_count
            );
        }

        let inner = self.inner;
        let context = context.clone();

        Box::pin(async move {
            let response = inner.decide(&context).await;

            match &response {
                Ok(response) => {
                    println!("== Jev response ==");
                    println!("  selection:     {:?}", response.destination_id);
                    println!("  probabilities: {:?}", response.probabilities);
                }
                Err(error) => println!("== Jev error ==\n  {error}"),
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

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use kaseifu::auth::acquire;
use kaseifu::auth::keychain::{AuthError, KeychainStore, SecretStore};
use kaseifu::auth::prompt;
use kaseifu::classifier::jev::{JevError, JevIntegration, JevResponse, RealJevIntegration};
use kaseifu::classifier::pipeline;
use kaseifu::config::loader::validate_endpoint_value;
use kaseifu::config::{
    ApiKeyOrigin, CliOverrides, KeySourceMode, ResolvedConfig, apply_setting, key_source_setting,
    load_user, resolve, resolve_with_env, save_to,
};
use kaseifu::models::decision::DecisionContext;
use kaseifu::operations::organize;
use kaseifu::operations::undo;
use kaseifu::platform::macos::fsevents;
use kaseifu::presentation::renderers::{
    classify as classify_view, discover as discover_view, generic, inspect as inspect_view,
    organize as organize_view,
};
use kaseifu::presentation::{OutputFormat, resolve_mode};
use kaseifu::storage::history::{OperationHistory, journal_path};
use kaseifu::watch::coordinator::WatchConfig;
use kaseifu::watch::runner;
use kaseifu::{context, discovery, normalize};
use std::future::Future;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[derive(Parser)]
#[command(name = "kaseifu")]
#[command(version, about = "Semantic file organization for macOS")]
struct Cli {
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Auto)]
    format: OutputFormat,
    /// Override the Jev endpoint URL (highest priority).
    #[arg(long, global = true)]
    jev_endpoint: Option<String>,
    /// Override the Jev model alias (highest priority).
    #[arg(long, global = true)]
    jev_model: Option<String>,
    /// Subcommand; omitted means print the guidance only (exit 0, no work).
    #[command(subcommand)]
    command: Option<Commands>,
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

    /// Discover candidate destination directories (debug only).
    ///
    /// Searches for similar files using Spotlight and aggregates
    /// parent directories with scoring.
    ///
    /// Hidden from `--help`: this is a diagnostic view of the discovery
    /// stage, not part of the supported workflow (`classify` runs the same
    /// discovery internally). The command stays functional.
    #[command(hide = true)]
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

    /// First-time setup wizard (settings to TOML; the API key goes to the Keychain unless KASEIFU_KEY_SOURCE=env).
    Setup {
        /// Probe endpoint reachability after saving (uses the network).
        #[arg(long)]
        check_connection: bool,
    },

    /// Manage the Jev API key in the macOS Keychain (not used in KASEIFU_KEY_SOURCE=env).
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },

    /// Show or update user settings (TOML; never secrets).
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Diagnose configuration and environment (ASCII report).
    Doctor {
        /// Probe endpoint reachability (uses the network).
        #[arg(long)]
        check_connection: bool,
    },

    /// Print a shell completion script (`zsh --install` writes the file).
    ///
    /// Without `--install` the script goes to stdout and nothing is written.
    /// With `--install` the zsh script is written to the first writable
    /// completion directory (`--dir` > `KASEIFU_COMPLETION_DIR` > Homebrew /
    /// Prezto / XDG candidates); no `.zshrc` change is needed.
    Completions {
        /// Shell to generate for (`zsh` is the supported, installed shell).
        #[arg(value_enum)]
        shell: clap_complete::Shell,
        /// Write the script into the detected completion directory.
        #[arg(long)]
        install: bool,
        /// Explicit target directory for `--install`.
        #[arg(long, value_name = "PATH")]
        dir: Option<PathBuf>,
    },
}

/// `auth` subcommands (the key itself is never a CLI argument).
#[derive(Subcommand)]
enum AuthCommands {
    /// Read the API key interactively and store it in the Keychain.
    SetKey,
    /// Show whether an API key is stored (never the key itself).
    Status,
    /// Delete the stored API key (asks first unless `--yes`).
    RemoveKey {
        /// Delete without asking for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

/// `config` subcommands.
#[derive(Subcommand)]
enum ConfigCommands {
    /// Show effective settings (secrets are never shown).
    Show,
    /// Set one setting, e.g. `config set jev.model jev-latest`.
    Set {
        /// Setting key (see `config show` for the supported list).
        key: String,
        /// New value.
        value: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let format = cli.format;
    let overrides = cli_overrides(&cli);
    // Best-effort completion upkeep before dispatch: `completions` must stay
    // free of side effects, `doctor` must report the state as it is, and the
    // no-subcommand guidance stays side-effect free as well (ZC-2).
    if !matches!(
        &cli.command,
        None | Some(Commands::Completions { .. }) | Some(Commands::Doctor { .. })
    ) {
        kaseifu::completion::refresh_if_stale(&mut Cli::command());
    }
    match cli.command {
        None => handle_usage(format),
        Some(Commands::Inspect { path, all, verbose }) => {
            handle_inspect(&path, all, verbose, format)
        }
        Some(Commands::Scan { path }) => {
            handle_scan(&path, format);
            Ok(())
        }
        Some(Commands::Organize {
            file,
            execute,
            dry_run,
        }) => handle_organize(&file, execute, dry_run, format, &overrides).await,
        Some(Commands::Watch {
            paths,
            execute,
            dry_run,
        }) => handle_watch(&paths, execute, dry_run, format, &overrides).await,
        Some(Commands::Undo { last }) => handle_undo(last, format),
        Some(Commands::Discover { file, all, verbose }) => {
            handle_discover(&file, all, verbose, format)
        }
        Some(Commands::Classify { file, all, verbose }) => {
            handle_classify(&file, all, verbose, format, &overrides).await
        }
        Some(Commands::Setup { check_connection }) => {
            handle_setup(&overrides, check_connection, format).await
        }
        Some(Commands::Auth { action }) => handle_auth(action, format),
        Some(Commands::Config { action }) => handle_config(action, format),
        Some(Commands::Doctor { check_connection }) => {
            handle_doctor(&overrides, check_connection, format).await
        }
        Some(Commands::Completions {
            shell,
            install,
            dir,
        }) => handle_completions(shell, install, dir, format),
    }
}

/// `kaseifu` with no subcommand: guidance only (ZC-2).
///
/// Exit code 0, no usage error, and no side effects: no journal, no network,
/// no completion refresh, nothing on stdin. The key state is read the same way
/// `doctor` reads it (user TOML with a default fallback, Keychain through the
/// resolver), so a broken configuration still prints guidance.
fn handle_usage(format: OutputFormat) -> Result<()> {
    let (key_line, key_configured) = usage_key_state();
    let lines = vec![
        "kaseifu organizes files into the folders they belong to (macOS).".to_string(),
        String::new(),
        "  kaseifu classify <file>   decide where a file belongs (nothing moves)".to_string(),
        "  kaseifu organize <file>   plan the move; --execute performs it".to_string(),
        "  kaseifu doctor            check whether kaseifu is ready to use".to_string(),
        String::new(),
        "Files are never moved unless you pass --execute.".to_string(),
        format!("API key: {key_line}"),
        "Run kaseifu --help for every command.".to_string(),
    ];
    let json = serde_json::json!({
        "about": "Semantic file organization for macOS",
        "commands": [
            {"name": "classify", "purpose": "decide where a file belongs (nothing moves)"},
            {"name": "organize", "purpose": "plan the move; --execute performs it"},
            {"name": "doctor", "purpose": "check whether kaseifu is ready to use"},
        ],
        "execute_required": true,
        "api_key_configured": key_configured,
        "help": "kaseifu --help",
    })
    .to_string();
    emit_report(format, "KASEIFU", &lines, &json);
    Ok(())
}

/// Key state line for the no-subcommand guidance (never the key itself).
fn usage_key_state() -> (String, bool) {
    let store = keychain_store();
    let file = load_user().unwrap_or_default();
    match kaseifu::config::resolve_with_env(&CliOverrides::default(), &file, &store) {
        Ok(resolved) if resolved.jev.api_key.is_some() => (
            format!("configured ({})", resolved.api_key_origin.label()),
            true,
        ),
        Ok(resolved) => (usage_missing_key_line(resolved.key_source.mode), false),
        Err(_) => (
            "could not be checked; run kaseifu doctor".to_string(),
            false,
        ),
    }
}

/// What to do when no key is configured, per source mode.
fn usage_missing_key_line(mode: KeySourceMode) -> String {
    match mode {
        KeySourceMode::Auto => {
            "not configured; run kaseifu setup, or enter it when a Jev command asks".to_string()
        }
        KeySourceMode::Env => {
            "not configured (KASEIFU_KEY_SOURCE=env); run kaseifu setup, set JEV_API_KEY in .env, or enter it when a Jev command asks"
                .to_string()
        }
        KeySourceMode::Keychain => {
            "not configured (KASEIFU_KEY_SOURCE=keychain); run kaseifu setup, or enter it when a Jev command asks"
                .to_string()
        }
    }
}

/// `inspect`: one observation pass, Rich or JSON (read-only).
fn handle_inspect(path: &Path, all: bool, verbose: bool, format: OutputFormat) -> Result<()> {
    let mode = resolve_mode(format);
    let ctx = context::builder::build(path)?;
    if mode.rich {
        let normalized = normalize::normalize_with_options(ctx.clone(), &inspect_image_options())?;
        inspect_view::print_rich(&ctx, &normalized, all, verbose, mode.color);
    } else {
        println!("{}", serde_json::to_string_pretty(&ctx)?);
    }
    Ok(())
}

/// Image extraction toggles for read-only commands.
///
/// Best-effort user TOML (falls back to defaults so `inspect` never fails
/// on configuration); honors `[metadata.image]` without touching secrets.
fn inspect_image_options() -> kaseifu::context::image_metadata::MetadataOptions {
    load_user()
        .ok()
        .map(|file| {
            kaseifu::context::image_metadata::MetadataOptions::from_resolved(
                &kaseifu::config::resolve_with_env(
                    &CliOverrides::default(),
                    &file,
                    &KeychainStore::production(),
                )
                .map(|resolved| resolved.metadata)
                .unwrap_or(fallback_metadata()),
            )
        })
        .unwrap_or_default()
}

/// Resolved metadata defaults when configuration cannot load.
fn fallback_metadata() -> kaseifu::config::ResolvedMetadata {
    kaseifu::config::ResolvedMetadata {
        image_enabled: kaseifu::config::defaults::DEFAULT_METADATA_IMAGE_ENABLED,
        spotlight: kaseifu::config::defaults::DEFAULT_METADATA_SPOTLIGHT,
        exif_fallback: kaseifu::config::defaults::DEFAULT_METADATA_EXIF_FALLBACK,
        send_capture_time_to_jev: kaseifu::config::defaults::DEFAULT_SEND_CAPTURE_TIME_TO_JEV,
        send_gps_to_jev: kaseifu::config::defaults::DEFAULT_SEND_GPS_TO_JEV,
        weather_enabled: kaseifu::config::defaults::DEFAULT_WEATHER_ENABLED,
    }
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
    overrides: &CliOverrides,
) -> Result<()> {
    let actually_move = execute && !dry_run;

    let resolved = resolve_for_jev_command(overrides)?;
    validate_endpoint_value(&resolved.jev.endpoint)?;
    let jev = RealJevIntegration::from_resolved_full(&resolved);
    let history = OperationHistory::open(journal_path())?;

    let report = organize::organize_file_with_full(
        file,
        &jev,
        &history,
        actually_move,
        policy_thresholds(&resolved),
        &kaseifu::context::image_metadata::MetadataOptions::from_resolved(&resolved.metadata),
    )
    .await?;

    let mode = resolve_mode(format);
    if mode.rich {
        organize_view::print_rich(&report, file, actually_move, mode.color);
    } else {
        print!(
            "{}",
            organize_view::render_text(&report, file, actually_move)
        );
    }
    Ok(())
}

/// `watch`: validate roots, then run the debounced FSEvents loop.
async fn handle_watch(
    paths: &[PathBuf],
    execute: bool,
    dry_run: bool,
    format: OutputFormat,
    overrides: &CliOverrides,
) -> Result<()> {
    let actually_move = execute && !dry_run;

    for path in paths {
        if !path.is_dir() {
            anyhow::bail!("watch path is not a directory: {}", path.display());
        }
    }

    let resolved = resolve_for_jev_command(overrides)?;
    validate_endpoint_value(&resolved.jev.endpoint)?;
    let jev = RealJevIntegration::from_resolved_full(&resolved);
    let history = OperationHistory::open(journal_path())?;

    let (sender, receiver) = std::sync::mpsc::channel();
    let _watcher = fsevents::spawn(paths, sender)?;

    print_watch_start(paths, actually_move, format);
    runner::run_loop_with(
        receiver,
        &jev,
        &history,
        actually_move,
        WatchConfig::default(),
        policy_thresholds(&resolved),
        &kaseifu::context::image_metadata::MetadataOptions::from_resolved(&resolved.metadata),
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
    let history = OperationHistory::open(journal_path())?;
    print_pending_warning(&history);

    for _ in 0..last {
        if !apply_single_undo(&history) {
            break;
        }
    }
    Ok(())
}

/// Report `pending` rows before undoing, without touching them.
///
/// A pending row means a mutation was recorded but its result was never
/// written: the process was interrupted, or another process is performing the
/// operation right now. Neither case is auto-recoverable from the journal
/// alone — rewriting the row (or moving the file back) could destroy a state
/// that is actually correct — so this only makes the situation visible and
/// leaves the decision to the user (`pending` stays out of Undo's selection,
/// which requires `result='success'`).
fn print_pending_warning(history: &OperationHistory) {
    let Ok(entries) = history.pending_entries() else {
        return;
    };
    if entries.is_empty() {
        return;
    }
    let moves = entries
        .iter()
        .filter(|entry| entry.operation == "move")
        .count();
    let undos = entries
        .iter()
        .filter(|entry| entry.operation == "undo")
        .count();
    let noun = if entries.len() == 1 { "row" } else { "rows" };
    println!(
        "warning: {} pending journal {} ({} move, {} undo); interrupted moves may already be at the destination",
        entries.len(),
        noun,
        moves,
        undos
    );
    println!("  (not recovered automatically; run `kaseifu doctor` to inspect)");
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
    let normalized = normalize::normalize_with_options(ctx, &inspect_image_options())?;
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
    overrides: &CliOverrides,
) -> Result<()> {
    let resolved = resolve_for_jev_command(overrides)?;
    validate_endpoint_value(&resolved.jev.endpoint)?;
    let jev = RealJevIntegration::from_resolved_full(&resolved);
    let observer = ObservingJevIntegration {
        inner: &jev,
        verbose,
    };

    let report = pipeline::classify_report_with_options(
        file,
        &observer,
        &kaseifu::context::image_metadata::MetadataOptions::from_resolved(&resolved.metadata),
    )
    .await?;
    // Rust Policy verdict: Jev's `move_to` selection is never shown
    // as a move permission; AutoMove/Review/Leave comes from here.
    let policy =
        kaseifu::policy::decision::evaluate_with(&report.decision, policy_thresholds(&resolved));
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
            // Privacy: coordinates never reach stderr; only a masked summary.
            if let Some(meta) = context.image_metadata() {
                let capture = if meta.has_capture_time() {
                    "available"
                } else {
                    "not available"
                };
                eprintln!("  capture:   {capture}");
                eprintln!(
                    "  location:  {}",
                    kaseifu::context::image_metadata::masked_location(meta)
                );
            }

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

/// CLI overrides carried by the global flags (never secrets).
fn cli_overrides(cli: &Cli) -> CliOverrides {
    CliOverrides {
        endpoint: cli.jev_endpoint.clone(),
        model: cli.jev_model.clone(),
    }
}

/// Resolve settings for this invocation.
///
/// Priority: CLI overrides > environment (including the project-local `.env`
/// for developers) > user TOML plus Keychain > application defaults.
/// The API key source itself is selected by `KASEIFU_KEY_SOURCE`
/// (`auto`/`env`/`keychain`; debug builds default to `env`, releases to
/// `auto`). Credentials are never printed and never stored anywhere else.
fn resolve_for(overrides: &CliOverrides) -> Result<ResolvedConfig> {
    let store = KeychainStore::production();
    resolve(overrides, &store)
}

/// Resolve settings for a Jev-calling command, acquiring a missing key
/// interactively when a terminal is available (ZC-1).
///
/// With a TTY the key is read through the existing masked prompt and stored
/// where the selected `KASEIFU_KEY_SOURCE` mode keeps keys (the macOS Keychain
/// for `auto`/`keychain`, the project-local `.env` for `env`), then resolved
/// again from that location so the process never uses a second, unverified
/// copy. Without a TTY the command fails before doing any work (non-zero exit)
/// and names the commands that need no key (`inspect`, `doctor`) plus `setup`.
/// An empty entry, or an interrupted read, stores nothing.
fn resolve_for_jev_command(overrides: &CliOverrides) -> Result<ResolvedConfig> {
    let resolved = resolve_for(overrides)?;
    if resolved.jev.api_key.is_some() {
        return Ok(resolved);
    }
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("{}", acquire::non_tty_guidance(resolved.key_source.mode));
    }
    let target = acquire::prompt_and_store_key(
        resolved.key_source.mode,
        &keychain_store(),
        Path::new(acquire::DOTENV_FILE),
        &|| prompt::prompt_secret("Jev API key"),
    )?;
    let refreshed = resolve_for(overrides)?;
    if refreshed.jev.api_key.is_none() {
        // Rare: the write succeeded but resolution still finds nothing (an
        // empty JEV_API_KEY in the environment or .env masks a stored key, or
        // the Keychain denied the read-back). Say what to check next instead
        // of leaving a half-explained failure.
        anyhow::bail!(
            "the API key was saved to {} but did not resolve; check the source with kaseifu doctor \
             (an empty JEV_API_KEY in the environment or .env can mask a stored key)",
            target.label()
        );
    }
    // stderr keeps stdout (including `--format json`) free of extra lines.
    eprintln!("API key saved to {}", target.label());
    Ok(refreshed)
}

/// Policy thresholds from resolved configuration (user TOML may tune them).
fn policy_thresholds(resolved: &ResolvedConfig) -> kaseifu::policy::decision::Thresholds {
    kaseifu::policy::decision::Thresholds {
        auto_move_confidence: resolved.auto_move_confidence,
        auto_move_margin: resolved.auto_move_margin,
        review_confidence: resolved.review_confidence,
    }
}

// ---------------------------------------------------------------------------
// Setup / auth / config / doctor (v3.3 user-facing configuration)
// ---------------------------------------------------------------------------

/// Emit a command report: JSON for `--format json`, an ASCII panel for Rich,
/// plain `label: value` lines otherwise. Secrets must never be in `lines`.
fn emit_report(format: OutputFormat, title: &str, lines: &[String], json: &str) {
    if format == OutputFormat::Json {
        println!("{json}");
        return;
    }
    let mode = resolve_mode(format);
    if mode.rich {
        print!(
            "{}",
            kaseifu::presentation::components::panel::frame(title, lines, mode.color)
        );
    } else {
        for line in lines {
            println!("{line}");
        }
    }
}

/// Status word for a boolean check (ASCII only).
fn check_word(ok: bool) -> &'static str {
    if ok { "ok" } else { "FAIL" }
}

// --- setup ---------------------------------------------------------------

/// Answers collected by the setup wizard (the key is only a new value).
struct SetupInputs {
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

/// `setup`: interactive first-time configuration.
///
/// `env` mode (the debug default) never touches the Keychain: the key step is
/// replaced by guidance and the TOML settings are still saved.
async fn handle_setup(
    overrides: &CliOverrides,
    check_connection: bool,
    format: OutputFormat,
) -> Result<()> {
    let store = KeychainStore::production();
    let file = load_user().unwrap_or_default();
    let current = resolve_with_env(overrides, &file, &store)?;
    // The Keychain is not even read when the mode forbids it.
    let keychain_has_key = current.key_source.mode.keychain_allowed() && store.get().is_ok();

    emit_setup_intro(&current, keychain_has_key, format);
    let inputs = prompt_setup_inputs(&current, keychain_has_key)?;
    apply_setup_inputs(&inputs, &store)?;

    let verified = resolve_for(overrides)?;
    let report = kaseifu::doctor::diagnose(&verified, check_connection).await;
    emit_setup_summary(&verified, &report, format);
    Ok(())
}

fn emit_setup_intro(current: &ResolvedConfig, keychain_has_key: bool, format: OutputFormat) {
    let key_state = if current.api_key_from_env {
        "provided via environment or .env"
    } else if keychain_has_key {
        "stored"
    } else {
        "not configured"
    };
    let mut lines = vec![
        "Welcome to kaseifu".to_string(),
        format!("Jev API endpoint: {}", current.jev.endpoint),
        format!("Model: {}", current.jev.model),
        format!("API key: {key_state}"),
        format!("Key source: {}", current.key_source.label()),
    ];
    if !current.key_source.mode.keychain_allowed() {
        lines.extend(env_mode_keychain_lines());
    }
    emit_report(
        format,
        "KASEIFU / SETUP",
        &lines,
        &setup_json(current, keychain_has_key),
    );
}

fn setup_json(current: &ResolvedConfig, keychain_has_key: bool) -> String {
    let configured = current.jev.api_key.is_some();
    serde_json::json!({
        "endpoint": current.jev.endpoint,
        "model": current.jev.model,
        "api_key_configured": configured,
        "api_key_in_keychain": keychain_has_key,
        "api_key_source": current.api_key_origin.label(),
        "key_source_mode": current.key_source.mode.label(),
        "key_source_default": !current.key_source.explicit,
    })
    .to_string()
}

fn prompt_setup_inputs(current: &ResolvedConfig, keychain_has_key: bool) -> Result<SetupInputs> {
    let endpoint = prompt_endpoint(&current.jev.endpoint)?;
    let model = prompt_model(&current.jev.model)?;
    // env mode: no key prompt and no Keychain write (guidance is in the intro).
    let api_key = if current.key_source.mode.keychain_allowed() {
        prompt_setup_key(keychain_has_key)?
    } else {
        None
    };
    Ok(SetupInputs {
        endpoint,
        model,
        api_key,
    })
}

fn prompt_endpoint(current: &str) -> Result<String> {
    let input = prompt::prompt_line("Jev API endpoint", Some(current))?;
    let endpoint = or_current(input, current);
    validate_endpoint_value(&endpoint)?;
    Ok(endpoint)
}

fn prompt_model(current: &str) -> Result<String> {
    let input = prompt::prompt_line("Model", Some(current))?;
    let model = or_current(input, current);
    if model.trim().is_empty() {
        anyhow::bail!("invalid model (must not be empty)");
    }
    Ok(model)
}

fn or_current(input: String, current: &str) -> String {
    if input.trim().is_empty() {
        current.to_string()
    } else {
        input
    }
}

fn prompt_setup_key(keychain_has_key: bool) -> Result<Option<String>> {
    if keychain_has_key && !prompt::confirm("Replace the stored API key?", false)? {
        return Ok(None);
    }
    let secret = prompt::prompt_secret("Jev API key")?;
    if secret.trim().is_empty() {
        anyhow::bail!("API key must not be empty");
    }
    Ok(Some(secret))
}

fn apply_setup_inputs(inputs: &SetupInputs, store: &dyn SecretStore) -> Result<()> {
    let mut file = load_user().unwrap_or_default();
    file.jev.endpoint = Some(inputs.endpoint.clone());
    file.jev.model = Some(inputs.model.clone());
    save_to(&kaseifu::config::user_config_path(), &file)?;
    if let Some(secret) = &inputs.api_key {
        store
            .set(secret)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}

fn emit_setup_summary(
    verified: &ResolvedConfig,
    report: &kaseifu::doctor::DoctorReport,
    format: OutputFormat,
) {
    let key_state = status_word(verified.jev.api_key.is_some());
    let mut lines = vec![
        setup_key_summary_line(verified),
        format!("{} Configuration saved", check_word(true)),
        format!("Endpoint: {}", verified.jev.endpoint),
        format!("Model: {}", verified.jev.model),
        format!("API key: {key_state}"),
        format!("Setup {}", setup_verdict(report)),
    ];
    lines.push(format!(
        "Jev connectivity: {}",
        connectivity_word(&report.connectivity)
    ));
    let json = serde_json::json!({
        "endpoint": verified.jev.endpoint,
        "model": verified.jev.model,
        "api_key_configured": verified.jev.api_key.is_some(),
        "connectivity": connectivity_word(&report.connectivity),
    })
    .to_string();
    emit_report(format, "KASEIFU / SETUP", &lines, &json);
}

fn setup_key_summary_line(verified: &ResolvedConfig) -> String {
    if verified.key_source.mode.keychain_allowed() {
        format!("{} API key saved to macOS Keychain", check_word(true))
    } else {
        format!(
            "{} API key stays in env/.env (KASEIFU_KEY_SOURCE=env; the Keychain was not written)",
            check_word(true)
        )
    }
}

/// Guidance shown when `env` mode forbids the Keychain (development/tests).
fn env_mode_keychain_lines() -> Vec<String> {
    vec![
        "env mode (KASEIFU_KEY_SOURCE=env): the macOS Keychain is not used".to_string(),
        "Development and tests use JEV_API_KEY in `.env` or the environment.".to_string(),
        "Run the release-build kaseifu (or KASEIFU_KEY_SOURCE=auto) to register the key in the Keychain."
            .to_string(),
    ]
}

fn setup_verdict(report: &kaseifu::doctor::DoctorReport) -> &'static str {
    if report.checks.iter().all(|check| check.ok) {
        "complete"
    } else {
        "complete with warnings (run doctor)"
    }
}

fn status_word(configured: bool) -> &'static str {
    if configured {
        "Configured"
    } else {
        "Not configured"
    }
}

fn connectivity_word(connectivity: &kaseifu::doctor::Connectivity) -> String {
    match connectivity {
        kaseifu::doctor::Connectivity::NotTested => "Not tested".to_string(),
        kaseifu::doctor::Connectivity::Reachable => "Reachable".to_string(),
        kaseifu::doctor::Connectivity::Unreachable(detail) => format!("Unreachable ({detail})"),
    }
}

// --- auth ----------------------------------------------------------------

/// `auth`: dispatch to the Keychain subcommands.
fn handle_auth(action: AuthCommands, format: OutputFormat) -> Result<()> {
    match action {
        AuthCommands::SetKey => handle_auth_set_key(format),
        AuthCommands::Status => handle_auth_status(format),
        AuthCommands::RemoveKey { yes } => handle_auth_remove_key(yes, format),
    }
}

fn keychain_store() -> KeychainStore {
    KeychainStore::production()
}

fn handle_auth_set_key(format: OutputFormat) -> Result<()> {
    // Gate first: in env mode not a single Keychain call may happen.
    let setting = key_source_setting()?;
    if !setting.mode.keychain_allowed() {
        let json = serde_json::json!({
            "stored": false,
            "reason": "env-mode",
            "key_source_mode": setting.mode.label(),
        })
        .to_string();
        emit_report(format, "KASEIFU / AUTH", &env_mode_keychain_lines(), &json);
        return Ok(());
    }
    let store = keychain_store();
    if store.get().is_ok() && !prompt::confirm("Replace the stored API key?", false)? {
        let lines = vec!["API key unchanged".to_string()];
        emit_report(
            format,
            "KASEIFU / AUTH",
            &lines,
            r#"{"stored":false,"unchanged":true}"#,
        );
        return Ok(());
    }
    let secret = prompt::prompt_secret("Jev API key")?;
    if secret.trim().is_empty() {
        anyhow::bail!("API key must not be empty");
    }
    store
        .set(&secret)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let lines = vec!["API key saved to macOS Keychain".to_string()];
    emit_report(format, "KASEIFU / AUTH", &lines, r#"{"stored":true}"#);
    Ok(())
}

fn handle_auth_status(format: OutputFormat) -> Result<()> {
    let store = keychain_store();
    let file = load_user().unwrap_or_default();
    let cli = CliOverrides::default();
    let resolved = kaseifu::config::resolve_with_env(&cli, &file, &store)?;
    let lines = vec![
        "Jev Authentication".to_string(),
        "Provider: Jev".to_string(),
        format!("API Key: {}", key_state(&resolved)),
        format!("Source: {}", key_source_line(&resolved)),
        format!("KASEIFU_KEY_SOURCE: {}", resolved.key_source.label()),
        format!("Endpoint: {}", resolved.jev.endpoint),
    ];
    let json = serde_json::json!({
        "provider": "jev",
        "api_key_configured": resolved.jev.api_key.is_some(),
        "storage": key_storage(&resolved),
        "api_key_source": resolved.api_key_origin.label(),
        "key_source_mode": resolved.key_source.mode.label(),
        "key_source_default": !resolved.key_source.explicit,
        "endpoint": resolved.jev.endpoint,
    })
    .to_string();
    emit_report(format, "KASEIFU / AUTH", &lines, &json);
    Ok(())
}

/// Key presence for display (never the key itself).
///
/// The origin comes from resolution, so no display path ever re-reads the
/// Keychain (this is what keeps `env` mode free of Keychain calls).
fn key_state(resolved: &ResolvedConfig) -> &'static str {
    if resolved.jev.api_key.is_some() {
        "Configured"
    } else if matches!(resolved.api_key_origin, ApiKeyOrigin::Unavailable(_)) {
        "Unknown"
    } else {
        "Not configured"
    }
}

/// JSON `storage` value (kept compatible with earlier releases).
fn key_storage(resolved: &ResolvedConfig) -> String {
    match &resolved.api_key_origin {
        ApiKeyOrigin::Environment | ApiKeyOrigin::Dotenv => "Environment".to_string(),
        ApiKeyOrigin::Keychain => "macOS Keychain".to_string(),
        ApiKeyOrigin::Missing => "-".to_string(),
        ApiKeyOrigin::Unavailable(message) => message.clone(),
    }
}

/// `Source:` line: the origin that resolved the key (`env` / `.env` / ...).
fn key_source_line(resolved: &ResolvedConfig) -> String {
    match &resolved.api_key_origin {
        ApiKeyOrigin::Unavailable(message) => format!("unavailable ({message})"),
        origin => origin.label().to_string(),
    }
}

fn handle_auth_remove_key(yes: bool, format: OutputFormat) -> Result<()> {
    // Gate first: in env mode not a single Keychain call may happen.
    let setting = key_source_setting()?;
    if !setting.mode.keychain_allowed() {
        let lines = vec![
            "env mode (KASEIFU_KEY_SOURCE=env): the macOS Keychain is not touched".to_string(),
            "Nothing was removed. Run the release-build kaseifu (or KASEIFU_KEY_SOURCE=auto) to manage the Keychain entry."
                .to_string(),
        ];
        let json = serde_json::json!({
            "removed": false,
            "reason": "env-mode",
            "key_source_mode": setting.mode.label(),
        })
        .to_string();
        emit_report(format, "KASEIFU / AUTH", &lines, &json);
        return Ok(());
    }
    let store = keychain_store();
    if matches!(store.get(), Err(AuthError::NotFound)) {
        let lines = vec!["No API key is stored".to_string()];
        emit_report(
            format,
            "KASEIFU / AUTH",
            &lines,
            r#"{"removed":false,"reason":"not-found"}"#,
        );
        return Ok(());
    }
    if !yes && !prompt::confirm("Delete the stored API key?", false)? {
        let lines = vec!["API key unchanged".to_string()];
        emit_report(
            format,
            "KASEIFU / AUTH",
            &lines,
            r#"{"removed":false,"reason":"cancelled"}"#,
        );
        return Ok(());
    }
    match store.delete() {
        Ok(()) => {
            let lines = vec!["API key deleted from macOS Keychain".to_string()];
            emit_report(format, "KASEIFU / AUTH", &lines, r#"{"removed":true}"#);
        }
        Err(AuthError::NotFound) => {
            let lines = vec!["No API key is stored".to_string()];
            emit_report(
                format,
                "KASEIFU / AUTH",
                &lines,
                r#"{"removed":false,"reason":"not-found"}"#,
            );
        }
        Err(error) => return Err(anyhow::anyhow!(error.to_string())),
    }
    Ok(())
}

// --- config --------------------------------------------------------------

/// `config`: dispatch to the TOML subcommands.
fn handle_config(action: ConfigCommands, format: OutputFormat) -> Result<()> {
    match action {
        ConfigCommands::Show => handle_config_show(format),
        ConfigCommands::Set { key, value } => handle_config_set(&key, &value, format),
    }
}

fn handle_config_show(format: OutputFormat) -> Result<()> {
    let store = keychain_store();
    let file = load_user().unwrap_or_default();
    let cli = CliOverrides::default();
    let resolved = kaseifu::config::resolve_with_env(&cli, &file, &store)?;
    let state = key_state(&resolved);
    let lines = vec![
        "[jev]".to_string(),
        format!("endpoint = {}", resolved.jev.endpoint),
        format!("model = {}", resolved.jev.model),
        "[classifier.thresholds]".to_string(),
        format!("auto_move_confidence = {}", resolved.auto_move_confidence),
        format!("auto_move_margin = {}", resolved.auto_move_margin),
        format!("review_confidence = {}", resolved.review_confidence),
        "[operations]".to_string(),
        format!(
            "dry_run = {} (inactive; moves are controlled by --execute)",
            resolved.dry_run
        ),
        format!(
            "enable_undo = {} (inactive; every executed move is journaled)",
            resolved.enable_undo
        ),
        "[metadata.image]".to_string(),
        format!("enabled = {}", resolved.metadata.image_enabled),
        format!("spotlight = {}", resolved.metadata.spotlight),
        format!("exif_fallback = {}", resolved.metadata.exif_fallback),
        "[metadata.privacy]".to_string(),
        format!(
            "send_capture_time_to_jev = {}",
            resolved.metadata.send_capture_time_to_jev
        ),
        format!("send_gps_to_jev = {}", resolved.metadata.send_gps_to_jev),
        format!("API key: {state}"),
    ];
    let json = serde_json::json!({
        "jev": {"endpoint": resolved.jev.endpoint, "model": resolved.jev.model},
        "classifier": {"thresholds": {
            "auto_move_confidence": resolved.auto_move_confidence,
            "auto_move_margin": resolved.auto_move_margin,
            "review_confidence": resolved.review_confidence,
        }},
        "operations": {"dry_run": resolved.dry_run, "enable_undo": resolved.enable_undo},
        "inactive_settings": [
            "operations.dry_run",
            "operations.enable_undo",
        ],
        "metadata": {
            "image": {
                "enabled": resolved.metadata.image_enabled,
                "spotlight": resolved.metadata.spotlight,
                "exif_fallback": resolved.metadata.exif_fallback,
            },
            "privacy": {
                "send_capture_time_to_jev": resolved.metadata.send_capture_time_to_jev,
                "send_gps_to_jev": resolved.metadata.send_gps_to_jev,
            },
            "weather": {"enabled": resolved.metadata.weather_enabled},
        },
        "api_key_configured": resolved.jev.api_key.is_some(),
    })
    .to_string();
    emit_report(format, "KASEIFU / CONFIG", &lines, &json);
    Ok(())
}

fn handle_config_set(key: &str, value: &str, format: OutputFormat) -> Result<()> {
    let mut file = load_user().unwrap_or_default();
    apply_setting(&mut file, key, value)?;
    save_to(&kaseifu::config::user_config_path(), &file)?;
    let lines = vec![format!("Updated {key}")];
    let json = serde_json::json!({"updated": key}).to_string();
    emit_report(format, "KASEIFU / CONFIG", &lines, &json);
    Ok(())
}

// --- doctor --------------------------------------------------------------

/// `doctor`: local diagnostics plus optional connectivity probing.
async fn handle_doctor(
    overrides: &CliOverrides,
    check_connection: bool,
    format: OutputFormat,
) -> Result<()> {
    let store = keychain_store();
    let file = load_user().unwrap_or_default();
    let resolved = resolve_with_env(overrides, &file, &store)?;
    let report = kaseifu::doctor::diagnose(&resolved, check_connection).await;
    emit_doctor_report(&report, format);
    Ok(())
}

/// `doctor` report: one page answering "can I use kaseifu right now?".
///
/// The verdict comes first, each FAIL row carries the next action (ZC-4), and
/// the JSON adds `ready` / `failing` / `checks[].next` without touching the
/// existing keys.
fn emit_doctor_report(report: &kaseifu::doctor::DoctorReport, format: OutputFormat) {
    let failing: Vec<&str> = report
        .checks
        .iter()
        .filter(|check| !check.ok)
        .map(|check| check.name)
        .collect();
    let ready = failing.is_empty();
    let mut lines: Vec<String> = vec![doctor_verdict_line(&failing)];
    lines.extend(report.checks.iter().map(check_line));
    lines.push(format!(
        "Jev connectivity: {}",
        connectivity_word(&report.connectivity)
    ));
    let json = serde_json::json!({
        "ready": ready,
        "failing": failing,
        "checks": report.checks.iter().map(|check| serde_json::json!({
            "name": check.name,
            "ok": check.ok,
            "detail": check.detail,
            "next": check.next,
        })).collect::<Vec<_>>(),
        "connectivity": connectivity_word(&report.connectivity),
        "endpoint": report.endpoint,
    })
    .to_string();
    emit_report(format, "KASEIFU / DOCTOR", &lines, &json);
}

/// First line of the report: whether every check passed.
fn doctor_verdict_line(failing: &[&str]) -> String {
    if failing.is_empty() {
        "Ready: yes".to_string()
    } else {
        format!(
            "Ready: no ({} check(s) failed: {})",
            failing.len(),
            failing.join(", ")
        )
    }
}

/// One check row; a FAIL row always carries its next action.
fn check_line(check: &kaseifu::doctor::CheckResult) -> String {
    match (&check.next, check.ok) {
        (Some(next), false) => format!(
            "{} {}: {} (next: {})",
            check_word(check.ok),
            check.name,
            check.detail,
            next
        ),
        _ => format!("{} {}: {}", check_word(check.ok), check.name, check.detail),
    }
}

// --- completions -----------------------------------------------------------

/// `completions`: print a script, or install the zsh one.
///
/// Printing is pure: the script is the only thing on stdout and `--format`
/// does not apply. `--install` reports through the usual `emit_report` shapes.
fn handle_completions(
    shell: clap_complete::Shell,
    install: bool,
    dir: Option<PathBuf>,
    format: OutputFormat,
) -> Result<()> {
    let mut cmd = Cli::command();
    if !install {
        let script = kaseifu::completion::render(shell, &mut cmd);
        let mut stdout = std::io::stdout();
        stdout.write_all(&script)?;
        stdout.flush()?;
        return Ok(());
    }
    if shell != clap_complete::Shell::Zsh {
        anyhow::bail!("--install supports zsh only (requested: {shell})");
    }
    let target = kaseifu::completion::install_dir(dir.as_deref())?;
    let outcome = kaseifu::completion::install(&target, &mut cmd)?;
    let path = target.join(kaseifu::completion::FILE_NAME);
    let (line, status) = match outcome {
        kaseifu::completion::WriteOutcome::Written => (
            format!("Installed zsh completions: {}", path.display()),
            "installed",
        ),
        kaseifu::completion::WriteOutcome::Unchanged => (
            format!("zsh completions already up to date: {}", path.display()),
            "unchanged",
        ),
    };
    let json = serde_json::json!({
        "shell": "zsh",
        "path": path.display().to_string(),
        "status": status,
    })
    .to_string();
    emit_report(format, "KASEIFU / COMPLETIONS", &[line], &json);
    Ok(())
}

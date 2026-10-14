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
use kaseifu::operations::{mover, organize, plan, select as select_ops, undo};
use kaseifu::platform::macos::fsevents;
use kaseifu::presentation::renderers::{
    classify as classify_view, discover as discover_view, generic, inspect as inspect_view,
    organize as organize_view, select as select_view, uninstall as uninstall_view,
};
use kaseifu::presentation::{OutputFormat, resolve_mode};
use kaseifu::rename::{
    batch::{Applied, BatchItem, BatchOutcome, BatchReport},
    document::classify_document,
    rules::{PRESETS, find_preset, is_date_format},
    service::{
        DirectoryReport, PerceptionReport, PlanOutcome, RenameOptions, VideoReport, plan_rename,
    },
    store::RenameStore,
};
use kaseifu::semantic::Availability;
use kaseifu::semantic::provider::{FoundationModelsProvider, SemanticProvider};
use kaseifu::storage::history::{OperationHistory, journal_path};
use kaseifu::uninstall;
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
#[command(after_help = "\
Policies decide what kaseifu does with a classified file:
  AUTO MOVE  confidence >= 0.85 and margin >= 0.40 (built-in defaults;
             `kaseifu config show` prints the effective thresholds)
  REVIEW     confidence >= 0.60; a person should decide before it moves
  LEAVE      anything else; the file stays where it is

Naming a policy is not a command: they are the three verdicts of the local
policy check. Files are never moved unless you pass --execute.

Scope: image files are classified today (jpg, jpeg, png, heic, heif, hif,
tif, tiff, webp). Video (mp4, mov) is not classified yet and its capture
metadata is not sent to Jev.")]
struct Cli {
    /// Output format for commands with structured output.
    ///
    /// `json` applies to inspect, classify, organize, doctor, setup, auth,
    /// config, completions --install and uninstall. `scan`, `undo` and
    /// `watch` always render text.
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
    /// Show what kaseifu observes in a file (read-only).
    ///
    /// Prints the observation report behind classification: paths, directory
    /// entries, metadata and Spotlight evidence, as JSON when stdout is not a
    /// terminal. Nothing is moved.
    Inspect {
        path: PathBuf,
        /// Show all directory entries and evidence (default: up to 5).
        #[arg(long)]
        all: bool,
        /// Include full paths, the report schema and raw metadata.
        #[arg(long)]
        verbose: bool,
    },

    /// Print the target banner for a directory (scanning is not implemented yet).
    ///
    /// A placeholder for a future scan command: it prints the directory it
    /// was given and exits. Nothing is read, moved or written.
    Scan { path: PathBuf },

    /// Decide where a file belongs and show the result (moves only with --execute).
    ///
    /// Builds a move plan from the candidate folders and the classifier
    /// answer, then either reports what would happen (the default) or
    /// performs the move with --execute. Every executed move is journaled and
    /// can be reversed with `undo`.
    Organize {
        file: PathBuf,
        /// Move the file (default is a dry run; cannot be combined with --dry-run).
        #[arg(long, conflicts_with = "dry_run")]
        execute: bool,
        /// Request a dry run explicitly (the default; cannot be combined with --execute).
        #[arg(long)]
        dry_run: bool,
        /// Pick the destination yourself from the candidate folders (needs a terminal).
        #[arg(long, conflicts_with = "explain")]
        select: bool,
        /// Skip the confirmation prompt when `--select --execute` moves a file.
        #[arg(long, requires = "select")]
        yes: bool,
        /// Show the confidence, margin and effective thresholds behind the verdict.
        #[arg(long)]
        explain: bool,
    },

    /// Restore files moved by earlier runs (newest first).
    ///
    /// Reverses the last successful move; `--last N` reverses up to N moves.
    /// The original location must be free: a collision stops the undo
    /// instead of overwriting anything.
    Undo {
        #[arg(long, default_value_t = 1)]
        last: usize,
    },

    /// List candidate destination directories for one file (diagnostic view).
    ///
    /// Searches for similar files using Spotlight and ranks their parent
    /// directories. This is a diagnostic view of one internal step, not part
    /// of the normal workflow: `classify` runs the same search internally.
    /// Hidden from `--help`.
    #[command(hide = true)]
    Discover {
        file: PathBuf,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        verbose: bool,
    },

    /// Decide where a file belongs without moving anything.
    ///
    /// Runs the full search-and-classify flow for one file and prints the
    /// decision together with what kaseifu would do about it. This command
    /// never moves the file and never writes to the journal; `--explain`
    /// shows the numbers behind the verdict.
    Classify {
        file: PathBuf,
        /// Show all ranked candidates (default: top 5).
        #[arg(long)]
        all: bool,
        /// Show extra detail (full paths, evidence breakdowns).
        #[arg(long)]
        verbose: bool,
        /// Show the confidence, margin and effective thresholds behind the verdict.
        #[arg(long)]
        explain: bool,
    },

    /// Watch directories and organize changed files automatically.
    ///
    /// Events are filtered, debounced, deduplicated and stability-checked
    /// before the same flow as `organize` runs. Dry run by default; pass
    /// `--execute` to actually move files.
    Watch {
        /// Directory to watch (repeatable; there is no default).
        #[arg(long = "path", required = true)]
        paths: Vec<PathBuf>,
        /// Move files (default is a dry run; cannot be combined with --dry-run).
        #[arg(long, conflicts_with = "dry_run")]
        execute: bool,
        /// Request a dry run explicitly (the default; cannot be combined with --execute).
        #[arg(long)]
        dry_run: bool,
    },

    /// First-time setup: save the endpoint, the model and the API key.
    ///
    /// Interactive (needs a terminal). General settings go to the user TOML
    /// file; the API key goes to the macOS Keychain unless
    /// KASEIFU_KEY_SOURCE=env, which points at `.env` instead.
    Setup {
        /// Probe endpoint reachability after saving (uses the network).
        #[arg(long)]
        check_connection: bool,
    },

    /// Manage the Jev API key in the macOS Keychain (not touched in KASEIFU_KEY_SOURCE=env).
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },

    /// Show or update user settings (TOML; never secrets).
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Diagnose configuration and environment (one-page report).
    ///
    /// Checks platform, configuration, API key, endpoint, Spotlight, storage,
    /// journal, the old CWD-relative journal and zsh completions. Every
    /// failing check names the next command to run. The network is only
    /// reached with --check-connection.
    Doctor {
        /// Probe endpoint reachability (uses the network).
        #[arg(long)]
        check_connection: bool,
    },

    /// Remove what kaseifu installed on this machine (dry run by default).
    ///
    /// Deletes the zsh completion script. `--purge` also deletes the user
    /// configuration and the journal (deleting the journal loses the undo
    /// history), and `--remove-key` (or `--purge`) deletes the API key from
    /// the macOS Keychain after a confirmation (`--yes` skips it). Files
    /// kaseifu moved are never touched and kaseifu itself stays installed:
    /// run `cargo uninstall kaseifu` for the command itself.
    Uninstall {
        /// Delete the listed targets (the default is a dry run).
        #[arg(long)]
        execute: bool,
        /// Also delete the user configuration and the journal (loses the undo history).
        #[arg(long)]
        purge: bool,
        /// Also delete the API key from the macOS Keychain (asks first unless --yes).
        #[arg(long)]
        remove_key: bool,
        /// Skip the Keychain confirmation (used with --remove-key or --purge).
        #[arg(long)]
        yes: bool,
    },

    /// Propose a content-based file or folder name (dry run by default).
    ///
    /// Asks the on-device model for a title, applies the naming rule
    /// (--pattern, --template, --preset or the stored default), then either
    /// reports the proposal or renames with --execute. Every executed rename
    /// is journaled and can be reversed with `undo`. Use --patterns to list
    /// the rules. Documents are txt, md, pdf and html; images are jpg, jpeg,
    /// png, heic, heif, tif, tiff and webp, and their scene is described by
    /// on-device Vision labels. Videos are mp4, mov, m4v, 3gp and 3g2: the
    /// container's metadata comes from AVFoundation and the same on-device
    /// classifier labels a few representative frames. Folders are named from
    /// what is inside them (child names, file types and a few child files),
    /// and a folder kaseifu must not touch (system trees, app bundles, the
    /// Library tree, caches, hidden folders, the working directory) is
    /// refused with the reason. With --recursive the folder is not renamed:
    /// everything inside it is, after the whole list has been shown.
    Rename(Box<RenameCli>),

    /// Print a shell completion script (only zsh can be installed).
    ///
    /// Without --install the script goes to stdout and nothing is written;
    /// any supported shell can be printed that way. --install writes the zsh
    /// script into the first writable completion directory (`--dir` >
    /// KASEIFU_COMPLETION_DIR > Homebrew / Prezto / XDG candidates); no
    /// `.zshrc` change is needed.
    Completions {
        /// Shell to generate for (`zsh` is the supported, installed shell).
        #[arg(value_enum)]
        shell: clap_complete::Shell,
        /// Write the zsh script into the detected completion directory (zsh only).
        #[arg(long)]
        install: bool,
        /// Explicit target directory for --install.
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
    // free of side effects, `doctor` must report the state as it is, the
    // no-subcommand guidance stays side-effect free as well (ZC-2), and
    // `uninstall` must see (and delete) the completion files as they are
    // right now (ZC-11).
    if !matches!(
        &cli.command,
        None | Some(Commands::Completions { .. })
            | Some(Commands::Doctor { .. })
            | Some(Commands::Uninstall { .. })
    ) {
        kaseifu::completion::refresh_if_stale(&mut Cli::command());
    }
    dispatch(cli.command, format, &overrides).await
}

/// Run the requested subcommand (kept separate so `main` stays trivial).
async fn dispatch(
    command: Option<Commands>,
    format: OutputFormat,
    overrides: &CliOverrides,
) -> Result<()> {
    let Some(cmd) = command else {
        return handle_usage(format);
    };
    if is_support_command(&cmd) {
        return dispatch_support(cmd, format, overrides).await;
    }
    match cmd {
        Commands::Inspect { path, all, verbose } => handle_inspect(&path, all, verbose, format),
        Commands::Scan { path } => {
            handle_scan(&path, format);
            Ok(())
        }
        Commands::Organize {
            file,
            execute,
            dry_run,
            select,
            yes,
            explain,
        } => {
            dispatch_organize(
                OrganizeOptions {
                    file,
                    execute,
                    dry_run,
                    select,
                    yes,
                    explain,
                },
                format,
                overrides,
            )
            .await
        }
        Commands::Watch {
            paths,
            execute,
            dry_run,
        } => handle_watch(&paths, execute, dry_run, format, overrides).await,
        Commands::Rename(boxed) => handle_rename(*boxed, format).await,
        Commands::Undo { last } => handle_undo(last, format),
        Commands::Discover { file, all, verbose } => handle_discover(&file, all, verbose, format),
        Commands::Classify {
            file,
            all,
            verbose,
            explain,
        } => handle_classify(&file, all, verbose, explain, format, overrides).await,
        _ => unreachable!("support commands route through dispatch_support"),
    }
}

/// True for the environment and setup commands (handled by `dispatch_support`).
fn is_support_command(cmd: &Commands) -> bool {
    matches!(
        cmd,
        Commands::Setup { .. }
            | Commands::Auth { .. }
            | Commands::Config { .. }
            | Commands::Doctor { .. }
            | Commands::Uninstall { .. }
            | Commands::Completions { .. }
    )
}

/// Setup and environment commands (split out so `dispatch` stays small).
async fn dispatch_support(
    cmd: Commands,
    format: OutputFormat,
    overrides: &CliOverrides,
) -> Result<()> {
    match cmd {
        Commands::Setup { check_connection } => {
            handle_setup(overrides, check_connection, format).await
        }
        Commands::Auth { action } => handle_auth(action, format),
        Commands::Config { action } => handle_config(action, format),
        Commands::Doctor { check_connection } => {
            handle_doctor(overrides, check_connection, format).await
        }
        Commands::Uninstall {
            execute,
            purge,
            remove_key,
            yes,
        } => handle_uninstall(execute, purge, remove_key, yes, format),
        Commands::Completions {
            shell,
            install,
            dir,
        } => handle_completions(shell, install, dir, format),
        _ => unreachable!("only support commands reach dispatch_support"),
    }
}

/// The parsed `organize` flags, kept together so the dispatch helpers stay
/// within a readable argument count.
struct OrganizeOptions {
    file: PathBuf,
    execute: bool,
    dry_run: bool,
    select: bool,
    yes: bool,
    explain: bool,
}

/// `organize` with or without the interactive selection (ZC-13).
async fn dispatch_organize(
    options: OrganizeOptions,
    format: OutputFormat,
    overrides: &CliOverrides,
) -> Result<()> {
    if options.select {
        handle_organize_select(
            &options.file,
            options.execute,
            options.dry_run,
            options.yes,
            format,
            overrides,
        )
        .await
    } else {
        handle_organize(
            &options.file,
            options.execute,
            options.dry_run,
            options.explain,
            format,
            overrides,
        )
        .await
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

/// `organize`: decide, plan, then dry-run or move (journaled).
///
/// `--explain` adds the threshold view (ZC-5); the default output keeps the
/// historical sections.
async fn handle_organize(
    file: &Path,
    execute: bool,
    dry_run: bool,
    explain: bool,
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

    let thresholds = policy_thresholds(&resolved);
    let mode = resolve_mode(format);
    if format == OutputFormat::Json {
        let explanation = explain.then(|| organize_view::explanation_for(&report, thresholds));
        println!(
            "{}",
            serde_json::to_string_pretty(&organize_view::json_report(
                &report,
                file,
                actually_move,
                explanation.as_ref(),
            ))?
        );
    } else if mode.rich {
        organize_view::print_rich(
            &report,
            file,
            actually_move,
            mode.color,
            thresholds,
            explain,
        );
    } else {
        print!(
            "{}",
            organize_view::render_text_with(&report, file, actually_move, thresholds, explain)
        );
    }
    Ok(())
}

/// `organize --select`: pick the destination by hand (ZC-13).
///
/// Discovery only: Jev is never consulted, so this path needs no API key,
/// never reads or writes the Keychain and never opens a network connection.
/// The plan layer re-validates the destination (usable folder, no
/// collision); the move is journaled with NULL candidate/confidence/margin
/// columns so `undo` and `doctor` can tell a human move from a Jev move.
/// `--format json` prints the candidate list instead of prompting.
async fn handle_organize_select(
    file: &Path,
    execute: bool,
    dry_run: bool,
    yes: bool,
    format: OutputFormat,
    overrides: &CliOverrides,
) -> Result<()> {
    let actually_move = execute && !dry_run;
    let mode = resolve_mode(format);

    if format == OutputFormat::Json {
        return print_select_candidates(file, execute, overrides);
    }

    // TTY first: without a terminal the selection must fail before any work
    // (no `mdfind` run, no prompt, nothing read).
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(SELECT_NON_TTY_GUIDANCE);
    }

    let candidates = select_candidates(file, overrides)?;
    if candidates.is_empty() {
        print!("{}", select_view::no_candidates_text(mode.rich, mode.color));
        return Ok(());
    }

    print!(
        "{}",
        select_view::render_menu(file, &candidates, mode.rich, mode.color)
    );
    let Some(chosen) = prompt_select_choice(&candidates)? else {
        println!("{}", select_view::cancelled_line());
        return Ok(());
    };

    run_selected_move(file, &candidates[chosen], actually_move, yes, mode)
}

/// Plan, display and (when requested) perform the chosen manual move.
fn run_selected_move(
    file: &Path,
    candidate: &kaseifu::models::candidate::CandidateDestination,
    actually_move: bool,
    yes: bool,
    mode: kaseifu::presentation::OutputMode,
) -> Result<()> {
    let move_plan = match plan::build_manual_move_plan(file, candidate) {
        Ok(move_plan) => move_plan,
        Err(error) => {
            anyhow::bail!(
                "cannot move this file: {}",
                organize_view::plan_error_message(&error, mode.rich, mode.color)
            );
        }
    };
    print!(
        "{}",
        select_view::render_plan(&move_plan, candidate, actually_move, mode.rich, mode.color)
    );

    if !actually_move {
        return Ok(());
    }
    if !confirm_manual_move(yes)? {
        println!("{}", select_view::cancelled_line());
        return Ok(());
    }
    execute_manual_move_report(&move_plan, mode.rich, mode.color)
}

/// Non-TTY guidance for `organize --select` (ZC-13).
///
/// `discover` is deliberately not named: it stays a hidden diagnostic view.
const SELECT_NON_TTY_GUIDANCE: &str = "interactive selection needs a terminal (stdin is not a TTY)\n\
list the candidate destinations in JSON: kaseifu organize <file> --select --format json\n\
a future release will add --to <dir> for non-interactive selection";

/// `--select --format json`: the candidate list, no interaction, no Jev.
fn print_select_candidates(file: &Path, execute: bool, overrides: &CliOverrides) -> Result<()> {
    if execute {
        anyhow::bail!(
            "--format json lists the candidate destinations and does not select; \
             run without --format json to select and move interactively"
        );
    }
    let candidates = select_candidates(file, overrides)?;
    println!("{}", serde_json::to_string_pretty(&candidates)?);
    Ok(())
}

/// Discovery for the selection flow (observe -> normalize -> discover; no Jev).
fn select_candidates(
    file: &Path,
    overrides: &CliOverrides,
) -> Result<Vec<kaseifu::models::candidate::CandidateDestination>> {
    let resolved = resolve_for(overrides)?;
    let (_, candidates) = pipeline::discover_candidates(
        file,
        &kaseifu::context::image_metadata::MetadataOptions::from_resolved(&resolved.metadata),
    )?;
    Ok(candidates)
}

/// Ask until a valid answer arrives; `None` means the user cancelled.
fn prompt_select_choice(
    candidates: &[kaseifu::models::candidate::CandidateDestination],
) -> Result<Option<usize>> {
    loop {
        let answer = prompt::prompt_line(select_ops::SELECT_PROMPT, None)?;
        match select_ops::parse_select_command(&answer, candidates.len()) {
            Ok(select_ops::SelectCommand::Quit) => return Ok(None),
            Ok(select_ops::SelectCommand::Choose(index)) => return Ok(Some(index)),
            Err(message) => eprintln!("{message}"),
        }
    }
}

/// `--yes` skips the confirmation; the default is to ask and to cancel.
fn confirm_manual_move(yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    prompt::confirm("Move this file?", false)
}

/// Perform the chosen manual move and report it (journal included).
fn execute_manual_move_report(
    plan: &kaseifu::operations::plan::MovePlan,
    rich: bool,
    color: bool,
) -> Result<()> {
    let history = OperationHistory::open(journal_path())?;
    match mover::execute_manual_move(plan, &history) {
        Ok(mover::MoveOutcome::Moved { operation_id }) => {
            print!(
                "{}",
                select_view::render_moved(plan, &operation_id, rich, color)
            );
            Ok(())
        }
        Ok(mover::MoveOutcome::WouldMove) => {
            // execute_manual_move always performs the move; this arm only
            // keeps the outcome match exhaustive.
            anyhow::bail!("the move was not performed")
        }
        Err(error) => anyhow::bail!("move failed: {error:?}"),
    }
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

/// `watch` start banner (the mode is always stated, ZC-9).
fn print_watch_start(paths: &[PathBuf], actually_move: bool, format: OutputFormat) {
    let roots = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mode = resolve_mode(format);
    println!(
        "{}",
        generic::watch_line("start", &roots, !actually_move, mode.color)
    );
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

/// Parsed `rename` arguments (boxed: the struct form keeps `Commands` small).
#[derive(clap::Args, Debug)]
struct RenameCli {
    /// File or folder to rename (a document, an image, a video, or a folder).
    target: Option<PathBuf>,
    /// Rename the file (default is a dry run; cannot be combined with --dry-run).
    #[arg(long, conflicts_with = "dry_run")]
    execute: bool,
    /// Request a dry run explicitly (the default; cannot be combined with --execute).
    #[arg(long)]
    dry_run: bool,
    /// Rename everything inside this folder (bounded), not the folder itself.
    #[arg(long)]
    recursive: bool,
    /// Answer yes to the batch confirmation (used with --recursive --execute).
    #[arg(long, requires = "recursive")]
    yes: bool,
    /// Naming pattern: a P01-P12 id or slug (ai, date-title, ...).
    #[arg(long, value_name = "PATTERN")]
    pattern: Option<String>,
    /// Custom template with {variables} (overrides --pattern).
    #[arg(long, value_name = "TEMPLATE")]
    template: Option<String>,
    /// Stored preset to apply.
    #[arg(long, value_name = "NAME")]
    preset: Option<String>,
    /// Fixed prefix for the {prefix} variable.
    #[arg(long, value_name = "TEXT")]
    prefix: Option<String>,
    /// Fixed suffix for the {suffix} variable.
    #[arg(long, value_name = "TEXT")]
    suffix: Option<String>,
    /// Project for the {project} variable (defaults to the model value).
    #[arg(long, value_name = "TEXT")]
    project: Option<String>,
    /// Sequence number for the {seq} variable.
    #[arg(long, value_name = "N")]
    seq: Option<u32>,
    /// Date format (YYYY-MM-DD, YYYYMMDD, YYYY-MM, YYYY, or Japanese YMD).
    #[arg(long, value_name = "FORMAT")]
    date_format: Option<String>,
    /// List builtin patterns and stored presets.
    #[arg(long)]
    patterns: bool,
    /// List stored presets only.
    #[arg(long)]
    list_presets: bool,
    /// Save --template/--pattern/--preset as the stored preset NAME.
    #[arg(long, value_name = "NAME")]
    save_preset: Option<String>,
    /// Delete the stored preset NAME.
    #[arg(long, value_name = "NAME")]
    delete_preset: Option<String>,
    /// Show the default rule.
    #[arg(long)]
    show_default: bool,
    /// Set the default rule (pattern id, slug, preset name or preset:NAME).
    #[arg(long, value_name = "RULE")]
    set_default: Option<String>,
}

/// `rename`: management flags, or a document dry-run / execution.
async fn handle_rename(args: RenameCli, format: OutputFormat) -> Result<()> {
    let store = RenameStore::open(journal_path())?;
    if has_management(&args) {
        if args.target.is_some() {
            anyhow::bail!("management flags cannot be combined with a target file");
        }
        return rename_management(&store, &args, format);
    }
    let Some(target) = args.target.as_deref() else {
        return rename_guidance(format);
    };
    if args.recursive {
        return rename_batch(&store, target, &args, format).await;
    }
    rename_file(&store, target, &args, format).await
}

/// True when any preset/default management flag is present.
fn has_management(args: &RenameCli) -> bool {
    args.patterns
        || args.list_presets
        || args.save_preset.is_some()
        || args.delete_preset.is_some()
        || args.show_default
        || args.set_default.is_some()
}

/// Run the first matching management action (split out for complexity).
fn rename_management(store: &RenameStore, args: &RenameCli, format: OutputFormat) -> Result<()> {
    if args.patterns || args.list_presets {
        return rename_list(store, args.patterns, format);
    }
    if let Some(name) = args.save_preset.as_deref() {
        return rename_save(store, name, args, format);
    }
    if let Some(name) = args.delete_preset.as_deref() {
        return rename_delete(store, name, format);
    }
    if let Some(rule) = args.set_default.as_deref() {
        return rename_set_default(store, rule, format);
    }
    if args.show_default {
        return rename_show_default(store, format);
    }
    rename_guidance(format)
}

/// `rename` without a target or management flag: print guidance only.
fn rename_guidance(format: OutputFormat) -> Result<()> {
    emit_report(
        format,
        "Rename",
        &[
            "usage: kaseifu rename <file-or-folder> [--pattern PATTERN] [--execute]".to_string(),
            "batch: kaseifu rename <folder> --recursive [--execute]".to_string(),
            "documents: txt, md, pdf, html".to_string(),
            "images: jpg, jpeg, png, heic, heif, tif, tiff, webp".to_string(),
            "videos: mp4, mov, m4v, 3gp, 3g2".to_string(),
            "folders: named from what is inside them, never from their own name".to_string(),
            "run `kaseifu rename --patterns` to list the naming rules".to_string(),
        ],
        &serde_json::json!({"rename": {"status": "guidance"}}).to_string(),
    );
    Ok(())
}

/// `rename --patterns` / `rename --list-presets`.
fn rename_list(store: &RenameStore, with_builtin: bool, format: OutputFormat) -> Result<()> {
    let mut lines = Vec::new();
    if with_builtin {
        for preset in PRESETS {
            lines.push(format!(
                "{} {} {} - {}",
                preset.id, preset.slug, preset.template, preset.description
            ));
        }
    }
    let stored = store
        .list_presets()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    for preset in &stored {
        lines.push(format!(
            "preset:{} {} ({})",
            preset.name, preset.template, preset.date_format
        ));
    }
    if lines.is_empty() {
        lines.push("no stored presets (builtins are hidden by --list-presets)".to_string());
    }
    let json = serde_json::json!({
        "rename": {
            "status": "patterns",
            "builtin": with_builtin,
            "presets": stored.iter().map(|preset| serde_json::json!({
                "name": preset.name,
                "template": preset.template,
                "date_format": preset.date_format,
            })).collect::<Vec<_>>(),
        }
    })
    .to_string();
    emit_report(format, "Rename patterns", &lines, &json);
    Ok(())
}

/// `rename --save-preset NAME` from --template, --pattern or --preset.
fn rename_save(
    store: &RenameStore,
    name: &str,
    args: &RenameCli,
    format: OutputFormat,
) -> Result<()> {
    if args.target.is_some() {
        anyhow::bail!("--save-preset cannot be combined with a target file");
    }
    let (template, date_format) = rename_save_template(store, args)?;
    store
        .save_preset(name, &template, &date_format)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let line = format!("saved preset:{name} {template} ({date_format})");
    let json =
        serde_json::json!({"rename": {"status": "saved", "preset": name, "template": template}})
            .to_string();
    emit_report(format, "Rename preset", &[line], &json);
    Ok(())
}

/// Resolve the template and date format a preset save should store.
fn rename_save_template(store: &RenameStore, args: &RenameCli) -> Result<(String, String)> {
    let date_format = args
        .date_format
        .clone()
        .unwrap_or_else(|| "YYYY-MM-DD".to_string());
    if !is_date_format(&date_format) {
        anyhow::bail!("unknown date format: {date_format}");
    }
    if let Some(template) = args.template.as_deref() {
        return Ok((template.to_string(), date_format));
    }
    if let Some(spec) = args.pattern.as_deref() {
        let preset = find_preset(spec).ok_or_else(|| anyhow::anyhow!("unknown pattern: {spec}"))?;
        if preset.id == "P12" {
            anyhow::bail!("P12 custom needs --template");
        }
        return Ok((preset.template.to_string(), date_format));
    }
    if let Some(name) = args.preset.as_deref() {
        let stored = store
            .get_preset(name)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let Some(preset) = stored else {
            anyhow::bail!("unknown preset: {name}");
        };
        return Ok((preset.template, preset.date_format));
    }
    anyhow::bail!("--save-preset needs --template, --pattern or --preset")
}

/// `rename --delete-preset NAME`.
fn rename_delete(store: &RenameStore, name: &str, format: OutputFormat) -> Result<()> {
    let deleted = store
        .delete_preset(name)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    if !deleted {
        anyhow::bail!("unknown preset: {name}");
    }
    let line = format!("deleted preset:{name}");
    let json = serde_json::json!({"rename": {"status": "deleted", "preset": name}}).to_string();
    emit_report(format, "Rename preset", &[line], &json);
    Ok(())
}

/// `rename --set-default RULE`.
fn rename_set_default(store: &RenameStore, rule: &str, format: OutputFormat) -> Result<()> {
    store
        .set_default(rule)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let current = store
        .get_default()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let line = format!("default rule: {current}");
    let json = serde_json::json!({"rename": {"status": "default", "default": current}}).to_string();
    emit_report(format, "Rename default", &[line], &json);
    Ok(())
}

/// `rename --show-default`.
fn rename_show_default(store: &RenameStore, format: OutputFormat) -> Result<()> {
    let current = store
        .get_default()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let line = format!("default rule: {current}");
    let json = serde_json::json!({"rename": {"status": "default", "default": current}}).to_string();
    emit_report(format, "Rename default", &[line], &json);
    Ok(())
}

/// One `rename` file report (text lines plus the fixed JSON shape).
struct RenameReport<'a> {
    current_name: &'a str,
    proposal: Option<&'a str>,
    pattern: &'a str,
    date_source: &'a str,
    status: &'a str,
    reason: Option<&'a str>,
    operation_id: Option<&'a str>,
    target: &'a Path,
    perception: Option<&'a PerceptionReport>,
    video: Option<&'a VideoReport>,
    directory: Option<&'a DirectoryReport>,
}

/// The `perception` object for the JSON report (images and videos).
fn perception_json(report: &PerceptionReport) -> serde_json::Value {
    match report {
        PerceptionReport::Labels(labels) => serde_json::json!({
            "status": "labels",
            "labels": labels
                .iter()
                .map(|entry| serde_json::json!({
                    "label": entry.label,
                    "confidence": entry.confidence,
                }))
                .collect::<Vec<_>>(),
        }),
        PerceptionReport::Unavailable(reason) => serde_json::json!({
            "status": "unavailable",
            "reason": reason,
        }),
    }
}

/// The `video` object for the JSON report (videos only).
///
/// Only keys the container actually reported are present; the frame count is
/// how many representative frames were classified. Every key is an addition:
/// no image or document report changes shape.
fn video_json(report: &VideoReport) -> serde_json::Value {
    let mut value = serde_json::json!({ "frames": report.frames });
    let info = &report.info;
    if let Some(duration) = info.duration_seconds {
        value["duration_seconds"] = serde_json::json!(duration);
    }
    if let Some(width) = info.width {
        value["width"] = serde_json::json!(width);
    }
    if let Some(height) = info.height {
        value["height"] = serde_json::json!(height);
    }
    if let Some(rate) = info.frame_rate {
        value["frame_rate"] = serde_json::json!(rate);
    }
    if let Some(recorded) = info.creation_date.as_deref() {
        value["creation_date"] = serde_json::json!(recorded);
    }
    value
}

/// The `directory` object for the JSON report (folders only).
///
/// Every key is an addition: no document, image or video report changes shape.
/// The child names are not repeated here (they are in the request, not in the
/// report); the user sees the counts and the file types.
fn directory_json(report: &DirectoryReport) -> serde_json::Value {
    let facts = &report.facts;
    serde_json::json!({
        "total_entries": facts.total_entries,
        "file_count": facts.file_count,
        "directory_count": facts.directory_count,
        "truncated": facts.truncated,
        "extensions": facts
            .extensions
            .iter()
            .map(|entry| serde_json::json!({
                "extension": entry.extension,
                "count": entry.count,
            }))
            .collect::<Vec<_>>(),
    })
}

/// Emit a `rename` file report (text lines plus the fixed JSON shape).
fn emit_rename_report(format: OutputFormat, report: &RenameReport) {
    let mut lines = vec![
        format!("target: {}", report.target.display()),
        format!("current: {}", report.current_name),
        format!("pattern: {}", report.pattern),
    ];
    if let Some(proposal) = report.proposal {
        lines.push(format!("proposal: {proposal}"));
    }
    if !report.date_source.is_empty() {
        lines.push(format!("date_source: {}", report.date_source));
    }
    if let Some(reason) = report.reason {
        lines.push(format!("reason: {reason}"));
    }
    if let Some(video) = report.video {
        lines.push(format!("video: {}", video.summary()));
    }
    if let Some(perception) = report.perception {
        lines.push(format!("perception: {}", perception.summary()));
    }
    if let Some(directory) = report.directory {
        lines.push(format!("directory: {}", directory.summary()));
    }
    if let Some(operation_id) = report.operation_id {
        lines.push(format!("operation_id: {operation_id}"));
    }
    lines.push(format!("status: {}", report.status));
    if report.status == "proposed" {
        lines.push("mode: dry-run (pass --execute to rename)".to_string());
    }
    let json = serde_json::json!({
        "rename": {
            "target": report.target.display().to_string(),
            "current_name": report.current_name,
            "proposal": report.proposal,
            "status": report.status,
            "reason": report.reason,
            "pattern": report.pattern,
            "date_source": report.date_source,
            "operation_id": report.operation_id,
            "perception": report.perception.map(perception_json),
            "video": report.video.map(video_json),
            "directory": report.directory.map(directory_json),
        }
    })
    .to_string();
    emit_report(format, "Rename", &lines, &json);
}

/// `rename <file>`: dry-run proposal or `--execute` rename.
/// Reject rename targets that are neither documents, images, videos nor folders.
///
/// Unknown types fail fast here with the message that names what the command
/// accepts, so no extraction or model call is spent on them. Folders are
/// accepted by filesystem type (a folder called `notes.png` is a folder); the
/// folder-specific refusals (system trees, bundles, the Library tree, caches,
/// hidden folders, the working directory) are applied by the planner, which
/// compares canonical paths.
fn reject_foreign_rename_target(target: &Path) -> Result<()> {
    if target.is_dir()
        || kaseifu::context::image_metadata::is_image_file(target)
        || kaseifu::context::video::is_video_file(target)
    {
        return Ok(());
    }
    classify_document(target)
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("{error}"))
}

async fn rename_file(
    store: &RenameStore,
    target: &Path,
    args: &RenameCli,
    format: OutputFormat,
) -> Result<()> {
    // Classify the extension before touching the model: documents, images
    // and videos are renamed here; unknown types fail fast with the message
    // that names what the command accepts.
    reject_foreign_rename_target(target)?;
    let options = rename_options(args);
    let provider = FoundationModelsProvider::production();
    let history = OperationHistory::open(journal_path())?;
    let outcome = plan_rename(&provider, store, target, &options)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    match outcome {
        PlanOutcome::Ready(plan) => rename_ready(
            &plan,
            &history,
            args.execute && !args.dry_run,
            format,
            target,
        ),
        PlanOutcome::Noop {
            current_name,
            rule_label,
        } => {
            emit_rename_report(
                format,
                &RenameReport {
                    current_name: &current_name,
                    proposal: Option::None,
                    pattern: &rule_label,
                    date_source: "",
                    status: "noop",
                    reason: Some("name is already current"),
                    operation_id: Option::None,
                    target,
                    perception: Option::None,
                    video: Option::None,
                    directory: Option::None,
                },
            );
            Ok(())
        }
        PlanOutcome::Held {
            current_name,
            rule_label,
            reason,
        } => {
            emit_rename_report(
                format,
                &RenameReport {
                    current_name: &current_name,
                    proposal: Option::None,
                    pattern: &rule_label,
                    date_source: "",
                    status: "held",
                    reason: Some(&reason),
                    operation_id: Option::None,
                    target,
                    perception: Option::None,
                    video: Option::None,
                    directory: Option::None,
                },
            );
            Ok(())
        }
    }
}

/// Report a ready plan: preview it, or execute and report the journal id.
/// The naming options one `rename` invocation carries.
fn rename_options(args: &RenameCli) -> RenameOptions {
    RenameOptions {
        pattern: args.pattern.clone(),
        template: args.template.clone(),
        preset: args.preset.clone(),
        prefix: args.prefix.clone(),
        suffix: args.suffix.clone(),
        project: args.project.clone(),
        seq: args.seq,
        date_format: args.date_format.clone(),
    }
}

/// `rename <folder> --recursive`: preview or apply a bounded batch.
///
/// Planning is sequential and bounded: at most
/// [`kaseifu::rename::batch::MAX_BATCH_TARGETS`] targets, one on-device
/// generation each (original spec section 11.2). A target the single-target
/// rules hold or refuse becomes a held item instead of failing the run (section
/// 12), and two items that would write one name are held together before
/// anything is applied.
async fn rename_batch(
    store: &RenameStore,
    root: &Path,
    args: &RenameCli,
    format: OutputFormat,
) -> Result<()> {
    if !root.is_dir() {
        anyhow::bail!("--recursive needs a folder: {}", root.display());
    }
    let provider = FoundationModelsProvider::production();
    let options = rename_options(args);
    let walk = kaseifu::rename::batch::walk(root)?;
    // An empty folder is an empty batch: nothing to plan means nothing to ask
    // the model for, so the model is only required when there is work.
    if !walk.targets.is_empty() {
        require_model(&provider)?;
    }
    let mut report = kaseifu::rename::batch::plan(&provider, store, &walk, root, &options).await;
    let execute = args.execute && !args.dry_run;
    // The confirmation question exists only on a terminal and never for machine
    // output: a run without one must not block, and one invocation must stay one
    // parseable JSON document (original spec section 8.5 with this tool's
    // non-TTY rule).
    let prompt =
        execute && !args.yes && format != OutputFormat::Json && std::io::stdin().is_terminal();
    if !execute {
        emit_batch_report(format, &report, BatchPhase::Preview);
        return Ok(());
    }
    if prompt {
        emit_batch_report(format, &report, BatchPhase::Awaiting);
    }
    if !confirmed_by_prompt(prompt)? {
        println!("aborted: nothing was renamed");
        return Ok(());
    }
    let history = OperationHistory::open(journal_path())?;
    kaseifu::rename::batch::apply(&mut report, &history);
    emit_batch_report(format, &report, BatchPhase::Result);
    Ok(())
}

/// Fail the whole run when the on-device model cannot start at all.
///
/// A batch is not a place for the same failure repeated per target: the
/// acceptance criteria ask for a safe exit (original spec section 15), so an
/// unavailable model is reported once, before any planning is spent.
fn require_model(provider: &FoundationModelsProvider) -> Result<()> {
    match provider.availability() {
        Ok(Availability::Available) => Ok(()),
        Ok(Availability::Unavailable { reason, detail }) => {
            let detail = detail.map(|text| format!(": {text}")).unwrap_or_default();
            anyhow::bail!(
                "the on-device model is not available ({reason}{detail}); nothing was renamed"
            )
        }
        Err(error) => {
            anyhow::bail!("the on-device model is not available ({error}); nothing was renamed")
        }
    }
}

/// Ask the confirmation question, when there is one to ask.
///
/// Answering `true` without reading anything is the behaviour for every run
/// without a terminal: `--execute` is already the explicit request, and a
/// pipeline must never wait for input.
fn confirmed_by_prompt(prompt: bool) -> Result<bool> {
    if !prompt {
        return Ok(true);
    }
    print!("Apply changes? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Which part of a batch run a report describes.
///
/// The summary has to say the right thing at each step: a dry run is not an
/// execution waiting for its confirmation, and neither is the result of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchPhase {
    /// Nothing will be applied unless `--execute` is passed.
    Preview,
    /// `--execute` was passed and the answer to the question is pending.
    Awaiting,
    /// The ready proposals were applied (or refused) and the batch is over.
    Result,
}

/// Emit a batch report: one summary panel plus one line per item, or one JSON
/// document.
///
/// The item lines stay plain in every mode: a panel would wrap them, and the
/// list is the part the user reads (original spec section 8.5).
fn emit_batch_report(format: OutputFormat, report: &BatchReport, phase: BatchPhase) {
    if format == OutputFormat::Json {
        println!("{}", batch_json(report, phase));
        return;
    }
    let mode = resolve_mode(format);
    let lines = batch_summary_lines(report, phase);
    if mode.rich {
        print!(
            "{}",
            kaseifu::presentation::components::panel::frame("Rename (batch)", &lines, mode.color)
        );
    } else {
        for line in &lines {
            println!("{line}");
        }
    }
    for line in batch_item_lines(report) {
        println!("{line}");
    }
}

/// The summary a batch prints above its item list.
fn batch_summary_lines(report: &BatchReport, phase: BatchPhase) -> Vec<String> {
    let mut lines = vec![
        format!("folder: {}", report.root.display()),
        format!("proposals: {}", report.proposals()),
        format!("conflicts: {}", report.conflicts()),
        format!("held: {}", report.held()),
        format!("noop: {}", report.noop()),
    ];
    if report.truncated {
        lines.push(format!(
            "bounded: the folder holds more than {} targets; only those were planned",
            kaseifu::rename::batch::MAX_BATCH_TARGETS
        ));
    }
    lines.push(format!("status: {}", batch_status(report, phase)));
    match phase {
        BatchPhase::Preview => lines.push("mode: dry-run (pass --execute to rename)".to_string()),
        BatchPhase::Awaiting => {
            lines.push("mode: execute (waiting for confirmation)".to_string());
        }
        BatchPhase::Result => {
            lines.push("order: applied children before parents (deepest first)".to_string());
            let recorded = report.recorded();
            if recorded > 0 {
                lines.push(format!("undo: kaseifu undo --last {recorded}"));
            }
        }
    }
    lines
}

/// The word for what a batch report shows.
fn batch_status(report: &BatchReport, phase: BatchPhase) -> &'static str {
    if phase != BatchPhase::Result {
        return "proposed";
    }
    let ready = report.proposals();
    let recorded = report.recorded();
    if ready > 0 && recorded == ready {
        "applied"
    } else if recorded == 0 {
        "not applied"
    } else {
        "partially applied"
    }
}

/// One numbered line per item, in the order the batch planned them.
fn batch_item_lines(report: &BatchReport) -> Vec<String> {
    report
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| format!("{}. {}", index + 1, batch_item_line(item)))
        .collect()
}

/// What one item line says.
fn batch_item_line(item: &BatchItem) -> String {
    let mut line = match item.plan() {
        Some(plan) => format!("{} -> {}", item.current_name, plan.proposal),
        None => item.current_name.clone(),
    };
    if let Some(note) = batch_item_note(item) {
        line.push_str("  [");
        line.push_str(&note);
        line.push(']');
    }
    line
}

/// The bracketed note of one item (why it is held, or what applying did).
fn batch_item_note(item: &BatchItem) -> Option<String> {
    if let Some(applied) = &item.applied {
        return Some(match applied {
            Applied::Executed(id) => format!("renamed {id}"),
            Applied::Failed(reason) => format!("failed: {reason}"),
        });
    }
    match &item.outcome {
        BatchOutcome::Propose(_) => None,
        BatchOutcome::Conflict { reason } => Some(format!("conflict: {reason}")),
        BatchOutcome::Held { reason } => Some(format!("held: {reason}")),
        BatchOutcome::Noop => Some("noop: name is already current".to_string()),
    }
}

/// The batch JSON document.
///
/// A batch is its own shape, so nothing the single-target form already answers
/// loses a key; the item objects reuse the single-target key names.
fn batch_json(report: &BatchReport, phase: BatchPhase) -> serde_json::Value {
    serde_json::json!({
        "rename": {
            "status": batch_status(report, phase),
            "target": report.root.display().to_string(),
            "proposals": report.proposals(),
            "conflicts": report.conflicts(),
            "held": report.held(),
            "noop": report.noop(),
            "truncated": report.truncated,
            "applied": report.recorded(),
            "undo_last": report.recorded(),
            "items": report.items.iter().map(batch_item_json).collect::<Vec<_>>(),
        }
    })
}

/// One batch item in the JSON report.
fn batch_item_json(item: &BatchItem) -> serde_json::Value {
    serde_json::json!({
        "target": item.target.display().to_string(),
        "current_name": item.current_name,
        "proposal": item.plan().map(|plan| plan.proposal.clone()),
        "pattern": item.plan().map(|plan| plan.rule_label.clone()),
        "date_source": item.plan().map(|plan| plan.date_source),
        "status": batch_item_status(item),
        "reason": batch_item_note(item).filter(|_| item.plan().is_none()),
        "operation_id": match &item.applied {
            Some(Applied::Executed(id)) => Some(id.clone()),
            _ => Option::None,
        },
    })
}

/// The word for one item's state.
fn batch_item_status(item: &BatchItem) -> &'static str {
    match &item.applied {
        Some(Applied::Executed(_)) => "executed",
        Some(Applied::Failed(_)) => "failed",
        None => match &item.outcome {
            BatchOutcome::Propose(_) => "proposed",
            BatchOutcome::Conflict { .. } => "conflict",
            BatchOutcome::Held { .. } => "held",
            BatchOutcome::Noop => "noop",
        },
    }
}

fn rename_ready(
    plan: &kaseifu::rename::service::RenamePlan,
    history: &OperationHistory,
    execute: bool,
    format: OutputFormat,
    target: &Path,
) -> Result<()> {
    if !execute {
        emit_rename_report(
            format,
            &RenameReport {
                current_name: &plan.current_name,
                proposal: Some(&plan.proposal),
                pattern: &plan.rule_label,
                date_source: plan.date_source,
                status: "proposed",
                reason: Option::None,
                operation_id: Option::None,
                target,
                perception: plan.perception.as_ref(),
                video: plan.video.as_ref(),
                directory: plan.directory.as_ref(),
            },
        );
        return Ok(());
    }
    let operation_id = kaseifu::rename::service::execute_plan(plan, history)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    emit_rename_report(
        format,
        &RenameReport {
            current_name: &plan.current_name,
            proposal: Some(&plan.proposal),
            pattern: &plan.rule_label,
            date_source: plan.date_source,
            status: "executed",
            reason: Option::None,
            operation_id: Some(&operation_id),
            target,
            perception: plan.perception.as_ref(),
            video: plan.video.as_ref(),
            directory: plan.directory.as_ref(),
        },
    );
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
            println!("undo failed: {error}");
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
    explain: bool,
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
        classify_view::print_rich(
            &report,
            &policy,
            all,
            verbose,
            mode.color,
            policy_thresholds(&resolved),
            explain,
        );
    } else if explain {
        // Additive only: the decision object keeps every existing key and
        // gains the explanation (ZC-5).
        let mut value = serde_json::to_value(&report.decision)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "explanation".to_string(),
                classify_view::decision_explanation(&report, &policy, policy_thresholds(&resolved))
                    .to_json(),
            );
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
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

// --- uninstall -----------------------------------------------------------

/// `uninstall`: delete only what kaseifu installed, and nothing else (ZC-11).
///
/// Dry run by default: the plan only reads the filesystem, is printed, and
/// `--execute` is required for any deletion. The Keychain is never touched in
/// `KASEIFU_KEY_SOURCE=env`, and its confirmation prompt is checked for a
/// terminal before the first deletion, so a non-TTY `--execute` with
/// `--remove-key` (or `--purge`) and without `--yes` fails with nothing
/// removed.
fn handle_uninstall(
    execute: bool,
    purge: bool,
    remove_key: bool,
    yes: bool,
    format: OutputFormat,
) -> Result<()> {
    let key_source = key_source_setting()?;
    let mut plan = uninstall::plan(
        &uninstall::UninstallOptions {
            execute,
            purge,
            remove_key,
        },
        &uninstall_paths(),
        key_source.mode,
    );
    if execute {
        if key_prompt_needed(&plan, yes) && !std::io::stdin().is_terminal() {
            anyhow::bail!(UNINSTALL_NON_TTY_KEY_GUIDANCE);
        }
        let store = keychain_store();
        uninstall::execute(
            &mut plan,
            &store,
            key_source.mode.keychain_allowed(),
            &|| {
                if yes {
                    Ok(true)
                } else {
                    prompt::confirm("Delete the stored API key?", false)
                }
            },
        );
    }
    emit_report(
        format,
        "KASEIFU / UNINSTALL",
        &uninstall_view::report_lines(&plan, &key_source),
        &uninstall_view::json_report(&plan, &key_source).to_string(),
    );
    Ok(())
}

/// Locations `uninstall` inspects: the completion override (or the detected
/// directories), the user config, the journal and the legacy CWD journal.
fn uninstall_paths() -> uninstall::UninstallPaths {
    uninstall::UninstallPaths {
        completion_dirs: kaseifu::completion::RefreshPlan::from_env().candidates,
        config: kaseifu::config::user_config_path(),
        journal: journal_path(),
        legacy_journal: PathBuf::from(kaseifu::storage::history::LEGACY_DATABASE_PATH),
    }
}

/// True when the Keychain step would ask for a confirmation.
fn key_prompt_needed(plan: &uninstall::UninstallPlan, yes: bool) -> bool {
    !yes && plan.entries.iter().any(|entry| {
        entry.kind == uninstall::TargetKind::Keychain
            && entry.action == uninstall::TargetAction::Remove
    })
}

/// Non-TTY guidance for `uninstall --execute --remove-key` (ZC-11).
const UNINSTALL_NON_TTY_KEY_GUIDANCE: &str = "deleting the Keychain API key asks for confirmation, and that needs a terminal (stdin is not a TTY)\n\
pass --yes to delete it without asking, or drop --remove-key/--purge to keep it";

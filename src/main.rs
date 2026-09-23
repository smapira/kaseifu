use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod classifier;
mod context;
mod discovery;
mod models;
mod normalize;
mod operations;
mod platform;
mod policy;
mod storage;

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

    /// Organize files automatically.
    Organize {
        path: PathBuf,
        #[arg(long, default_value_t = true)]
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

        Commands::Organize { path, dry_run } => {
            println!("Organize: {} dry_run={}", path.display(), dry_run);
        }

        Commands::Undo { last } => {
            println!("Undo last {} operation(s)", last);
        }

        Commands::Discover { file } => {
            let context = context::builder::build(&file)?;
            let normalized = normalize::normalize(context)?;
            let candidates = discovery::discover(normalized)?;

            // Output as pretty JSON
            println!("{}", serde_json::to_string_pretty(&candidates)?);
        }
    }

    Ok(())
}

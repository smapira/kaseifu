mod classifier;
mod context;
mod discovery;
mod models;
pub mod normalize;
mod operations;
mod platform;
mod policy;
mod storage;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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

    /// Discover and organize files.
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { path } => {
            let context = context::builder::build(&path)?;

            println!("{}", serde_json::to_string_pretty(&context)?);
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
    }

    Ok(())
}

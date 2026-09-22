use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn move_file(source: &Path, destination: &Path, dry_run: bool) -> Result<()> {
    if dry_run {
        println!(
            "[dry-run] {} -> {}",
            source.display(),
            destination.display()
        );

        return Ok(());
    }

    fs::rename(source, destination)?;

    Ok(())
}

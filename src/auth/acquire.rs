//! Interactive API key acquisition for the zero-configuration flow (ZC-1).
//!
//! A Jev-calling command resolves its key first. When nothing is configured
//! and stdin is a terminal, the key is read with the existing masked prompt
//! (`auth::prompt`) and stored where the selected `KASEIFU_KEY_SOURCE` mode
//! keeps keys: the macOS Keychain for `auto`/`keychain`, the project-local
//! `.env` for `env`. `env` never touches the Keychain — not even here — which
//! is the same guarantee every other key path keeps.
//!
//! The input reader is injected (the CLI passes the real prompt; tests pass a
//! closure) so the storage contract and the zero-Keychain guarantee can be
//! proven without a terminal and without the developer's real files. The key
//! value is never logged, printed or included in an error message, and nothing
//! is stored when the entry is empty or the read fails.

use crate::auth::keychain::SecretStore;
use crate::config::key_source::KeySourceMode;
use anyhow::{Context, Result};
use std::path::Path;

/// Project-local file used by `KASEIFU_KEY_SOURCE=env`.
pub const DOTENV_FILE: &str = ".env";

/// Environment variable name written into that file.
pub const DOTENV_KEY_NAME: &str = "JEV_API_KEY";

/// Where an interactively entered key was stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySaveTarget {
    /// The macOS Keychain (`auto` / `keychain`).
    Keychain,
    /// The project-local `.env` (`env`).
    Dotenv,
}

impl KeySaveTarget {
    /// Display label for the save confirmation (never the key itself).
    pub fn label(&self) -> &'static str {
        match self {
            Self::Keychain => "the macOS Keychain",
            Self::Dotenv => ".env",
        }
    }
}

/// Read one key from the terminal and store it where `mode` keeps keys.
///
/// `auto`/`keychain` write the Keychain; `env` writes `.env` and never calls
/// the store (the injected [`SecretStore`] records any access in tests). An
/// empty entry or a read failure returns an error with nothing stored.
pub fn prompt_and_store_key(
    mode: KeySourceMode,
    store: &dyn SecretStore,
    dotenv_path: &Path,
    read_secret: &dyn Fn() -> Result<String>,
) -> Result<KeySaveTarget> {
    let secret = read_secret()?;
    let key = secret.trim();
    if key.is_empty() {
        anyhow::bail!("API key must not be empty (nothing was saved)");
    }
    if mode.keychain_allowed() {
        store
            .set(key)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        return Ok(KeySaveTarget::Keychain);
    }
    save_key_to_dotenv(dotenv_path, key)?;
    Ok(KeySaveTarget::Dotenv)
}

/// Guidance for a Jev command that found no key and cannot ask (no TTY).
///
/// The wording keeps the established failure reasons (`no API key is
/// configured`, the effective `KASEIFU_KEY_SOURCE`) and adds the way out:
/// the commands that work without a key, plus `setup`.
///
/// `discover` is deliberately not listed: it is the hidden debug view of one
/// pipeline stage, not a supported workflow.
pub fn non_tty_guidance(mode: KeySourceMode) -> String {
    let reason = match mode {
        KeySourceMode::Auto => "no API key is configured".to_string(),
        KeySourceMode::Env => {
            "no API key is configured (KASEIFU_KEY_SOURCE=env); set JEV_API_KEY in the environment or in .env"
                .to_string()
        }
        KeySourceMode::Keychain => {
            "no API key is stored in the macOS Keychain (KASEIFU_KEY_SOURCE=keychain); run the release-build kaseifu setup to store it"
                .to_string()
        }
    };
    format!(
        "{reason}\n\
         interactive key input needs a terminal (stdin is not a TTY)\n\
         commands that work without a key: kaseifu inspect <file>, kaseifu doctor\n\
         run kaseifu setup to configure kaseifu"
    )
}

/// Create or update the `JEV_API_KEY` assignment in a project-local `.env`.
///
/// Every other line is kept as it is. An existing assignment is replaced in
/// place (an `export ` prefix is kept); otherwise the assignment is appended.
/// The file is written atomically (sibling temp file plus rename) with mode
/// `0600`, so a secret is never left world-readable and an interrupted write
/// never truncates an existing file.
pub fn save_key_to_dotenv(path: &Path, key: &str) -> Result<()> {
    let assignment = dotenv_assignment(key)?;
    let existing = read_dotenv(path)?;
    let updated = replace_assignment(&existing, &assignment);
    write_dotenv(path, &updated)
}

/// `JEV_API_KEY=<value>` with the value rendered for `dotenvy` to read back.
fn dotenv_assignment(key: &str) -> Result<String> {
    Ok(format!("{DOTENV_KEY_NAME}={}", render_value(key)?))
}

/// Render one value so it survives a `dotenvy` round trip verbatim.
///
/// Simple values stay unquoted. Anything `dotenvy` would interpret (`$`,
/// `\`, quotes, whitespace, `#`, control characters) is single-quoted, which
/// the parser reads literally. Values that cannot be expressed (a line break,
/// or a `'` inside a value that needs quoting) are rejected with nothing
/// saved rather than written in a form that would come back different.
fn render_value(value: &str) -> Result<String> {
    if value.contains(['\n', '\r', '\0']) {
        anyhow::bail!("the API key contains a line break and cannot be stored in .env");
    }
    let needs_quoting = value
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '$' | '\\' | '\'' | '"' | '#'));
    if !needs_quoting {
        return Ok(value.to_string());
    }
    if value.contains('\'') {
        anyhow::bail!("the API key cannot be stored in .env (nothing was saved)");
    }
    Ok(format!("'{value}'"))
}

fn read_dotenv(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// Replace the `JEV_API_KEY` assignment (or append it), keeping every other
/// line in order.
fn replace_assignment(existing: &str, assignment: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in existing.lines() {
        if !replaced && is_key_assignment(line) {
            lines.push(with_export_prefix(line, assignment));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(assignment.to_string());
    }
    let mut updated = lines.join("\n");
    updated.push('\n');
    updated
}

/// True when `line` assigns [`DOTENV_KEY_NAME`] (an `export ` prefix, extra
/// spaces around the name, or quoted values do not hide it).
fn is_key_assignment(line: &str) -> bool {
    let trimmed = line.trim_start();
    let body = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    body.split_once('=')
        .is_some_and(|(name, _)| name.trim() == DOTENV_KEY_NAME)
}

/// Keep an existing `export ` prefix so the file's style is not changed.
fn with_export_prefix(line: &str, assignment: &str) -> String {
    if line.trim_start().starts_with("export ") {
        format!("export {assignment}")
    } else {
        assignment.to_string()
    }
}

/// Write the `.env` contents atomically with mode `0600`.
fn write_dotenv(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    let temp = sibling_temp_path(path);
    write_private(&temp, contents)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error).with_context(|| format!("replace {}", path.display()));
    }
    Ok(())
}

/// Sibling temporary file name, following the `config::loader` convention.
fn sibling_temp_path(path: &Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| DOTENV_FILE.to_string());
    parent.join(format!(".{name}.tmp-{}", std::process::id()))
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("write {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

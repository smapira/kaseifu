//! Rename rule storage in the existing SQLite journal database.
//!
//! No new database file and no TOML/YAML/JSON settings file: user presets
//! and the default rule live in two tables beside `operations`, created with
//! the same `CREATE TABLE IF NOT EXISTS` pattern as
//! [`crate::storage::history::OperationHistory`]. Builtin presets P01-P12
//! stay code constants and are never seeded.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;

use super::rules::{DEFAULT_RULE_ID, find_preset, validate_template_shape};

/// Settings key holding the default rule.
pub const DEFAULT_RULE_KEY: &str = "default_rule";

/// One stored user preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPreset {
    pub name: String,
    pub template: String,
    pub date_format: String,
}

/// Preset and settings storage on the journal database file.
pub struct RenameStore {
    conn: Connection,
}

impl RenameStore {
    /// Open (or create) the store at `path`, ensuring only its own tables.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create rename store directory {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open rename store {}", path.display()))?;
        Self::from_connection(conn)
    }

    /// In-memory store (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS rename_presets (
                name         TEXT PRIMARY KEY CHECK (name <> ''),
                template     TEXT NOT NULL CHECK (template <> ''),
                date_format  TEXT NOT NULL DEFAULT 'YYYY-MM-DD',
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rename_settings (
                key          TEXT PRIMARY KEY CHECK (key <> ''),
                value        TEXT NOT NULL
            );",
        )
        .context("initialize rename store schema")?;
        Ok(Self { conn })
    }

    /// Save (insert or replace) a user preset.
    pub fn save_preset(&self, name: &str, template: &str, date_format: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("preset name must not be empty");
        }
        validate_template_shape(template, date_format)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO rename_presets (name, template, date_format, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(name) DO UPDATE SET template = ?2, date_format = ?3, updated_at = ?4",
            rusqlite::params![name, template, date_format, now],
        )?;
        Ok(())
    }

    /// Delete a user preset (`true` when a row existed).
    pub fn delete_preset(&self, name: &str) -> Result<bool> {
        let changed = self.conn.execute(
            "DELETE FROM rename_presets WHERE name = ?1",
            rusqlite::params![name.trim()],
        )?;
        Ok(changed > 0)
    }

    /// Fetch one user preset by name.
    pub fn get_preset(&self, name: &str) -> Result<Option<UserPreset>> {
        let mut statement = self
            .conn
            .prepare("SELECT name, template, date_format FROM rename_presets WHERE name = ?1")?;
        let mut rows = statement.query(rusqlite::params![name.trim()])?;
        match rows.next()? {
            Some(row) => Ok(Some(UserPreset {
                name: row.get(0)?,
                template: row.get(1)?,
                date_format: row.get(2)?,
            })),
            Option::None => Ok(Option::None),
        }
    }

    /// All user presets in name order.
    pub fn list_presets(&self) -> Result<Vec<UserPreset>> {
        let mut statement = self
            .conn
            .prepare("SELECT name, template, date_format FROM rename_presets ORDER BY name ASC")?;
        let mut rows = statement.query([])?;
        let mut presets = Vec::new();
        while let Some(row) = rows.next()? {
            presets.push(UserPreset {
                name: row.get(0)?,
                template: row.get(1)?,
                date_format: row.get(2)?,
            });
        }
        Ok(presets)
    }

    /// The default rule: a stored value, or [`DEFAULT_RULE_ID`] when unset.
    pub fn get_default(&self) -> Result<String> {
        let mut statement = self
            .conn
            .prepare("SELECT value FROM rename_settings WHERE key = ?1")?;
        let mut rows = statement.query(rusqlite::params![DEFAULT_RULE_KEY])?;
        match rows.next()? {
            Some(row) => Ok(row.get(0)?),
            Option::None => Ok(DEFAULT_RULE_ID.to_string()),
        }
    }

    /// Store the default rule (a preset id/slug or `preset:<name>`).
    pub fn set_default(&self, spec: &str) -> Result<()> {
        let resolved = resolve_rule_reference(self, spec)?;
        self.conn.execute(
            "INSERT INTO rename_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            rusqlite::params![DEFAULT_RULE_KEY, resolved],
        )?;
        Ok(())
    }
}

/// Normalize a default-rule reference to its stored form.
///
/// Accepts a builtin id/slug (stored as the id), `preset:<name>`, or a bare
/// preset name (stored as `preset:<name>`). Anything else is an error, never
/// silently the default.
fn resolve_rule_reference(store: &RenameStore, spec: &str) -> Result<String> {
    let trimmed = spec.trim();
    if let Some(preset) = find_preset(trimmed) {
        return Ok(preset.id.to_string());
    }
    let bare = trimmed.strip_prefix("preset:").unwrap_or(trimmed).trim();
    if store.get_preset(bare)?.is_some() {
        return Ok(format!("preset:{bare}"));
    }
    anyhow::bail!("unknown rule: {trimmed} (expected P01-P12, a slug, or preset:<name>)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_in_memory() {
        let store = RenameStore::open_in_memory().expect("open store");
        assert!(store.list_presets().expect("list").is_empty());
        store
            .save_preset("memo", "{date}_{title}", "YYYY-MM-DD")
            .expect("save");
        let fetched = store.get_preset("memo").expect("get").expect("present");
        assert_eq!(fetched.template, "{date}_{title}");
        assert_eq!(store.list_presets().expect("list").len(), 1);
        store
            .save_preset("memo", "{title}", "YYYYMMDD")
            .expect("overwrite");
        let updated = store.get_preset("memo").expect("get").expect("present");
        assert_eq!(
            (updated.template.as_str(), updated.date_format.as_str()),
            ("{title}", "YYYYMMDD")
        );
        assert!(store.delete_preset("memo").expect("delete"));
        assert!(!store.delete_preset("memo").expect("delete again"));
        assert!(store.get_preset("memo").expect("get").is_none());
    }

    #[test]
    fn preset_saves_reject_bad_shapes() {
        let store = RenameStore::open_in_memory().expect("open store");
        assert!(store.save_preset("", "{title}", "YYYY-MM-DD").is_err());
        assert!(store.save_preset("x", "{unknown}", "YYYY-MM-DD").is_err());
        assert!(store.save_preset("x", "{title}", "DD-MM-YYYY").is_err());
    }

    #[test]
    fn default_rule_falls_back_and_validates() {
        let store = RenameStore::open_in_memory().expect("open store");
        assert_eq!(store.get_default().expect("default"), DEFAULT_RULE_ID);
        store.set_default("date-title").expect("set slug");
        assert_eq!(store.get_default().expect("default"), "P02");
        assert!(store.set_default("P13").is_err());
        assert!(store.set_default("preset:missing").is_err());
        store
            .save_preset("memo", "{title}", "YYYY-MM-DD")
            .expect("save");
        store.set_default("memo").expect("set bare preset name");
        assert_eq!(store.get_default().expect("default"), "preset:memo");
    }
}

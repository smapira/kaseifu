//! Phase 5 - SQLite operation journal.
//!
//! Design rule (docs/design/05-execute.md): a mutation is recorded
//! **before** it happens and updated with its result afterwards, so a
//! filesystem mutation can never be unjournaled.
//!
//! Columns: id, timestamp, operation, source_path, destination_path,
//! candidate_id, confidence, margin, result, error, undone_at.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;

/// Default journal location (see config/default.toml; git-ignored).
pub const DEFAULT_DATABASE_PATH: &str = "data/kaseifu.db";

/// One journal row.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: String,
    /// `"move"` or `"undo"`.
    pub operation: String,
    pub source_path: String,
    pub destination_path: String,
    pub candidate_id: Option<String>,
    pub confidence: Option<f64>,
    pub margin: Option<f64>,
    /// `"pending"`, `"success"` or `"failure"`.
    pub result: String,
    pub error: Option<String>,
    pub undone_at: Option<String>,
}

/// The append-oriented operation journal.
pub struct OperationHistory {
    conn: Connection,
}

impl OperationHistory {
    /// Open (or create) the journal at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create journal directory {}", parent.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open journal {}", path.display()))?;
        Self::from_connection(conn)
    }

    /// In-memory journal (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS operations (
                id               TEXT PRIMARY KEY,
                timestamp        TEXT NOT NULL,
                operation        TEXT NOT NULL,
                source_path      TEXT NOT NULL,
                destination_path TEXT NOT NULL,
                candidate_id     TEXT,
                confidence       REAL,
                margin           REAL,
                result           TEXT NOT NULL,
                error            TEXT,
                undone_at        TEXT
            );",
        )
        .context("initialize journal schema")?;
        Ok(Self { conn })
    }

    /// Record an operation **before** mutating the filesystem.
    ///
    /// Returns the operation id used by [`OperationHistory::mark_success`]
    /// and [`OperationHistory::mark_failure`].
    pub fn record_pending(
        &self,
        operation: &str,
        source: &Path,
        destination: &Path,
        candidate_id: Option<&str>,
        confidence: Option<f64>,
        margin: Option<f64>,
    ) -> Result<String> {
        let id = new_operation_id();
        self.conn.execute(
            "INSERT INTO operations
                (id, timestamp, operation, source_path, destination_path,
                 candidate_id, confidence, margin, result)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending')",
            rusqlite::params![
                id,
                now(),
                operation,
                source.to_string_lossy(),
                destination.to_string_lossy(),
                candidate_id,
                confidence,
                margin,
            ],
        )?;
        Ok(id)
    }

    /// Mark a recorded operation as successful.
    pub fn mark_success(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET result = 'success', error = NULL WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    /// Mark a recorded operation as failed (with a detail message).
    pub fn mark_failure(&self, id: &str, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET result = 'failure', error = ?2 WHERE id = ?1",
            rusqlite::params![id, error],
        )?;
        Ok(())
    }

    /// Mark a successful move as undone (it is no longer undoable).
    pub fn mark_undone(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET undone_at = ?2 WHERE id = ?1",
            rusqlite::params![id, now()],
        )?;
        Ok(())
    }

    /// Most recent successful, not-yet-undone move.
    pub fn last_successful_move(&self) -> Result<Option<HistoryEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT id, timestamp, operation, source_path, destination_path,
                    candidate_id, confidence, margin, result, error, undone_at
             FROM operations
             WHERE operation = 'move' AND result = 'success' AND undone_at IS NULL
             ORDER BY rowid DESC
             LIMIT 1",
        )?;
        let mut rows = statement.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_entry(row)?)),
            None => Ok(None),
        }
    }

    /// All rows in insertion order.
    pub fn entries(&self) -> Result<Vec<HistoryEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT id, timestamp, operation, source_path, destination_path,
                    candidate_id, confidence, margin, result, error, undone_at
             FROM operations
             ORDER BY rowid ASC",
        )?;
        let mut rows = statement.query([])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push(row_to_entry(row)?);
        }
        Ok(entries)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        operation: row.get(2)?,
        source_path: row.get(3)?,
        destination_path: row.get(4)?,
        candidate_id: row.get(5)?,
        confidence: row.get(6)?,
        margin: row.get(7)?,
        result: row.get(8)?,
        error: row.get(9)?,
        undone_at: row.get(10)?,
    })
}

/// Timestamp helper (RFC 3339, UTC).
fn now() -> String {
    Utc::now().to_rfc3339()
}

/// Operation id helper (process id + nanos keeps ids unique in practice).
fn new_operation_id() -> String {
    format!(
        "op-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

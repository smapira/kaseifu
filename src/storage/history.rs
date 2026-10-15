//! Phase 5 - SQLite operation journal.
//!
//! Design rule (docs/design/05-execute.md): a mutation is recorded
//! **before** it happens and updated with its result afterwards, so a
//! filesystem mutation can never be unjournaled.
//!
//! Columns: id, timestamp, operation, source_path, destination_path,
//! candidate_id, confidence, margin, result, error, undone_at, batch_id.
//!
//! `batch_id` arrived later (batch undo): old journals lack the column and
//! gain it through the additive migration in [`OperationHistory::open`].

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// File name of the journal inside the user data directory.
pub const JOURNAL_FILE_NAME: &str = "kaseifu.db";

/// Legacy journal location: the CWD-relative default used before the journal
/// moved to the user data directory.
///
/// Kept only so `doctor` can point out a leftover journal from an older
/// build. Nothing reads, migrates, overwrites or deletes it: the file may
/// hold a real user's undo history, and silently moving or merging it could
/// lose or misattribute moves (a CWD-relative name can exist once per
/// working directory).
pub const LEGACY_DATABASE_PATH: &str = "data/kaseifu.db";

/// Default journal location: `<app_dir>/kaseifu.db`
/// (`~/Library/Application Support/kaseifu/kaseifu.db` on macOS).
///
/// Absolute and independent of the current working directory: the previous
/// default (`data/kaseifu.db`) silently created a different journal per
/// working directory, so `undo` could not see moves recorded elsewhere and a
/// changed working directory looked like an empty history.
pub fn journal_path() -> PathBuf {
    crate::config::storage::app_dir().join(JOURNAL_FILE_NAME)
}

/// A `pending` row is treated as an interrupted operation after this long.
///
/// A shorter-lived pending row may belong to a move another process is
/// performing right now; it is reported with its file state either way.
pub const PENDING_STALE_AFTER_SECS: i64 = 3600;

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
    /// The rename batch this row belongs to (`None` for single moves and
    /// for rows written before the column existed).
    pub batch_id: Option<String>,
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
        ensure_batch_column(&conn)?;
        Ok(Self { conn })
    }
}

/// What the journal files away before a mutation (the batch-undo extension
/// rides on the same insert so the row shape never depends on the caller).
#[derive(Debug, Clone, Copy)]
pub struct PendingRecord<'a> {
    /// `"move"` or `"undo"`.
    pub operation: &'a str,
    /// Where the file is now.
    pub source: &'a Path,
    /// Where the mutation takes it.
    pub destination: &'a Path,
    pub candidate_id: Option<&'a str>,
    pub confidence: Option<f64>,
    pub margin: Option<f64>,
    /// The rename batch the operation belongs to (`None` for single moves).
    pub batch_id: Option<&'a str>,
}

impl OperationHistory {
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
        self.record_pending_in_batch(PendingRecord {
            operation,
            source,
            destination,
            candidate_id,
            confidence,
            margin,
            batch_id: None,
        })
    }

    /// [`record_pending`](OperationHistory::record_pending) for a batch member
    /// (batch undo reverses exactly these rows).
    pub fn record_pending_in_batch(&self, pending: PendingRecord<'_>) -> Result<String> {
        let id = new_operation_id();
        self.conn.execute(
            "INSERT INTO operations
                (id, timestamp, operation, source_path, destination_path,
                 candidate_id, confidence, margin, result, batch_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9)",
            rusqlite::params![
                id,
                now(),
                pending.operation,
                pending.source.to_string_lossy(),
                pending.destination.to_string_lossy(),
                pending.candidate_id,
                pending.confidence,
                pending.margin,
                pending.batch_id,
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
                    candidate_id, confidence, margin, result, error, undone_at, batch_id
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

    /// Successful, not-yet-undone moves of one batch, newest first.
    ///
    /// Empty for an unknown batch id (the caller reports nothing to undo).
    /// Rows written before the migration carry NULL and never match.
    pub fn successful_moves_in_batch(&self, batch_id: &str) -> Result<Vec<HistoryEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT id, timestamp, operation, source_path, destination_path,
                    candidate_id, confidence, margin, result, error, undone_at, batch_id
             FROM operations
             WHERE operation = 'move' AND result = 'success' AND undone_at IS NULL
               AND batch_id = ?1
             ORDER BY rowid DESC",
        )?;
        let mut rows = statement.query([batch_id])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push(row_to_entry(row)?);
        }
        Ok(entries)
    }

    /// Every row still marked `pending`, in insertion order.
    ///
    /// A pending row means a mutation was recorded but its result was never
    /// written: the process was interrupted (between `record_pending` and the
    /// rename, or between the rename and `mark_success`), or another process
    /// is performing the operation right now. Callers **report** these rows;
    /// nothing repairs, rewrites or deletes them: this journal cannot tell an
    /// interrupted move from one that is still in flight, and silently
    /// rewriting the row (or moving the file back) could destroy a state that
    /// is actually correct.
    pub fn pending_entries(&self) -> Result<Vec<HistoryEntry>> {
        query_pending(&self.conn)
    }

    /// All rows in insertion order.
    pub fn entries(&self) -> Result<Vec<HistoryEntry>> {
        let mut statement = self.conn.prepare(
            "SELECT id, timestamp, operation, source_path, destination_path,
                    candidate_id, confidence, margin, result, error, undone_at, batch_id
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
        // The read-only inspection path selects the legacy column list, so a
        // missing index reads as no batch (old journals keep working there
        // without the migration, which needs a writable connection).
        batch_id: row.get(11).unwrap_or(None),
    })
}

/// Every `pending` row of a connection, in insertion order.
fn query_pending(conn: &Connection) -> Result<Vec<HistoryEntry>> {
    let mut statement = conn.prepare(
        "SELECT id, timestamp, operation, source_path, destination_path,
                candidate_id, confidence, margin, result, error, undone_at
         FROM operations
         WHERE result = 'pending'
         ORDER BY rowid ASC",
    )?;
    let mut rows = statement.query([])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        entries.push(row_to_entry(row)?);
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Read-only inspection (diagnostics)
// ---------------------------------------------------------------------------

/// Additive batch-undo migration: old journals lack `batch_id`, new ones
/// have it. Checked (not attempted blindly) so a genuine database problem
/// still surfaces instead of being swallowed with the expected case.
fn ensure_batch_column(conn: &Connection) -> Result<()> {
    let existing: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('operations')")
        .context("inspect journal schema")?
        .query_map([], |row| row.get(0))
        .context("inspect journal schema")?
        .map(|name| name.unwrap_or_default())
        .collect();
    if !existing.iter().any(|name| name == "batch_id") {
        conn.execute("ALTER TABLE operations ADD COLUMN batch_id TEXT", [])
            .context("migrate journal schema (batch_id)")?;
    }
    Ok(())
}

/// One `pending` row with the filesystem state it refers to.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingOperation {
    /// The pending row itself.
    pub entry: HistoryEntry,
    /// Age in seconds, when the row timestamp parses as RFC 3339.
    pub age_seconds: Option<i64>,
    /// True when the row is old enough to be an interrupted operation (an
    /// unparseable timestamp counts as stale).
    pub stale: bool,
    /// The recorded source path still exists on disk.
    pub source_exists: bool,
    /// The recorded destination path exists on disk.
    pub destination_exists: bool,
}

/// Read-only journal snapshot for diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalInspection {
    /// Total number of recorded operations.
    pub total_operations: u64,
    /// Rows still marked `pending`, in insertion order.
    pub pending: Vec<PendingOperation>,
}

/// Inspect a journal without writing anything.
///
/// `Ok(None)` means no journal file exists yet (it is created on first use).
/// `doctor` must never create, initialize or lock the journal for writing, so
/// this opens a read-only connection and never runs the schema setup.
pub fn inspect_journal(path: &Path) -> Result<Option<JournalInspection>> {
    inspect_journal_at(path, Utc::now())
}

/// [`inspect_journal`] with an explicit clock, so tests can age a pending row
/// without faking its timestamp.
pub fn inspect_journal_at(path: &Path, now: DateTime<Utc>) -> Result<Option<JournalInspection>> {
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open journal {} (read-only)", path.display()))?;
    inspect_connection(&conn, now).map(Some)
}

/// Read the journal contents of an already-open connection.
fn inspect_connection(conn: &Connection, now: DateTime<Utc>) -> Result<JournalInspection> {
    let table_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'operations'",
        [],
        |row| row.get(0),
    )?;
    if table_count == 0 {
        anyhow::bail!("not an initialized kaseifu journal (no operations table)");
    }
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))?;
    let pending = query_pending(conn)?
        .into_iter()
        .map(|entry| to_pending_operation(entry, now))
        .collect();
    Ok(JournalInspection {
        total_operations: total.max(0) as u64,
        pending,
    })
}

/// Pair a pending row with its age and the current filesystem state.
fn to_pending_operation(entry: HistoryEntry, now: DateTime<Utc>) -> PendingOperation {
    let age_seconds = parse_age_seconds(&entry.timestamp, now);
    PendingOperation {
        stale: age_seconds.is_none_or(|age| age >= PENDING_STALE_AFTER_SECS),
        age_seconds,
        source_exists: Path::new(&entry.source_path).exists(),
        destination_exists: Path::new(&entry.destination_path).exists(),
        entry,
    }
}

/// Age of a journal timestamp in seconds (RFC 3339, UTC).
fn parse_age_seconds(timestamp: &str, now: DateTime<Utc>) -> Option<i64> {
    DateTime::parse_from_rfc3339(timestamp).ok().map(|at| {
        now.signed_duration_since(at.with_timezone(&Utc))
            .num_seconds()
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

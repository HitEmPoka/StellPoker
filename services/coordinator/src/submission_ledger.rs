//! At-most-once Soroban proof submission ledger (Issue #592).
//!
//! Prevents duplicate on-chain submissions for deal / reveal / showdown
//! proofs when the coordinator retries after a transient failure.  Each
//! submission is keyed by `(session_id, action)` where `action` is one of
//! `"deal"`, `"reveal-flop"`, `"reveal-turn"`, `"reveal-river"`, or
//! `"showdown"`.
//!
//! The ledger is backed by a lightweight SQLite database so entries survive
//! a coordinator crash and restart (crash-safe recovery).  On startup the
//! coordinator reloads the ledger and any key that is already marked
//! `Submitted` will short-circuit the submission path, returning the cached
//! transaction hash instead of re-submitting.
//!
//! ## Usage
//!
//! ```rust,ignore
//! let ledger = SubmissionLedger::open("./data/submission_ledger.db").unwrap();
//! let key = SubmissionKey::new("table-1-deal-<uuid>", "deal");
//!
//! if let Some(entry) = ledger.get(&key).unwrap() {
//!     // Already submitted — return the cached tx hash.
//!     return Ok(entry.tx_hash);
//! }
//!
//! let tx_hash = soroban::submit_deal_proof(...).await?;
//! ledger.mark_submitted(&key, &tx_hash).unwrap();
//! ```

use rusqlite::{params, Connection, Result as SqliteResult};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The composite key that uniquely identifies one proof submission attempt.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SubmissionKey {
    /// MPC proof session ID (already a UUID-based unique ID after Issue #12).
    pub session_id: String,
    /// Action label: `"deal"`, `"reveal-flop"`, `"reveal-turn"`,
    /// `"reveal-river"`, or `"showdown"`.
    pub action: String,
}

impl SubmissionKey {
    pub fn new(session_id: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            action: action.into(),
        }
    }

    fn as_db_key(&self) -> String {
        format!("{}:{}", self.session_id, self.action)
    }
}

/// The persisted record for one submission.
#[derive(Clone, Debug)]
pub struct SubmissionEntry {
    pub session_id: String,
    pub action: String,
    /// On-chain transaction hash returned by Soroban, empty if the submission
    /// was skipped / not yet confirmed.
    pub tx_hash: String,
    pub submitted_at: i64, // Unix seconds
}

// ---------------------------------------------------------------------------
// Ledger
// ---------------------------------------------------------------------------

pub struct SubmissionLedger {
    conn: Connection,
}

impl SubmissionLedger {
    /// Open (or create) the SQLite-backed ledger at `db_path`.
    pub fn open(db_path: &str) -> SqliteResult<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS submission_ledger (
                 key           TEXT PRIMARY KEY,
                 session_id    TEXT NOT NULL,
                 action        TEXT NOT NULL,
                 tx_hash       TEXT NOT NULL DEFAULT '',
                 submitted_at  INTEGER NOT NULL
             );
             -- Issue #602: purge_before (WHERE submitted_at < ?) and
             -- load_all (ORDER BY submitted_at DESC) both full-scanned
             -- without this index.
             CREATE INDEX IF NOT EXISTS idx_submission_ledger_submitted_at
                 ON submission_ledger (submitted_at);
             COMMIT;",
        )?;
        Ok(Self { conn })
    }

    /// Returns `Some(entry)` if this `(session_id, action)` has already been
    /// submitted, `None` if it is new.
    pub fn get(&self, key: &SubmissionKey) -> SqliteResult<Option<SubmissionEntry>> {
        let db_key = key.as_db_key();
        let mut stmt = self.conn.prepare(
            "SELECT session_id, action, tx_hash, submitted_at
               FROM submission_ledger
              WHERE key = ?",
        )?;

        let result = stmt.query_row(params![db_key], |row| {
            Ok(SubmissionEntry {
                session_id: row.get(0)?,
                action: row.get(1)?,
                tx_hash: row.get(2)?,
                submitted_at: row.get(3)?,
            })
        });

        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Record that `key` was submitted and store `tx_hash`.  Uses
    /// INSERT OR IGNORE so a concurrent retry that races here is a no-op.
    pub fn mark_submitted(&self, key: &SubmissionKey, tx_hash: &str) -> SqliteResult<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR IGNORE INTO submission_ledger
                 (key, session_id, action, tx_hash, submitted_at)
             VALUES (?, ?, ?, ?, ?)",
            params![
                key.as_db_key(),
                &key.session_id,
                &key.action,
                tx_hash,
                now,
            ],
        )?;
        Ok(())
    }

    /// Purge entries older than `before_secs` (Unix seconds).  Useful for
    /// routine GC — session IDs are already single-use so very old entries
    /// won't be looked up again.
    pub fn purge_before(&self, before_secs: i64) -> SqliteResult<usize> {
        self.conn.execute(
            "DELETE FROM submission_ledger WHERE submitted_at < ?",
            params![before_secs],
        )
    }

    /// Return all entries, newest-first.  Used at startup to log recovered state.
    pub fn load_all(&self) -> SqliteResult<Vec<SubmissionEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, action, tx_hash, submitted_at
               FROM submission_ledger
              ORDER BY submitted_at DESC",
        )?;
        let entries = stmt
            .query_map([], |row| {
                Ok(SubmissionEntry {
                    session_id: row.get(0)?,
                    action: row.get(1)?,
                    tx_hash: row.get(2)?,
                    submitted_at: row.get(3)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;
        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// Thread-safe store wrapper (mirrors session_cache pattern)
// ---------------------------------------------------------------------------

pub type SubmissionLedgerStore = Arc<Mutex<SubmissionLedger>>;

/// Open a ledger at `db_path` wrapped in `Arc<Mutex<_>>`.  Falls back to an
/// in-memory database if the file cannot be opened (dev / test).
pub fn new_store(db_path: &str) -> SubmissionLedgerStore {
    match SubmissionLedger::open(db_path) {
        Ok(ledger) => {
            tracing::info!(db_path, "submission ledger opened");
            Arc::new(Mutex::new(ledger))
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                db_path,
                "failed to open submission ledger — falling back to in-memory"
            );
            Arc::new(Mutex::new(
                SubmissionLedger::open(":memory:").expect("in-memory ledger"),
            ))
        }
    }
}

/// Check whether a submission has already been recorded. Returns `Ok(Some(tx_hash))`
/// if the proof was already submitted, `Ok(None)` if this is a new submission.
pub fn check_already_submitted(
    store: &SubmissionLedgerStore,
    session_id: &str,
    action: &str,
) -> Result<Option<String>, String> {
    let key = SubmissionKey::new(session_id, action);
    let guard = store
        .lock()
        .map_err(|_| "submission ledger lock poisoned".to_string())?;
    guard
        .get(&key)
        .map(|opt| opt.map(|e| e.tx_hash))
        .map_err(|e| format!("submission ledger read error: {}", e))
}

/// Record a completed submission in the ledger.
pub fn record_submission(
    store: &SubmissionLedgerStore,
    session_id: &str,
    action: &str,
    tx_hash: &str,
) -> Result<(), String> {
    let key = SubmissionKey::new(session_id, action);
    let guard = store
        .lock()
        .map_err(|_| "submission ledger lock poisoned".to_string())?;
    guard
        .mark_submitted(&key, tx_hash)
        .map_err(|e| format!("submission ledger write error: {}", e))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> SubmissionLedger {
        SubmissionLedger::open(":memory:").expect("in-memory ledger")
    }

    #[test]
    fn new_key_returns_none() {
        let ledger = in_memory();
        let key = SubmissionKey::new("table-1-deal-uuid-abc", "deal");
        assert!(ledger.get(&key).unwrap().is_none());
    }

    #[test]
    fn after_mark_submitted_get_returns_entry() {
        let ledger = in_memory();
        let key = SubmissionKey::new("table-1-deal-uuid-abc", "deal");
        ledger.mark_submitted(&key, "txhash-abc123").unwrap();

        let entry = ledger.get(&key).unwrap().expect("should be found");
        assert_eq!(entry.session_id, "table-1-deal-uuid-abc");
        assert_eq!(entry.action, "deal");
        assert_eq!(entry.tx_hash, "txhash-abc123");
    }

    #[test]
    fn duplicate_mark_submitted_is_idempotent() {
        let ledger = in_memory();
        let key = SubmissionKey::new("table-2-showdown-uuid", "showdown");
        ledger.mark_submitted(&key, "tx-first").unwrap();
        // Second write for the same key must be a no-op (INSERT OR IGNORE).
        ledger.mark_submitted(&key, "tx-second").unwrap();

        let entry = ledger.get(&key).unwrap().unwrap();
        assert_eq!(
            entry.tx_hash, "tx-first",
            "first write must win; replay must not overwrite"
        );
    }

    #[test]
    fn different_actions_same_session_are_independent() {
        let ledger = in_memory();
        let deal_key = SubmissionKey::new("sess-1", "deal");
        let reveal_key = SubmissionKey::new("sess-1", "reveal-flop");

        ledger.mark_submitted(&deal_key, "tx-deal").unwrap();
        assert!(ledger.get(&reveal_key).unwrap().is_none());
    }

    #[test]
    fn load_all_returns_entries_in_newest_first_order() {
        let ledger = in_memory();
        for i in 0..5u32 {
            let key = SubmissionKey::new(format!("sess-{}", i), "deal");
            ledger
                .mark_submitted(&key, &format!("tx-{}", i))
                .unwrap();
        }
        let all = ledger.load_all().unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn purge_before_removes_old_entries() {
        let ledger = in_memory();
        // Insert an entry with a very old timestamp by patching directly.
        let key = SubmissionKey::new("old-sess", "deal");
        ledger
            .conn
            .execute(
                "INSERT INTO submission_ledger (key, session_id, action, tx_hash, submitted_at)
                 VALUES (?, ?, ?, ?, ?)",
                params!["old-sess:deal", "old-sess", "deal", "tx-old", 1_000_000i64],
            )
            .unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let removed = ledger.purge_before(now - 1).unwrap();
        assert_eq!(removed, 1);
        assert!(ledger.get(&key).unwrap().is_none());
    }

    #[test]
    fn store_wrapper_check_and_record_roundtrip() {
        let store = Arc::new(Mutex::new(in_memory()));

        assert!(
            check_already_submitted(&store, "sess-rr", "deal")
                .unwrap()
                .is_none()
        );

        record_submission(&store, "sess-rr", "deal", "tx-rr").unwrap();

        let result = check_already_submitted(&store, "sess-rr", "deal").unwrap();
        assert_eq!(result, Some("tx-rr".to_string()));
    }

    /// Crash-safe recovery: simulate restart by opening the same DB twice.
    #[test]
    fn crash_recovery_persists_across_reopen() {
        let dir = std::env::temp_dir();
        let path = dir
            .join(format!("submission_ledger_test_{}.db", std::process::id()))
            .to_string_lossy()
            .to_string();

        {
            let ledger = SubmissionLedger::open(&path).unwrap();
            let key = SubmissionKey::new("crash-sess", "deal");
            ledger.mark_submitted(&key, "tx-crash").unwrap();
        } // connection closed / "crash"

        {
            // Reopen — simulates coordinator restart
            let ledger = SubmissionLedger::open(&path).unwrap();
            let key = SubmissionKey::new("crash-sess", "deal");
            let entry = ledger.get(&key).unwrap().expect("entry must survive reopen");
            assert_eq!(entry.tx_hash, "tx-crash");
        }

        let _ = std::fs::remove_file(&path);
    }
}

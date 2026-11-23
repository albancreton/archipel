//! The session log in one SQLite file.
//!
//! The log is a cache: the harness keeps the transcript of record. So writes favour speed over
//! surviving a power cut (`synchronous = NORMAL` with a write-ahead log): a crash of the daemon
//! loses nothing, a crash of the machine may lose the last moments.

use bridge_core::{
    log::{LogEntry, Record},
    store::{SessionRecord, Store, StoreError},
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;

/// Version of the tables, kept in SQLite's `user_version`.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    cwd        TEXT NOT NULL,
    title      TEXT,
    state      TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS entries (
    root       TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    session_id TEXT NOT NULL,
    record     TEXT NOT NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (root, seq)
) WITHOUT ROWID;
";

const SESSION_COLUMNS: &str = "session_id, agent_id, cwd, title, state, created_at, updated_at,
    (SELECT COALESCE(MAX(seq), 0) FROM entries WHERE root = sessions.session_id)";

/// A [`Store`] backed by a SQLite file.
#[derive(Debug)]
pub struct SqliteStore {
    db: Connection,
}

impl SqliteStore {
    /// Opens the file, creating it and its tables when missing.
    ///
    /// # Errors
    /// When the file cannot be opened, or was written by a newer version of this code.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::prepare(Connection::open(path).map_err(failed)?)
    }

    /// A store that lives in memory, for tests.
    ///
    /// # Errors
    /// When SQLite cannot start.
    pub fn in_memory() -> Result<Self, StoreError> {
        Self::prepare(Connection::open_in_memory().map_err(failed)?)
    }

    fn prepare(db: Connection) -> Result<Self, StoreError> {
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(failed)?;
        db.pragma_update(None, "synchronous", "NORMAL")
            .map_err(failed)?;
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(failed)?;
        if version > SCHEMA_VERSION {
            return Err(StoreError(format!(
                "the log file has version {version}, this program knows up to {SCHEMA_VERSION}"
            )));
        }
        db.execute_batch(SCHEMA).map_err(failed)?;
        db.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(failed)?;
        Ok(Self { db })
    }
}

fn failed(error: impl std::fmt::Display) -> StoreError {
    StoreError(error.to_string())
}

fn session_from(row: &Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        session_id: row.get(0)?,
        agent_id: row.get(1)?,
        cwd: row.get(2)?,
        title: row.get(3)?,
        state: row.get(4)?,
        created_at: row.get::<_, i64>(5)?.cast_unsigned(),
        updated_at: row.get::<_, i64>(6)?.cast_unsigned(),
        head_seq: row.get::<_, i64>(7)?.cast_unsigned(),
    })
}

impl Store for SqliteStore {
    fn put_session(&mut self, record: &SessionRecord) -> Result<(), StoreError> {
        self.db
            .execute(
                "INSERT INTO sessions (session_id, agent_id, cwd, title, state, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (session_id) DO UPDATE SET
                    agent_id = ?2, cwd = ?3, title = ?4, state = ?5, updated_at = ?7",
                params![
                    record.session_id,
                    record.agent_id,
                    record.cwd,
                    record.title,
                    record.state,
                    record.created_at.cast_signed(),
                    record.updated_at.cast_signed(),
                ],
            )
            .map(drop)
            .map_err(failed)
    }

    fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError> {
        let mut query = self
            .db
            .prepare_cached(&format!(
                "SELECT {SESSION_COLUMNS} FROM sessions ORDER BY updated_at DESC, session_id"
            ))
            .map_err(failed)?;
        query
            .query_map([], session_from)
            .and_then(Iterator::collect)
            .map_err(failed)
    }

    fn session(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        self.db
            .prepare_cached(&format!(
                "SELECT {SESSION_COLUMNS} FROM sessions WHERE session_id = ?1"
            ))
            .and_then(|mut query| query.query_row([session_id], session_from).optional())
            .map_err(failed)
    }

    fn append(
        &mut self,
        root: &str,
        session_id: &str,
        record: &Record,
        at: u64,
    ) -> Result<u64, StoreError> {
        let body = serde_json::to_string(record).map_err(failed)?;
        let at = at.cast_signed();
        let tx = self.db.transaction().map_err(failed)?;
        let known = tx
            .execute(
                "UPDATE sessions SET updated_at = ?2 WHERE session_id = ?1",
                params![root, at],
            )
            .map_err(failed)?;
        if known == 0 {
            return Err(StoreError(format!("unknown session {root}")));
        }
        let seq: i64 = tx
            .query_row(
                "INSERT INTO entries (root, seq, session_id, record, at)
                 SELECT ?1, COALESCE(MAX(seq), 0) + 1, ?2, ?3, ?4 FROM entries WHERE root = ?1
                 RETURNING seq",
                params![root, session_id, body, at],
                |row| row.get(0),
            )
            .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(seq.cast_unsigned())
    }

    fn after(&self, root: &str, seq: u64) -> Result<Vec<LogEntry>, StoreError> {
        let mut query = self
            .db
            .prepare_cached(
                "SELECT seq, session_id, record FROM entries WHERE root = ?1 AND seq > ?2 ORDER BY seq",
            )
            .map_err(failed)?;
        let rows = query
            .query_map(params![root, seq.cast_signed()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(failed)?;
        rows.map(|row| {
            let (seq, session_id, body) = row.map_err(failed)?;
            Ok(LogEntry {
                seq: seq.cast_unsigned(),
                session_id,
                record: serde_json::from_str(&body).map_err(failed)?,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, at: u64) -> SessionRecord {
        SessionRecord {
            session_id: id.into(),
            agent_id: "claude".into(),
            cwd: "/work".into(),
            title: None,
            state: None,
            head_seq: 0,
            created_at: at,
            updated_at: at,
        }
    }

    fn update(text: &str) -> Record {
        Record::Update {
            update: json!({"sessionUpdate": "agent_message_chunk", "messageId": "m", "content": {"type": "text", "text": text}}),
        }
    }

    #[test]
    fn entries_are_numbered_per_session_and_read_back_in_order() {
        let mut store = SqliteStore::in_memory().unwrap();
        store.put_session(&record("a", 1)).unwrap();
        store.put_session(&record("b", 2)).unwrap();
        assert_eq!(store.append("a", "a", &update("x"), 10).unwrap(), 1);
        assert_eq!(store.append("a", "child", &update("y"), 11).unwrap(), 2);
        assert_eq!(store.append("b", "b", &update("z"), 5).unwrap(), 1);
        assert!(store.append("missing", "m", &update("?"), 6).is_err());

        let later = store.after("a", 1).unwrap();
        assert_eq!(later.len(), 1);
        assert_eq!((later[0].seq, later[0].session_id.as_str()), (2, "child"));
        assert_eq!(later[0].record, update("y"));

        let sessions = store.sessions().unwrap();
        let listed: Vec<_> = sessions
            .iter()
            .map(|s| (s.session_id.as_str(), s.head_seq, s.updated_at))
            .collect();
        assert_eq!(listed, vec![("a", 2, 11), ("b", 1, 5)]);
    }

    #[test]
    fn a_session_keeps_its_log_when_its_title_changes_and_survives_reopening() {
        let path = std::env::temp_dir().join(format!("bridge-store-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut store = SqliteStore::open(&path).unwrap();
            store.put_session(&record("a", 1)).unwrap();
            store.append("a", "a", &update("x"), 2).unwrap();
            store
                .put_session(&SessionRecord {
                    title: Some("Hello".into()),
                    state: Some("idle".into()),
                    created_at: 99,
                    updated_at: 3,
                    ..record("a", 1)
                })
                .unwrap();
        }
        let store = SqliteStore::open(&path).unwrap();
        let session = store.session("a").unwrap().unwrap();
        assert_eq!(session.title.as_deref(), Some("Hello"));
        assert_eq!(
            (session.created_at, session.updated_at, session.head_seq),
            (1, 3, 1)
        );
        assert_eq!(store.session("nope").unwrap(), None);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}

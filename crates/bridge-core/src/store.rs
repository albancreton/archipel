//! Where sessions and their logs are kept. Implementations decide how; this crate has one in memory.

use crate::log::{LogEntry, Record};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;

/// What the daemon knows about a session without reading its log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    /// The agent that owns the session, such as `claude`.
    pub agent_id: String,
    pub cwd: String,
    pub title: Option<String>,
    /// The last ACP turn state: `running`, `requires_action` or `idle`.
    pub state: Option<String>,
    /// Sequence number of the newest log entry, or 0 when the log is empty.
    pub head_seq: u64,
    /// Milliseconds since the Unix epoch.
    pub created_at: u64,
    /// Milliseconds since the Unix epoch.
    pub updated_at: u64,
}

/// A storage failure, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "store: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

/// Sessions and their logs. One log per root session; a subagent's entries are in its parent's log.
pub trait Store: Send {
    /// Adds a session, or updates its title, state and times. `head_seq` is ignored.
    ///
    /// # Errors
    /// When the storage fails.
    fn put_session(&mut self, record: &SessionRecord) -> Result<(), StoreError>;

    /// All sessions, most recently updated first.
    ///
    /// # Errors
    /// When the storage fails.
    fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError>;

    /// One session, if it exists.
    ///
    /// # Errors
    /// When the storage fails.
    fn session(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError>;

    /// Appends to the log of `root` and returns the new entry's sequence number.
    ///
    /// # Errors
    /// When the storage fails or the session does not exist.
    fn append(
        &mut self,
        root: &str,
        session_id: &str,
        record: &Record,
        at: u64,
    ) -> Result<u64, StoreError>;

    /// The entries of `root` after `seq`, oldest first.
    ///
    /// # Errors
    /// When the storage fails.
    fn after(&self, root: &str, seq: u64) -> Result<Vec<LogEntry>, StoreError>;
}

/// A store held in memory. Used in tests and when nothing needs to survive a restart.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    sessions: IndexMap<String, (SessionRecord, Vec<LogEntry>)>,
}

impl Store for MemoryStore {
    fn put_session(&mut self, record: &SessionRecord) -> Result<(), StoreError> {
        match self.sessions.get_mut(&record.session_id) {
            Some((existing, entries)) => {
                *existing = SessionRecord {
                    head_seq: entries.len() as u64,
                    created_at: existing.created_at,
                    ..record.clone()
                };
            }
            None => {
                let record = SessionRecord {
                    head_seq: 0,
                    ..record.clone()
                };
                self.sessions
                    .insert(record.session_id.clone(), (record, Vec::new()));
            }
        }
        Ok(())
    }

    fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError> {
        let mut all: Vec<_> = self.sessions.values().map(|(r, _)| r.clone()).collect();
        all.sort_by_key(|record| std::cmp::Reverse(record.updated_at));
        Ok(all)
    }

    fn session(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self.sessions.get(session_id).map(|(r, _)| r.clone()))
    }

    fn append(
        &mut self,
        root: &str,
        session_id: &str,
        record: &Record,
        at: u64,
    ) -> Result<u64, StoreError> {
        let (session, entries) = self
            .sessions
            .get_mut(root)
            .ok_or_else(|| StoreError(format!("unknown session {root}")))?;
        let seq = entries.len() as u64 + 1;
        entries.push(LogEntry {
            seq,
            session_id: session_id.to_owned(),
            record: record.clone(),
        });
        session.head_seq = seq;
        session.updated_at = at;
        Ok(seq)
    }

    fn after(&self, root: &str, seq: u64) -> Result<Vec<LogEntry>, StoreError> {
        Ok(self
            .sessions
            .get(root)
            .map_or_else(Vec::new, |(_, entries)| {
                let start = entries.partition_point(|entry| entry.seq <= seq);
                entries[start..].to_vec()
            }))
    }
}

/// Formats milliseconds since the Unix epoch as an RFC 3339 time in UTC.
#[must_use]
pub fn rfc3339(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Civil date from a day count (Howard Hinnant's algorithm), for dates from 1970 on.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str) -> SessionRecord {
        SessionRecord {
            session_id: id.into(),
            agent_id: "claude".into(),
            cwd: "/tmp".into(),
            title: None,
            state: None,
            head_seq: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn sequence_numbers_start_at_one_per_session_and_after_returns_the_rest() {
        let mut store = MemoryStore::default();
        store.put_session(&record("a")).unwrap();
        store.put_session(&record("b")).unwrap();
        let update = Record::Update {
            update: json!({"sessionUpdate": "state_update", "state": "idle"}),
        };
        assert_eq!(store.append("a", "a", &update, 5).unwrap(), 1);
        assert_eq!(store.append("a", "child", &update, 6).unwrap(), 2);
        assert_eq!(store.append("b", "b", &update, 7).unwrap(), 1);
        assert!(store.append("missing", "x", &update, 8).is_err());
        let later: Vec<u64> = store.after("a", 1).unwrap().iter().map(|e| e.seq).collect();
        assert_eq!(later, vec![2]);
        assert_eq!(store.session("a").unwrap().unwrap().head_seq, 2);
        assert_eq!(store.sessions().unwrap()[0].session_id, "b");
    }

    #[test]
    fn times_are_formatted_in_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_795_392_000_123), "2026-11-23T00:00:00.123Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }
}

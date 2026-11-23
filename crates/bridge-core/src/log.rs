//! The session log: every update gets a sequence number, and a client catches up from one.

use crate::{json, state};
use bridge_schema::rules::{Action, RuleSet, StateRule};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One logged `session/update`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Position in the log, starting at 1, without gaps.
    pub seq: u64,
    /// The session the update belongs to. A subagent's child session has its own id.
    pub session_id: String,
    /// The body of the update, as sent on the wire.
    pub update: Value,
}

/// Where a session's updates are kept. Implementations decide how they are stored.
pub trait LogStore {
    /// Adds an update and returns its sequence number.
    fn append(&mut self, session_id: &str, update: Value) -> u64;
    /// The sequence number of the newest update, or 0 when the log is empty.
    fn head(&self) -> u64;
    /// The updates after `seq`, oldest first.
    fn after(&self, seq: u64) -> Vec<LogEntry>;
}

/// A log held in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemoryLog {
    entries: Vec<LogEntry>,
}

impl LogStore for MemoryLog {
    fn append(&mut self, session_id: &str, update: Value) -> u64 {
        let seq = self.head() + 1;
        self.entries.push(LogEntry {
            seq,
            session_id: session_id.to_owned(),
            update,
        });
        seq
    }

    fn head(&self) -> u64 {
        self.entries.last().map_or(0, |entry| entry.seq)
    }

    fn after(&self, seq: u64) -> Vec<LogEntry> {
        let start = self.entries.partition_point(|entry| entry.seq <= seq);
        self.entries[start..].to_vec()
    }
}

/// Shortens a run of updates for replay: neighbouring chunks of the same item become one update.
///
/// Applying the result gives the same state as applying the input. A merged update keeps the
/// sequence number of its last part, so a client that applied it is up to date to that point.
#[must_use]
pub fn condense(rules: &RuleSet, entries: &[LogEntry]) -> Vec<LogEntry> {
    let mut condensed: Vec<LogEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(previous) = condensed.last_mut()
            && let Some(rule) = chunk_rule(rules, &entry.update)
            && merge_chunk(previous, entry, rule)
        {
            continue;
        }
        condensed.push(entry.clone());
    }
    condensed
}

/// The rule of an update that only appends to one item.
fn chunk_rule<'a>(rules: &'a RuleSet, update: &Value) -> Option<&'a StateRule> {
    let kind = update.get("sessionUpdate")?.as_str()?;
    match rules.updates.get(kind)?.session.as_slice() {
        [rule] if rule.action == Action::Append && rule.id.is_some() && rule.value.is_some() => {
            Some(rule)
        }
        _ => None,
    }
}

/// Folds `next` into `previous` when both are chunks of the same item. Returns whether it did.
fn merge_chunk(previous: &mut LogEntry, next: &LogEntry, rule: &StateRule) -> bool {
    let (Some(id_path), Some(value_path)) = (rule.id.as_deref(), rule.value.as_deref()) else {
        return false;
    };
    let same_item = previous.session_id == next.session_id
        && previous.update.get("sessionUpdate") == next.update.get("sessionUpdate")
        && json::get(&previous.update, id_path).is_some()
        && json::get(&previous.update, id_path) == json::get(&next.update, id_path);
    if !same_item {
        return false;
    }
    let (Some(old), Some(new)) = (
        json::get(&previous.update, value_path),
        json::get(&next.update, value_path),
    ) else {
        return false;
    };
    let merged = match (old, new) {
        (Value::String(old), Value::String(new)) => {
            Value::String(state::concat_text(old, new, state::is_base64(rule)))
        }
        (old, new) => match (state::text_of(old), state::text_of(new)) {
            (Some(old_text), Some(new_text)) => {
                let mut block = old.clone();
                block["text"] = Value::String(format!("{old_text}{new_text}"));
                block
            }
            _ => return false,
        },
    };
    let Some(body) = previous.update.as_object_mut() else {
        return false;
    };
    *json::slot(body, value_path) = merged;
    previous.seq = next.seq;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chunk(id: &str, text: &str) -> Value {
        json!({"sessionUpdate": "agent_message_chunk", "messageId": id, "content": {"type": "text", "text": text}})
    }

    #[test]
    fn sequence_numbers_start_at_one_and_after_returns_the_rest() {
        let mut log = MemoryLog::default();
        assert_eq!(log.head(), 0);
        assert_eq!(log.append("s", chunk("m", "a")), 1);
        assert_eq!(log.append("s", chunk("m", "b")), 2);
        assert_eq!(
            log.after(1).iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![2]
        );
        assert!(log.after(2).is_empty());
    }

    #[test]
    fn condense_merges_only_neighbouring_chunks_of_the_same_item_and_session() {
        let mut log = MemoryLog::default();
        log.append("s", chunk("m1", "Hel"));
        log.append("s", chunk("m1", "lo"));
        log.append("s", chunk("m2", "x"));
        log.append("child", chunk("m2", "y"));
        log.append(
            "s",
            json!({"sessionUpdate": "state_update", "state": "idle"}),
        );
        let condensed = condense(&RuleSet::pinned(), &log.after(0));
        assert_eq!(
            condensed.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
        assert_eq!(condensed[0].update["content"]["text"], json!("Hello"));
    }
}

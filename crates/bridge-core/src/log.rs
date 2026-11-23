//! The session log: every event gets a sequence number, and a client catches up from one.

use crate::{json, state};
use bridge_schema::rules::{Action, RuleSet, StateRule};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// What one log entry holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Record {
    /// The body of a `session/update`, as sent on the wire.
    Update { update: Value },
    /// A request from the agent that waits for the user: an approval or a question.
    Request {
        /// The id the daemon gave the request. Stable across clients and reconnects.
        request_id: String,
        method: String,
        params: Value,
        /// Never stored. [`condense`] sets it when the answer is later in the same run.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<Value>,
    },
    /// The answer to an earlier request.
    Resolved {
        request_id: String,
        method: String,
        outcome: Value,
        /// A label for the client that answered, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_by: Option<String>,
    },
}

/// One numbered entry of a session's log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Position in the log, starting at 1, without gaps.
    pub seq: u64,
    /// The session the entry belongs to. A subagent's child session has its own id.
    pub session_id: String,
    pub record: Record,
}

impl LogEntry {
    /// The update body, when the entry is an update.
    #[must_use]
    pub fn update(&self) -> Option<&Value> {
        match &self.record {
            Record::Update { update } => Some(update),
            _ => None,
        }
    }
}

/// Shortens a run of entries for replay.
///
/// - Neighbouring chunks of the same item become one update.
/// - A request whose answer is in the run carries the answer, and the answer entry is dropped.
///
/// Applying the result gives the same state as applying the input. A merged update keeps the
/// sequence number of its last part.
#[must_use]
pub fn condense(rules: &RuleSet, entries: &[LogEntry]) -> Vec<LogEntry> {
    let mut condensed: Vec<LogEntry> = Vec::with_capacity(entries.len());
    let mut requests: HashMap<&str, usize> = HashMap::new();
    for entry in entries {
        match &entry.record {
            Record::Update { update } => {
                if let Some(previous) = condensed.last_mut()
                    && let Some(rule) = chunk_rule(rules, update)
                    && merge_chunk(previous, entry, update, rule)
                {
                    continue;
                }
            }
            Record::Request { request_id, .. } => {
                requests.insert(request_id, condensed.len());
            }
            Record::Resolved {
                request_id,
                outcome: answer,
                ..
            } => {
                if let Some(&position) = requests.get(request_id.as_str())
                    && let Record::Request { outcome, .. } = &mut condensed[position].record
                {
                    *outcome = Some(answer.clone());
                    continue;
                }
            }
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
fn merge_chunk(previous: &mut LogEntry, next: &LogEntry, new: &Value, rule: &StateRule) -> bool {
    let previous_session = previous.session_id.clone();
    let Record::Update { update: old } = &mut previous.record else {
        return false;
    };
    let (Some(id_path), Some(value_path)) = (rule.id.as_deref(), rule.value.as_deref()) else {
        return false;
    };
    let same_item = previous_session == next.session_id
        && old.get("sessionUpdate") == new.get("sessionUpdate")
        && json::get(old, id_path).is_some()
        && json::get(old, id_path) == json::get(new, id_path);
    if !same_item {
        return false;
    }
    let (Some(old_value), Some(new_value)) =
        (json::get(old, value_path), json::get(new, value_path))
    else {
        return false;
    };
    let merged = match (old_value, new_value) {
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
    let Some(body) = old.as_object_mut() else {
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

    fn chunk(seq: u64, session: &str, id: &str, text: &str) -> LogEntry {
        LogEntry {
            seq,
            session_id: session.to_owned(),
            record: Record::Update {
                update: json!({"sessionUpdate": "agent_message_chunk", "messageId": id, "content": {"type": "text", "text": text}}),
            },
        }
    }

    #[test]
    fn condense_merges_only_neighbouring_chunks_of_the_same_item_and_session() {
        let entries = vec![
            chunk(1, "s", "m1", "Hel"),
            chunk(2, "s", "m1", "lo"),
            chunk(3, "s", "m2", "x"),
            chunk(4, "child", "m2", "y"),
            LogEntry {
                seq: 5,
                session_id: "s".into(),
                record: Record::Update {
                    update: json!({"sessionUpdate": "state_update", "state": "idle"}),
                },
            },
        ];
        let condensed = condense(&RuleSet::pinned(), &entries);
        assert_eq!(
            condensed.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
        assert_eq!(
            condensed[0].update().unwrap()["content"]["text"],
            json!("Hello")
        );
    }

    #[test]
    fn condense_puts_the_answer_on_its_request() {
        let request = |seq, id: &str| LogEntry {
            seq,
            session_id: "s".into(),
            record: Record::Request {
                request_id: id.into(),
                method: "session/request_permission".into(),
                params: json!({"sessionId": "s"}),
                outcome: None,
            },
        };
        let entries = vec![
            request(1, "a"),
            request(2, "b"),
            LogEntry {
                seq: 3,
                session_id: "s".into(),
                record: Record::Resolved {
                    request_id: "a".into(),
                    method: "session/request_permission".into(),
                    outcome: json!({"outcome": {"outcome": "cancelled"}}),
                    resolved_by: None,
                },
            },
        ];
        let condensed = condense(&RuleSet::pinned(), &entries);
        assert_eq!(condensed.len(), 2);
        assert!(
            matches!(
                &condensed[0].record,
                Record::Request {
                    outcome: Some(_),
                    ..
                }
            ),
            "answered request carries its answer"
        );
        assert!(
            matches!(&condensed[1].record, Record::Request { outcome: None, .. }),
            "unanswered request stays pending"
        );
        // An answer whose request is not in the run is kept.
        assert_eq!(condense(&RuleSet::pinned(), &entries[2..]).len(), 1);
    }
}

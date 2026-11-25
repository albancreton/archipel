//! State rules: how each ACP message changes what a client holds.
//!
//! ACP v2 annotates its schema with `x-state`. Each annotation says that a message appends to,
//! upserts, replaces or removes a *target* (a message, a tool call, the session state, ...),
//! optionally keyed by an id found in the message. A client that follows these rules needs no
//! per-message code.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// What a rule does to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Add `value` to the end of `field` in the target entry.
    Append,
    /// Merge the message (or `value`) into the target entry, creating it if needed.
    Upsert,
    /// Replace the target (or one entry, or one field of it) with the message (or `value`).
    Replace,
    /// Delete the target entry.
    Remove,
}

/// One state change. Paths are dotted and relative to the message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateRule {
    pub action: Action,
    /// The kind of thing changed, such as `agentMessage`, `toolCall` or `state`.
    pub target: String,
    /// Where the entry's id is in the message. Absent when the target is a single value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The field of the target that is changed. Absent means the whole entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Where the new data is in the message. Absent means the whole message body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Rules for one message, split by what they change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Changes to the session the message belongs to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session: Vec<StateRule>,
    /// Changes to agent-wide state, such as the authentication status.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent: Vec<StateRule>,
}

/// Every state rule in the pinned schema.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSet {
    /// Where the rules were extracted from.
    pub source: String,
    /// `session/update` notifications, by their `sessionUpdate` value.
    pub updates: BTreeMap<String, Rules>,
    /// Requests, by method. The rules read the request's params.
    pub requests: BTreeMap<String, Rules>,
    /// Responses, by the method they answer. The rules read the result.
    pub responses: BTreeMap<String, Rules>,
    /// Other notifications, by method.
    pub notifications: BTreeMap<String, Rules>,
}

impl RuleSet {
    /// The rules checked in at `schema/state-rules.json`.
    ///
    /// # Panics
    /// If the checked-in file is not valid, which `just ci` rules out.
    #[must_use]
    pub fn pinned() -> Self {
        serde_json::from_str(crate::STATE_RULES).expect("schema/state-rules.json is valid")
    }

    /// Reads the `x-state` annotations out of an ACP schema document.
    #[must_use]
    pub fn extract(schema: &Value, source: &str) -> Self {
        let mut set = Self {
            source: source.to_owned(),
            ..Self::default()
        };
        let Some(defs) = schema.get("$defs").and_then(Value::as_object) else {
            return set;
        };

        if let Some(variants) = defs.get("SessionUpdate").and_then(variants) {
            for variant in variants {
                let kind = variant
                    .pointer("/properties/sessionUpdate/const")
                    .and_then(Value::as_str);
                if let (Some(kind), Some(rules)) = (kind, rules_of(variant)) {
                    set.updates.insert(kind.to_owned(), rules);
                }
            }
        }
        for (name, def) in defs {
            let (Some(method), Some(rules)) =
                (def.get("x-method").and_then(Value::as_str), rules_of(def))
            else {
                continue;
            };
            let bucket = if name.ends_with("Request") {
                &mut set.requests
            } else if name.ends_with("Response") {
                &mut set.responses
            } else {
                &mut set.notifications
            };
            bucket.insert(method.to_owned(), rules);
        }
        set
    }
}

fn variants(def: &Value) -> Option<&Vec<Value>> {
    def.get("oneOf")
        .or_else(|| def.get("anyOf"))
        .and_then(Value::as_array)
}

fn rules_of(def: &Value) -> Option<Rules> {
    let rules: Rules = serde_json::from_value(def.get("x-state")?.clone()).ok()?;
    Some(rules)
}

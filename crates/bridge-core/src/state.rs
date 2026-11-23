//! Client state, built by applying ACP v2 messages according to the schema's state rules.
//!
//! The rules (`schema/state-rules.json`) name a *target* for each message: `agentMessage`,
//! `toolCall`, `terminal`, `state` and so on. A target is either a collection of entries keyed by
//! an id, or a single value. Nothing here knows individual message kinds; a new kind in the schema
//! needs no code change.

use crate::json;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bridge_schema::rules::{Action, RuleSet, Rules, StateRule};
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Map, Value};
use std::fmt;

/// The key of the session update kind in an update body.
const KIND_KEY: &str = "sessionUpdate";
/// Fields whose streamed text is base64, as `(target, field)`. Joining them needs a decode first.
const BASE64_FIELDS: &[(&str, &str)] = &[("terminal", "output.data")];
/// Target name used for pending and answered requests (approvals, questions).
pub const REQUEST_TARGET: &str = "request";

/// Points at one entry of a collection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ItemRef {
    pub target: String,
    pub id: String,
}

/// What applying a message did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// An entry was created or changed.
    Item(ItemRef),
    /// An entry was removed.
    Removed(ItemRef),
    /// A single value, such as `state` or `usage`, was changed.
    Value(String),
    /// The message carries no state, such as `notice` or `replay_complete`.
    Event(String),
    /// The message kind is not in the rules. It is kept in [`SessionState::unknown`].
    Unknown(String),
}

/// Why a message could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// The update has no `sessionUpdate` field.
    MissingKind,
    /// A rule needs an id that the message does not carry.
    MissingId { kind: String, path: String },
    /// A rule points at data that the message does not carry.
    MissingValue { kind: String, path: String },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKind => write!(f, "update has no `{KIND_KEY}` field"),
            Self::MissingId { kind, path } => write!(f, "`{kind}` has no id at `{path}`"),
            Self::MissingValue { kind, path } => write!(f, "`{kind}` has no value at `{path}`"),
        }
    }
}

impl std::error::Error for ApplyError {}

/// A request from the agent that waits for the user: an approval or a question.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub method: String,
    pub params: Value,
    /// The answer, once given.
    pub outcome: Option<Value>,
}

/// Everything a client holds for one session.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionState {
    /// Entries by target, then by id.
    pub collections: IndexMap<String, IndexMap<String, Value>>,
    /// Single values by target: `state`, `usage`, `configOptions`, ...
    pub values: IndexMap<String, Value>,
    /// Entries in the order they first appeared.
    pub timeline: Vec<ItemRef>,
    /// Approvals and questions, by request id.
    pub requests: IndexMap<String, Request>,
    /// Messages without state, in order: notices and the like.
    pub events: Vec<Value>,
    /// Messages whose kind the rules do not know, in order.
    pub unknown: Vec<Value>,
}

impl SessionState {
    /// Applies the body of one `session/update` notification.
    ///
    /// # Errors
    /// When the update lacks its kind, or an id or value its rule needs.
    pub fn apply_update(
        &mut self,
        rules: &RuleSet,
        update: &Value,
    ) -> Result<Vec<Change>, ApplyError> {
        let kind = update
            .get(KIND_KEY)
            .and_then(Value::as_str)
            .ok_or(ApplyError::MissingKind)?;
        let Some(rules) = rules.updates.get(kind) else {
            self.unknown.push(update.clone());
            return Ok(vec![Change::Unknown(kind.to_owned())]);
        };
        if rules.session.is_empty() {
            self.events.push(update.clone());
            return Ok(vec![Change::Event(kind.to_owned())]);
        }
        self.apply_rules(kind, rules, update)
    }

    /// Records a request from the agent and applies its state rules.
    ///
    /// # Errors
    /// When an id or value a rule needs is missing.
    pub fn apply_request(
        &mut self,
        rules: &RuleSet,
        request_id: &str,
        method: &str,
        params: &Value,
    ) -> Result<Vec<Change>, ApplyError> {
        let mut changes = match rules.requests.get(method) {
            Some(rules) => self.apply_rules(method, rules, params)?,
            None => Vec::new(),
        };
        let request = Request {
            method: method.to_owned(),
            params: params.clone(),
            outcome: None,
        };
        if self
            .requests
            .insert(request_id.to_owned(), request)
            .is_none()
        {
            self.timeline.push(ItemRef {
                target: REQUEST_TARGET.to_owned(),
                id: request_id.to_owned(),
            });
        }
        changes.push(Change::Item(ItemRef {
            target: REQUEST_TARGET.to_owned(),
            id: request_id.to_owned(),
        }));
        Ok(changes)
    }

    /// Records the answer to a request. Returns false when the request is not known.
    pub fn resolve_request(&mut self, request_id: &str, outcome: Value) -> bool {
        match self.requests.get_mut(request_id) {
            Some(request) => {
                request.outcome = Some(outcome);
                true
            }
            None => false,
        }
    }

    /// Applies the state rules of a response, such as the config options in `session/new`.
    ///
    /// # Errors
    /// When an id or value a rule needs is missing.
    pub fn apply_response(
        &mut self,
        rules: &RuleSet,
        method: &str,
        result: &Value,
    ) -> Result<Vec<Change>, ApplyError> {
        match rules.responses.get(method) {
            Some(rules) => self.apply_rules(method, rules, result),
            None => Ok(Vec::new()),
        }
    }

    /// The entry a timeline reference points at.
    #[must_use]
    pub fn item(&self, item: &ItemRef) -> Option<&Value> {
        self.collections.get(&item.target)?.get(&item.id)
    }

    /// The requests that have no answer yet.
    pub fn pending_requests(&self) -> impl Iterator<Item = (&String, &Request)> {
        self.requests
            .iter()
            .filter(|(_, request)| request.outcome.is_none())
    }

    fn apply_rules(
        &mut self,
        kind: &str,
        rules: &Rules,
        body: &Value,
    ) -> Result<Vec<Change>, ApplyError> {
        rules
            .session
            .iter()
            .map(|rule| self.apply_rule(kind, rule, body))
            .collect()
    }

    fn apply_rule(
        &mut self,
        kind: &str,
        rule: &StateRule,
        body: &Value,
    ) -> Result<Change, ApplyError> {
        let id = match &rule.id {
            Some(path) => Some(json::get(body, path).and_then(json::id_string).ok_or_else(
                || ApplyError::MissingId {
                    kind: kind.to_owned(),
                    path: path.clone(),
                },
            )?),
            None => None,
        };
        let value = match &rule.value {
            // Optional data that is absent or null leaves the state alone.
            Some(path) => match json::get(body, path) {
                Some(Value::Null) | None if rule.action != Action::Append => {
                    return Ok(Change::Value(rule.target.clone()));
                }
                Some(value) => value.clone(),
                None => {
                    return Err(ApplyError::MissingValue {
                        kind: kind.to_owned(),
                        path: path.clone(),
                    });
                }
            },
            None => without_kind(body),
        };

        match (rule.action, id) {
            (Action::Remove, Some(id)) => {
                let item = ItemRef {
                    target: rule.target.clone(),
                    id,
                };
                if let Some(entries) = self.collections.get_mut(&item.target) {
                    entries.shift_remove(&item.id);
                }
                self.timeline.retain(|other| other != &item);
                Ok(Change::Removed(item))
            }
            (Action::Remove, None) => {
                self.values.shift_remove(&rule.target);
                Ok(Change::Value(rule.target.clone()))
            }
            (action, Some(id)) => {
                let id_key = json::leaf(rule.id.as_deref().unwrap_or_default()).to_owned();
                let item = ItemRef {
                    target: rule.target.clone(),
                    id: id.clone(),
                };
                let entries = self.collections.entry(rule.target.clone()).or_default();
                if !entries.contains_key(&id) {
                    let mut entry = Map::new();
                    entry.insert(id_key, Value::String(id.clone()));
                    entries.insert(id.clone(), Value::Object(entry));
                    self.timeline.push(item.clone());
                }
                let entry = entries.get_mut(&id).expect("inserted above");
                change_entry(entry, rule, action, value);
                Ok(Change::Item(item))
            }
            (action, None) => {
                let entry = self
                    .values
                    .entry(rule.target.clone())
                    .or_insert(Value::Null);
                change_entry(entry, rule, action, value);
                Ok(Change::Value(rule.target.clone()))
            }
        }
    }
}

/// The message body without its kind tag.
fn without_kind(body: &Value) -> Value {
    match body {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| *key != KIND_KEY)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether the rule streams base64 text.
pub(crate) fn is_base64(rule: &StateRule) -> bool {
    rule.field
        .as_deref()
        .is_some_and(|field| BASE64_FIELDS.contains(&(rule.target.as_str(), field)))
}

fn change_entry(entry: &mut Value, rule: &StateRule, action: Action, value: Value) {
    match (action, rule.field.as_deref()) {
        (Action::Append, Some(field)) => {
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            append(
                json::slot(entry.as_object_mut().expect("made an object"), field),
                value,
                is_base64(rule),
            );
        }
        (Action::Replace, Some(field)) => {
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            *json::slot(entry.as_object_mut().expect("made an object"), field) = value;
        }
        (Action::Upsert, _) => merge(entry, value),
        // Append without a field has nothing to append to; treat it like a replacement.
        (Action::Append | Action::Replace | Action::Remove, None) | (Action::Remove, Some(_)) => {
            *entry = value
        }
    }
}

/// Merges the fields of `incoming` into `entry`. A null field clears that field.
fn merge(entry: &mut Value, incoming: Value) {
    match (entry.as_object_mut(), incoming) {
        (Some(fields), Value::Object(incoming)) => {
            for (key, value) in incoming {
                if value.is_null() {
                    fields.shift_remove(&key);
                } else {
                    fields.insert(key, value);
                }
            }
        }
        (_, incoming) => *entry = incoming,
    }
}

/// Appends streamed data to a field.
///
/// - Text is concatenated. Base64 text is decoded first, so that the result is valid base64.
/// - A content block is added to a list; two neighbouring text blocks become one.
pub(crate) fn append(slot: &mut Value, value: Value, base64: bool) {
    match (&mut *slot, value) {
        (Value::Null, Value::String(text)) => *slot = Value::String(text),
        (Value::String(existing), Value::String(text)) => {
            *existing = concat_text(existing, &text, base64)
        }
        (Value::Null, block) => *slot = Value::Array(vec![block]),
        (Value::Array(blocks), block) => push_block(blocks, block),
        (other, block) => *other = Value::Array(vec![other.take(), block]),
    }
}

/// Joins two streamed strings.
pub(crate) fn concat_text(existing: &str, text: &str, base64: bool) -> String {
    if base64 && let (Ok(mut left), Ok(right)) = (BASE64.decode(existing), BASE64.decode(text)) {
        left.extend(right);
        return BASE64.encode(left);
    }
    format!("{existing}{text}")
}

fn push_block(blocks: &mut Vec<Value>, block: Value) {
    if let (Some(last), Some(text)) = (blocks.last_mut(), text_of(&block))
        && last.get("type").and_then(Value::as_str) == Some("text")
        && let Some(Value::String(existing)) = last.get_mut("text")
    {
        existing.push_str(text);
        return;
    }
    blocks.push(block);
}

pub(crate) fn text_of(block: &Value) -> Option<&str> {
    (block.get("type").and_then(Value::as_str) == Some("text"))
        .then(|| block.get("text").and_then(Value::as_str))
        .flatten()
}

/// One client's view of an adapter connection: the sessions, including subagent child sessions.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Conversation {
    /// Sessions by id, in the order they first appeared.
    pub sessions: IndexMap<String, SessionState>,
    /// Requests this side sent and has no answer to yet: request id to method and session.
    #[serde(skip)]
    outgoing: IndexMap<String, (String, Option<String>)>,
    /// Requests from the agent, to find their session when the answer goes out.
    #[serde(skip)]
    incoming: IndexMap<String, String>,
}

/// Which way a message travelled, seen from the client side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// From the agent to the client.
    In,
    /// From the client to the agent.
    Out,
}

impl Conversation {
    /// Applies one JSON-RPC message.
    ///
    /// # Errors
    /// When a message lacks an id or value that its state rule needs.
    pub fn apply_message(
        &mut self,
        rules: &RuleSet,
        direction: Direction,
        message: &Value,
    ) -> Result<Vec<Change>, ApplyError> {
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").and_then(json::id_string);
        let params = message.get("params").unwrap_or(&Value::Null);
        let session_id = params.get("sessionId").and_then(Value::as_str);

        match (direction, method, id) {
            (Direction::In, Some("session/update"), None) => {
                let (Some(session_id), Some(update)) = (session_id, params.get("update")) else {
                    return Ok(Vec::new());
                };
                self.session(session_id).apply_update(rules, update)
            }
            (Direction::In, Some(method), Some(id)) => {
                let Some(session_id) = session_id else {
                    return Ok(Vec::new());
                };
                self.incoming.insert(id.clone(), session_id.to_owned());
                self.session(session_id)
                    .apply_request(rules, &id, method, params)
            }
            (Direction::Out, Some(method), Some(id)) => {
                self.outgoing
                    .insert(id, (method.to_owned(), session_id.map(str::to_owned)));
                Ok(Vec::new())
            }
            (Direction::Out, None, Some(id)) => {
                let Some(session_id) = self.incoming.shift_remove(&id) else {
                    return Ok(Vec::new());
                };
                let outcome = message
                    .get("result")
                    .or_else(|| message.get("error"))
                    .cloned()
                    .unwrap_or(Value::Null);
                self.session(&session_id).resolve_request(&id, outcome);
                Ok(vec![Change::Item(ItemRef {
                    target: REQUEST_TARGET.to_owned(),
                    id,
                })])
            }
            (Direction::In, None, Some(id)) => {
                let Some((method, session_id)) = self.outgoing.shift_remove(&id) else {
                    return Ok(Vec::new());
                };
                let Some(result) = message.get("result") else {
                    return Ok(Vec::new());
                };
                let session_id = session_id.or_else(|| {
                    result
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
                match session_id {
                    Some(session_id) => self
                        .session(&session_id)
                        .apply_response(rules, &method, result),
                    None => Ok(Vec::new()),
                }
            }
            _ => Ok(Vec::new()),
        }
    }

    /// The first session seen, which is the one the user talks to.
    #[must_use]
    pub fn root(&self) -> Option<(&String, &SessionState)> {
        self.sessions.first()
    }

    fn session(&mut self, session_id: &str) -> &mut SessionState {
        self.sessions.entry(session_id.to_owned()).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn apply(state: &mut SessionState, update: Value) -> Vec<Change> {
        state
            .apply_update(&RuleSet::pinned(), &update)
            .expect("applies")
    }

    #[test]
    fn chunks_with_the_same_id_become_one_message() {
        let mut state = SessionState::default();
        for part in ["Hel", "lo"] {
            apply(
                &mut state,
                json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": part}}),
            );
        }
        let message = &state.collections["agentMessage"]["m1"];
        assert_eq!(
            message,
            &json!({"messageId": "m1", "content": [{"type": "text", "text": "Hello"}]})
        );
        assert_eq!(
            state.timeline,
            vec![ItemRef {
                target: "agentMessage".into(),
                id: "m1".into()
            }]
        );
    }

    #[test]
    fn a_thought_and_a_message_with_the_same_id_are_two_items() {
        let mut state = SessionState::default();
        apply(
            &mut state,
            json!({"sessionUpdate": "agent_thought_chunk", "messageId": "m1", "content": {"type": "text", "text": "hm"}}),
        );
        apply(
            &mut state,
            json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "ok"}}),
        );
        assert_eq!(state.timeline.len(), 2);
    }

    #[test]
    fn tool_call_updates_merge_and_null_clears_a_field() {
        let mut state = SessionState::default();
        apply(
            &mut state,
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "pending", "title": "Terminal"}),
        );
        apply(
            &mut state,
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed", "title": null}),
        );
        assert_eq!(
            state.collections["toolCall"]["t1"],
            json!({"toolCallId": "t1", "status": "completed"})
        );
    }

    #[test]
    fn terminal_output_is_joined_as_bytes_not_as_base64_text() {
        let mut state = SessionState::default();
        // "ab" and "cd": joining the base64 strings as text would not decode to "abcd".
        for data in ["YWI=", "Y2Q="] {
            apply(
                &mut state,
                json!({"sessionUpdate": "terminal_output_chunk", "terminalId": "x", "data": data}),
            );
        }
        assert_eq!(
            state.collections["terminal"]["x"]["output"]["data"],
            json!("YWJjZA==")
        );
    }

    #[test]
    fn state_update_replaces_the_previous_state() {
        let mut state = SessionState::default();
        apply(
            &mut state,
            json!({"sessionUpdate": "state_update", "state": "running", "background": []}),
        );
        apply(
            &mut state,
            json!({"sessionUpdate": "state_update", "state": "idle", "stopReason": "end_turn"}),
        );
        assert_eq!(
            state.values["state"],
            json!({"state": "idle", "stopReason": "end_turn"})
        );
    }

    #[test]
    fn a_plan_is_replaced_and_removed_by_its_id() {
        let mut state = SessionState::default();
        apply(
            &mut state,
            json!({"sessionUpdate": "plan_update", "plan": {"planId": "p", "type": "markdown", "content": "a"}}),
        );
        apply(
            &mut state,
            json!({"sessionUpdate": "plan_update", "plan": {"planId": "p", "type": "markdown", "content": "b"}}),
        );
        assert_eq!(state.collections["plan"]["p"]["content"], json!("b"));
        let changes = apply(
            &mut state,
            json!({"sessionUpdate": "plan_removed", "planId": "p"}),
        );
        assert_eq!(
            changes,
            vec![Change::Removed(ItemRef {
                target: "plan".into(),
                id: "p".into()
            })]
        );
        assert!(state.timeline.is_empty());
    }

    #[test]
    fn an_unknown_kind_is_kept_and_reported() {
        let mut state = SessionState::default();
        let changes = apply(
            &mut state,
            json!({"sessionUpdate": "x.vendor.checkpoint", "ref": "abc"}),
        );
        assert_eq!(changes, vec![Change::Unknown("x.vendor.checkpoint".into())]);
        assert_eq!(state.unknown.len(), 1);
    }

    #[test]
    fn a_chunk_without_an_id_is_an_error() {
        let mut state = SessionState::default();
        let error = state
            .apply_update(&RuleSet::pinned(), &json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "x"}}))
            .unwrap_err();
        assert_eq!(
            error,
            ApplyError::MissingId {
                kind: "agent_message_chunk".into(),
                path: "messageId".into()
            }
        );
    }

    #[test]
    fn a_request_is_pending_until_it_is_answered() {
        let rules = RuleSet::pinned();
        let mut conversation = Conversation::default();
        let request = json!({"jsonrpc": "2.0", "id": 7, "method": "session/request_permission", "params": {
            "sessionId": "s", "title": "rm -rf build", "options": [{"optionId": "y", "kind": "allow_once", "name": "Yes"}],
            "subject": {"type": "tool_call", "toolCall": {"toolCallId": "t1", "title": "rm -rf build"}}}});
        conversation
            .apply_message(&rules, Direction::In, &request)
            .unwrap();
        let session = &conversation.sessions["s"];
        assert_eq!(session.pending_requests().count(), 1);
        // The request's rule also records the tool call it is about.
        assert_eq!(
            session.collections["toolCall"]["t1"]["title"],
            json!("rm -rf build")
        );

        let answer = json!({"jsonrpc": "2.0", "id": 7, "result": {"outcome": {"outcome": "selected", "optionId": "y"}}});
        conversation
            .apply_message(&rules, Direction::Out, &answer)
            .unwrap();
        assert_eq!(conversation.sessions["s"].pending_requests().count(), 0);
    }
}

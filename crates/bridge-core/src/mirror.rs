//! What a client of the daemon holds for one session, and how the daemon's messages change it.
//!
//! This is the reference for client SDKs: ACP v2 messages go through the state rules; the
//! daemon's own additions (sequence numbers, answered requests) are handled here.

use crate::{
    json,
    state::{ApplyError, Change, Conversation, Direction, ItemRef, REQUEST_TARGET},
};
use bridge_schema::{ext, rules::RuleSet};
use serde_json::Value;

/// A client's copy of one session, including its subagents' child sessions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mirror {
    pub conversation: Conversation,
    /// The newest sequence number applied. Sent as `afterSeq` when attaching again.
    pub seq: u64,
}

impl Mirror {
    /// Forgets everything, before a full replay.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Notes a request this client sent, so the response can be applied when it arrives.
    ///
    /// # Errors
    /// When a state rule cannot be applied.
    pub fn sent(&mut self, rules: &RuleSet, message: &Value) -> Result<Vec<Change>, ApplyError> {
        if message.get("method").is_none() {
            // An answer to the agent. The daemon confirms it with `_bridge/request/resolved`,
            // because another client may have answered first.
            return Ok(Vec::new());
        }
        self.conversation
            .apply_message(rules, Direction::Out, message)
    }

    /// Applies a message from the daemon.
    ///
    /// # Errors
    /// When a state rule cannot be applied.
    pub fn received(
        &mut self,
        rules: &RuleSet,
        message: &Value,
    ) -> Result<Vec<Change>, ApplyError> {
        let params = message.get("params").unwrap_or(&Value::Null);
        if let Some(seq) = seq_of(message) {
            self.seq = self.seq.max(seq);
        }
        if params
            .pointer("/update/sessionUpdate")
            .and_then(Value::as_str)
            == Some("replay_complete")
        {
            // Marks the end of a replay. It says nothing about the session itself.
            return Ok(Vec::new());
        }
        let session_id = params.get("sessionId").and_then(Value::as_str);
        match (message.get("method").and_then(Value::as_str), session_id) {
            (Some(ext::NOTIFICATION_REQUEST_RESOLVED), Some(session_id)) => {
                let Some(id) = params.get("requestId").and_then(json::id_string) else {
                    return Ok(Vec::new());
                };
                let session = self.conversation.session_mut(session_id);
                if !session.requests.contains_key(&id)
                    && let (Some(method), Some(request)) = (
                        params.get("method").and_then(Value::as_str),
                        params.get("params"),
                    )
                {
                    session.apply_request(rules, &id, method, request)?;
                }
                let outcome = params.get("outcome").cloned().unwrap_or(Value::Null);
                session.resolve_request(&id, outcome);
                Ok(vec![Change::Item(ItemRef {
                    target: REQUEST_TARGET.to_owned(),
                    id,
                })])
            }
            (Some(method), Some(session_id)) if message.get("id").is_some() => {
                let Some(id) = message.get("id").and_then(json::id_string) else {
                    return Ok(Vec::new());
                };
                let session = self.conversation.session_mut(session_id);
                if session.requests.contains_key(&id) {
                    // Sent again after a reconnect so it can be answered on this connection.
                    return Ok(Vec::new());
                }
                // Keep the request as the agent sent it, without the daemon's position mark,
                // so that it is the same whether it was seen live or in a replay.
                let mut params = params.clone();
                if let Some(meta) = params.get_mut("_meta").and_then(Value::as_object_mut) {
                    meta.remove(ext::META_KEY);
                    if meta.is_empty()
                        && let Some(object) = params.as_object_mut()
                    {
                        object.remove("_meta");
                    }
                }
                session.apply_request(rules, &id, method, &params)
            }
            _ => self
                .conversation
                .apply_message(rules, Direction::In, message),
        }
    }
}

/// The log position the daemon put on a message, if any.
#[must_use]
pub fn seq_of(message: &Value) -> Option<u64> {
    message
        .pointer("/params/_meta/bridge/seq")
        .and_then(Value::as_u64)
}

/// The session whose log a message from the daemon belongs to.
///
/// For a subagent's messages this is the parent the user talks to, not the child session.
#[must_use]
pub fn root_of(message: &Value) -> Option<&str> {
    message
        .pointer("/params/_meta/bridge/rootSessionId")
        .or_else(|| message.pointer("/params/sessionId"))
        .and_then(Value::as_str)
}

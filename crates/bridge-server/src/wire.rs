//! The JSON-RPC messages the daemon builds itself.

use bridge_core::log::{LogEntry, Record};
use bridge_schema::ext;
use serde_json::{Map, Value, json};

pub(crate) const INVALID_PARAMS: i64 = -32602;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INTERNAL_ERROR: i64 = -32603;

pub(crate) fn request(id: impl Into<Value>, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id.into(), "method": method, "params": params})
}

pub(crate) fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

pub(crate) fn result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(crate) fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// A response with the same outcome as `response` (its `result` or its `error`), under another id.
pub(crate) fn relay(id: &Value, response: &Value) -> Value {
    let mut relayed = json!({"jsonrpc": "2.0", "id": id});
    match response.get("error") {
        Some(error) => relayed["error"] = error.clone(),
        None => relayed["result"] = response.get("result").cloned().unwrap_or(Value::Null),
    }
    relayed
}

/// `params` with `_meta.bridge` set to the entry's position in the log.
fn with_position(mut params: Value, root: &str, entry: &LogEntry) -> Value {
    let mut position = Map::new();
    position.insert("seq".to_owned(), entry.seq.into());
    if entry.session_id != root {
        position.insert("rootSessionId".to_owned(), root.into());
    }
    if let Some(object) = params.as_object_mut() {
        let meta = object.entry("_meta").or_insert_with(|| json!({}));
        if !meta.is_object() {
            *meta = json!({});
        }
        meta[ext::META_KEY] = Value::Object(position);
    }
    params
}

/// The message that carries a log entry to a client.
///
/// A request the user can still answer goes out as a real JSON-RPC request. One that was already
/// answered cannot be asked again, so it goes out as a `_bridge/request/resolved` notification
/// holding both the question and the answer.
pub(crate) fn entry(root: &str, entry: &LogEntry) -> Value {
    match &entry.record {
        Record::Update { update } => notification(
            "session/update",
            with_position(
                json!({"sessionId": entry.session_id, "update": update}),
                root,
                entry,
            ),
        ),
        Record::Request {
            request_id,
            method,
            params,
            outcome: None,
        } => request(
            request_id.as_str(),
            method,
            with_position(params.clone(), root, entry),
        ),
        Record::Request {
            request_id,
            method,
            params,
            outcome: Some(outcome),
        } => resolved(root, entry, request_id, method, Some(params), outcome, None),
        Record::Resolved {
            request_id,
            method,
            outcome,
            resolved_by,
        } => resolved(
            root,
            entry,
            request_id,
            method,
            None,
            outcome,
            resolved_by.as_deref(),
        ),
    }
}

fn resolved(
    root: &str,
    entry: &LogEntry,
    request_id: &str,
    method: &str,
    params: Option<&Value>,
    outcome: &Value,
    resolved_by: Option<&str>,
) -> Value {
    let mut body = json!({
        "sessionId": entry.session_id,
        "requestId": request_id,
        "method": method,
        "outcome": outcome,
    });
    if let Some(params) = params {
        body["params"] = params.clone();
    }
    if let Some(resolved_by) = resolved_by {
        body["resolvedBy"] = resolved_by.into();
    }
    notification(
        ext::NOTIFICATION_REQUEST_RESOLVED,
        with_position(body, root, entry),
    )
}

/// An update body the daemon writes itself, marked as such.
pub(crate) fn own_update(mut update: Value) -> Value {
    update["_meta"] = json!({ext::META_KEY: {"origin": "daemon"}});
    update
}

/// The answer given on the user's behalf when a request can no longer reach the agent.
pub(crate) fn cancelled(method: &str) -> Value {
    match method {
        "session/request_permission" => json!({"outcome": {"outcome": "cancelled"}}),
        "elicitation/create" => json!({"action": "cancel"}),
        _ => json!({"code": -32800, "message": "the request was cancelled"}),
    }
}

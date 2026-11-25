//! Small helpers for dotted paths in JSON values.

use serde_json::{Map, Value};

/// Reads the value at a dotted path such as `subject.toolCall.toolCallId`.
pub(crate) fn get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, key| current.get(key))
}

/// Returns the slot at a dotted path, creating objects along the way.
pub(crate) fn slot<'a>(object: &'a mut Map<String, Value>, path: &str) -> &'a mut Value {
    let mut parts = path.split('.');
    let first = parts.next().unwrap_or_default();
    let mut current = object.entry(first.to_owned()).or_insert(Value::Null);
    for key in parts {
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        current = current
            .as_object_mut()
            .expect("just made an object")
            .entry(key.to_owned())
            .or_insert(Value::Null);
    }
    current
}

/// The last segment of a dotted path.
pub(crate) fn leaf(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// An id as a string, whether it is sent as a string or a number.
pub(crate) fn id_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

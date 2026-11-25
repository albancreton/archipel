//! A plain-text view of a conversation. It exists so that a person can read what the reducer
//! produced and compare it with what the harness showed; apps render items their own way.

use crate::state::{Conversation, ItemRef, REQUEST_TARGET, Request, SessionState};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::Value;
use std::fmt::Write as _;

const CLIP: usize = 200;

/// Renders the root session, with subagent sessions nested under the item that started them.
#[must_use]
pub fn conversation(conversation: &Conversation) -> String {
    let mut out = String::new();
    match conversation.root() {
        Some((id, session)) => {
            let mut seen = vec![id.clone()];
            write_session(&mut out, conversation, session, 0, &mut seen);
            for (other_id, other) in &conversation.sessions {
                if !seen.contains(other_id) {
                    let _ = writeln!(
                        out,
                        "\n(session {} not linked to a subagent item)",
                        short(other_id)
                    );
                    seen.push(other_id.clone());
                    write_session(&mut out, conversation, other, 1, &mut seen);
                }
            }
        }
        None => out.push_str("(no session)\n"),
    }
    out
}

fn write_session(
    out: &mut String,
    all: &Conversation,
    session: &SessionState,
    depth: usize,
    seen: &mut Vec<String>,
) {
    let pad = "    ".repeat(depth);
    let inline_terminals: Vec<&str> = session
        .collections
        .get("toolCall")
        .into_iter()
        .flat_map(|tools| tools.values())
        .flat_map(|tool| {
            tool.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|content| content.get("terminalId").and_then(Value::as_str))
        .collect();

    for item in &session.timeline {
        if item.target == REQUEST_TARGET {
            if let Some(request) = session.requests.get(&item.id) {
                let _ = writeln!(out, "{pad}{}", request_line(request));
            }
            continue;
        }
        let Some(entry) = session.item(item) else {
            continue;
        };
        match item.target.as_str() {
            "userMessage" => line(out, &pad, "user", &content_text(entry.get("content"))),
            "agentMessage" => line(out, &pad, "assistant", &content_text(entry.get("content"))),
            "agentThought" => line(out, &pad, "thought", &content_text(entry.get("content"))),
            "toolCall" => write_tool(out, &pad, session, entry),
            "terminal" if inline_terminals.contains(&item.id.as_str()) => {}
            "terminal" => write_terminal(out, &pad, entry),
            "plan" => {
                let kind = text(entry, "type");
                let body = match entry.get("entries").and_then(Value::as_array) {
                    Some(entries) => entries
                        .iter()
                        .map(|e| format!("[{}] {}", text(e, "status"), text(e, "content")))
                        .collect::<Vec<_>>()
                        .join("; "),
                    None => text(entry, "content").to_owned(),
                };
                line(out, &pad, &format!("plan {kind}"), &body);
            }
            "compaction" => line(
                out,
                &pad,
                &format!("compaction {}", text(entry, "status")),
                &content_text(entry.get("summary")),
            ),
            "subagent" => {
                let state = entry
                    .pointer("/state/state")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                line(
                    out,
                    &pad,
                    &format!("subagent {state}"),
                    text(entry, "title"),
                );
                if let Some(child) = all.sessions.get(&item.id)
                    && !seen.contains(&item.id)
                {
                    seen.push(item.id.clone());
                    write_session(out, all, child, depth + 1, seen);
                }
            }
            "sessionMessage" => {
                let label = format!(
                    "message {} -> {}",
                    short(text(entry, "senderSessionId")),
                    short(text(entry, "recipientSessionId"))
                );
                line(out, &pad, &label, &content_text(entry.get("content")));
            }
            // A cursor marks a place in the history; it is not content.
            "cursor" => {}
            other => line(out, &pad, other, &clip(&entry.to_string())),
        }
    }
    write_footer(out, &pad, session);
}

fn write_tool(out: &mut String, pad: &str, session: &SessionState, tool: &Value) {
    let label = format!("tool {} {}", text(tool, "kind"), text(tool, "status"));
    line(out, pad, label.trim_end(), text(tool, "title"));
    for content in tool
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match content.get("type").and_then(Value::as_str) {
            Some("terminal") => {
                let id = text(content, "terminalId");
                if let Some(terminal) = session.item(&ItemRef {
                    target: "terminal".into(),
                    id: id.into(),
                }) {
                    write_terminal(out, &format!("{pad}    "), terminal);
                }
            }
            Some("diff") => {
                let patch = content
                    .pointer("/patch/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let count = |sign: char, header: &str| {
                    patch
                        .lines()
                        .filter(|l| l.starts_with(sign) && !l.starts_with(header))
                        .count()
                };
                for change in content
                    .get("changes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let _ = writeln!(
                        out,
                        "{pad}    diff {} {}",
                        text(change, "operation"),
                        text(change, "path")
                    );
                }
                let _ = writeln!(
                    out,
                    "{pad}    +{} -{} lines",
                    count('+', "+++"),
                    count('-', "---")
                );
            }
            Some("content") => {
                let body = content
                    .get("content")
                    .map(|block| content_text(Some(&Value::Array(vec![block.clone()]))));
                let _ = writeln!(out, "{pad}    = {}", clip(&body.unwrap_or_default()));
            }
            Some(other) => {
                let _ = writeln!(out, "{pad}    <{other}>");
            }
            None => {}
        }
    }
}

fn write_terminal(out: &mut String, pad: &str, terminal: &Value) {
    let exit = match terminal.get("exitStatus") {
        Some(status) => match status.get("exitCode").and_then(Value::as_i64) {
            Some(code) => format!("exit {code}"),
            None => "ended".to_owned(),
        },
        None => "running".to_owned(),
    };
    let _ = writeln!(out, "{pad}$ {}   ({exit})", clip(text(terminal, "command")));
    let data = terminal
        .pointer("/output/data")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let decoded = BASE64
        .decode(data)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    for output in decoded.lines().filter(|l| !l.trim().is_empty()).take(12) {
        let _ = writeln!(out, "{pad}| {}", clip(output));
    }
}

fn request_line(request: &Request) -> String {
    let (label, subject) = match request.method.as_str() {
        "session/request_permission" => ("approval", text(&request.params, "title")),
        "elicitation/create" => ("question", text(&request.params, "message")),
        other => (other, ""),
    };
    let answer = match &request.outcome {
        None => "PENDING".to_owned(),
        Some(outcome) => match request.method.as_str() {
            "session/request_permission" => {
                let chosen = outcome.pointer("/outcome/optionId").and_then(Value::as_str);
                let option = request
                    .params
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|option| {
                        option.get("optionId").and_then(Value::as_str) == chosen && chosen.is_some()
                    });
                match option {
                    Some(option) => format!("{} ({})", text(option, "kind"), text(option, "name")),
                    None => outcome
                        .pointer("/outcome/outcome")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_owned(),
                }
            }
            _ => format!(
                "{} {}",
                text(outcome, "action"),
                outcome
                    .get("content")
                    .map(Value::to_string)
                    .unwrap_or_default()
            ),
        },
    };
    format!("[{label}] {} -> {}", clip(subject), clip(answer.trim_end()))
}

fn write_footer(out: &mut String, pad: &str, session: &SessionState) {
    if let Some(state) = session.values.get("state") {
        let background = state
            .get("background")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let _ = writeln!(
            out,
            "{pad}-- state: {} {} background={background}",
            text(state, "state"),
            text(state, "stopReason")
        );
    }
    if let Some(title) = session
        .values
        .get("sessionInfo")
        .map(|info| text(info, "title"))
        .filter(|t| !t.is_empty())
    {
        let _ = writeln!(out, "{pad}-- title: {title}");
    }
    let pending = session.pending_requests().count();
    if pending > 0 {
        let _ = writeln!(out, "{pad}-- pending requests: {pending}");
    }
    if !session.events.is_empty() {
        let kinds: Vec<&str> = session
            .events
            .iter()
            .map(|event| text(event, "sessionUpdate"))
            .collect();
        let _ = writeln!(out, "{pad}-- events: {}", kinds.join(", "));
    }
    if !session.unknown.is_empty() {
        let kinds: Vec<&str> = session
            .unknown
            .iter()
            .map(|update| text(update, "sessionUpdate"))
            .collect();
        let _ = writeln!(out, "{pad}-- unknown updates: {}", kinds.join(", "));
    }
}

fn line(out: &mut String, pad: &str, label: &str, body: &str) {
    let _ = writeln!(out, "{pad}[{label}] {}", clip(body));
}

/// The text of a content block list. Other block types are shown by their type name.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|block| match block.get("text").and_then(Value::as_str) {
                Some(text) => text.to_owned(),
                None => format!("<{}>", text(block, "type")),
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn short(id: &str) -> &str {
    id.char_indices()
        .rev()
        .nth(5)
        .map_or(id, |(start, _)| &id[start..])
}

/// One line, at most [`CLIP`] characters, with the number of characters left out.
fn clip(text: &str) -> String {
    let flat = text.replace('\n', "⏎");
    let total = flat.chars().count();
    if total <= CLIP {
        return flat;
    }
    let kept: String = flat.chars().take(CLIP).collect();
    format!("{kept}… (+{} chars)", total - CLIP)
}

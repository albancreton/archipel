//! A stand-in adapter that replays a recorded session.
//!
//! It walks the transcript in order. Where the recording has a message from the client, it waits
//! for the daemon to send one of the same kind; where it has a message from the agent, it sends
//! it. So the daemon sees what the real adapter sent, at no cost.
//!
//! Like a real harness, it also knows its session from before: `session/list` reports the
//! recorded session, and `session/resume` with `replayFrom` sends the recorded updates again.

use bridge_core::{state::Direction, transcript::Transcript};
use bridge_transport::Link;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

/// Replays `transcript` over `link` until the recording ends and the daemon goes away.
///
/// `speed` scales the recorded pauses between the agent's messages: 1.0 is the recorded pace,
/// 0.0 sends everything at once.
pub async fn replay(transcript: Transcript, speed: f64, link: Link) {
    let mut fake = Fake { transcript, link };
    // Recorded request id -> the id the daemon really used.
    let mut ids: HashMap<String, Value> = HashMap::new();
    let mut last_sent_at: Option<u64> = None;

    for at in 0..fake.transcript.frames.len() {
        let frame = fake.transcript.frames[at].clone();
        let method = frame.m.get("method").and_then(Value::as_str);
        let id = frame.m.get("id");
        match frame.dir {
            Direction::Out => {
                last_sent_at = None;
                let Some(received) = fake.wait_for(method, id.is_some()).await else {
                    return;
                };
                if let (Some(_), Some(recorded), Some(actual)) = (method, id, received.get("id")) {
                    ids.insert(recorded.to_string(), actual.clone());
                }
            }
            Direction::In => {
                if speed > 0.0
                    && let Some(previous) = last_sent_at
                {
                    let pause = frame.t.saturating_sub(previous) as f64 / speed;
                    tokio::time::sleep(Duration::from_millis(pause as u64)).await;
                }
                last_sent_at = Some(frame.t);
                let mut message = frame.m.clone();
                if method.is_none()
                    && let Some(actual) = id.and_then(|recorded| ids.get(&recorded.to_string()))
                {
                    message["id"] = actual.clone();
                }
                if fake.link.tx.send(message).await.is_err() {
                    return;
                }
            }
        }
    }
    // The recording is over. Stay up so the session stays live, and refuse further work.
    while fake.wait_for(Some(""), true).await.is_some() {}
}

struct Fake {
    transcript: Transcript,
    link: Link,
}

impl Fake {
    /// Waits for the daemon's next message of the expected kind and returns it.
    ///
    /// Requests of another kind are answered here: listing and resuming work as described in
    /// the module text, anything else fails. Returns `None` when the daemon is gone.
    async fn wait_for(&mut self, method: Option<&str>, has_id: bool) -> Option<Value> {
        loop {
            let message = self.link.rx.recv().await?;
            let got_method = message.get("method").and_then(Value::as_str);
            if got_method == method && message.get("id").is_some() == has_id {
                return Some(message);
            }
            let (Some(got), Some(id)) = (got_method, message.get("id")) else {
                continue;
            };
            let result = match got {
                "session/list" => Some(json!({"sessions": self.recorded_session()})),
                "session/resume" => {
                    if message["params"].get("replayFrom").is_some() {
                        for update in self.recorded_updates() {
                            self.link.tx.send(update).await.ok()?;
                        }
                    }
                    Some(self.setup().unwrap_or_else(|| json!({})))
                }
                _ => None,
            };
            let reply = match result {
                Some(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                None => json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32603,
                    "message": format!("the recorded session has no `{got}` at this point"),
                }}),
            };
            self.link.tx.send(reply).await.ok()?;
        }
    }

    /// The result of the recorded `session/new`.
    fn setup(&self) -> Option<Value> {
        self.transcript.frames.iter().find_map(|frame| {
            let result = frame.m.get("result")?;
            result.get("sessionId").map(|_| result.clone())
        })
    }

    /// The recorded session as `session/list` reports it.
    fn recorded_session(&self) -> Vec<Value> {
        let cwd = self.transcript.frames.iter().find_map(|frame| {
            (frame.m.get("method")?.as_str()? == "session/new")
                .then(|| frame.m.pointer("/params/cwd").cloned())?
        });
        let title = self.transcript.updates().find_map(|(_, update)| {
            (update.get("sessionUpdate")?.as_str()? == "session_info_update")
                .then(|| update.get("title").cloned())?
        });
        match (self.setup(), cwd) {
            (Some(setup), Some(cwd)) => vec![json!({
                "sessionId": setup["sessionId"],
                "cwd": cwd,
                "title": title,
                "updatedAt": "2026-11-23T00:00:00.000Z",
            })],
            _ => Vec::new(),
        }
    }

    /// Every recorded update again, then the marker that ends a replay.
    fn recorded_updates(&self) -> Vec<Value> {
        let session = self.setup().map(|setup| setup["sessionId"].clone());
        let mut updates: Vec<Value> = self
            .transcript
            .frames
            .iter()
            .filter(|frame| {
                frame.dir == Direction::In
                    && frame.m.get("method").and_then(Value::as_str) == Some("session/update")
            })
            .map(|frame| frame.m.clone())
            .collect();
        updates.push(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": session,
                "update": {"sessionUpdate": "replay_complete"},
            }}),
        );
        updates
    }
}

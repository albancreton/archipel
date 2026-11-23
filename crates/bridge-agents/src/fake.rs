//! A stand-in adapter that replays a recorded session.
//!
//! It walks the transcript in order. Where the recording has a message from the client, it waits
//! for the daemon to send one of the same kind; where it has a message from the agent, it sends
//! it. So the daemon sees what the real adapter sent, at no cost.

use bridge_core::{state::Direction, transcript::Transcript};
use bridge_transport::Link;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

/// Replays `transcript` over `link` until the recording ends and the daemon goes away.
///
/// `speed` scales the recorded pauses between the agent's messages: 1.0 is the recorded pace,
/// 0.0 sends everything at once.
pub async fn replay(transcript: Transcript, speed: f64, mut link: Link) {
    // Recorded request id -> the id the daemon really used.
    let mut ids: HashMap<String, Value> = HashMap::new();
    let mut last_sent_at: Option<u64> = None;

    for frame in &transcript.frames {
        let method = frame.m.get("method").and_then(Value::as_str);
        let id = frame.m.get("id");
        match frame.dir {
            Direction::Out => {
                last_sent_at = None;
                let Some(received) = wait_for(&mut link, method, id.is_some()).await else {
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
                if link.tx.send(message).await.is_err() {
                    return;
                }
            }
        }
    }
    // The recording is over. Stay up so the session stays live, and refuse further work.
    while wait_for(&mut link, Some(""), true).await.is_some() {}
}

/// Waits for the daemon's next message of the expected kind and returns it.
///
/// Requests of another kind are answered here: `session/resume` succeeds, anything else fails.
/// Returns `None` when the daemon is gone.
async fn wait_for(link: &mut Link, method: Option<&str>, has_id: bool) -> Option<Value> {
    loop {
        let message = link.rx.recv().await?;
        let got_method = message.get("method").and_then(Value::as_str);
        if got_method == method && message.get("id").is_some() == has_id {
            return Some(message);
        }
        if let (Some(got), Some(id)) = (got_method, message.get("id")) {
            let reply = if got == "session/resume" {
                json!({"jsonrpc": "2.0", "id": id, "result": {}})
            } else {
                json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32603,
                    "message": format!("the recorded session has no `{got}` at this point"),
                }})
            };
            link.tx.send(reply).await.ok()?;
        }
    }
}

//! A toy extension, to show what an extension is: a separate program that connects to the
//! daemon like an app, adds its own items to sessions, and serves its own methods.
//!
//! This one marks the end of every turn with a "checkpoint" item and can "restore" one. It
//! does not touch any files. A real one would record a git ref and reset to it; the daemon
//! would not need to change, or to know what git is.
//!
//! `bridge-demo-checkpoints [--home DIR | --socket PATH | --ws URL --token TOKEN]`

use bridge_client::{Connection, Endpoint};
use bridge_schema::ext;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// The namespace: its items are `x.demo.…`, its methods `_x/demo/…`.
const NAMESPACE: &str = "demo";
const CHECKPOINT: &str = "x.demo.checkpoint";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let endpoint = Endpoint::from_args(&mut args)?;
    let (mut daemon, _) = Connection::open(&endpoint, "checkpoint-extension").await?;
    daemon
        .call(
            ext::METHOD_EXTENSION_REGISTER,
            json!({"namespace": NAMESPACE}),
        )
        .await?;
    eprintln!("checkpoints: registered as `{NAMESPACE}`");

    // Sessions with a turn under way, and the checkpoints made so far, by session.
    let mut running: HashSet<String> = HashSet::new();
    let mut made: HashMap<String, Vec<Value>> = HashMap::new();

    while let Some(message) = daemon.next().await {
        let params = &message["params"];
        match (message["method"].as_str(), message.get("id")) {
            // The daemon tells every connection when a session's summary changes.
            (Some(ext::NOTIFICATION_SESSIONS_UPDATED), None) => {
                let session = &params["session"];
                let Some(id) = session["sessionId"].as_str() else {
                    continue;
                };
                if session["state"] != json!("idle") {
                    running.insert(id.to_owned());
                } else if running.remove(id) {
                    // A turn just ended: add a checkpoint item to the session.
                    let list = made.entry(id.to_owned()).or_default();
                    let turn = list.len() + 1;
                    let checkpoint = json!({
                        "sessionUpdate": CHECKPOINT,
                        "checkpointId": format!("{}-{turn}", &id[..id.len().min(8)]),
                        "label": format!("after turn {turn}"),
                    });
                    list.push(checkpoint.clone());
                    append(&mut daemon, id, checkpoint).await?;
                }
            }
            // An app calls one of this extension's methods; the daemon passes it on.
            (Some("_x/demo/checkpoints/list"), Some(id)) => {
                let list = params["sessionId"].as_str().and_then(|s| made.get(s));
                daemon.answer(id, json!({ "checkpoints": list })).await?;
            }
            (Some("_x/demo/checkpoint/restore"), Some(id)) => {
                let wanted = &params["checkpointId"];
                let session = params["sessionId"].as_str().unwrap_or_default();
                let known = made
                    .get(session)
                    .is_some_and(|list| list.iter().any(|c| c["checkpointId"] == *wanted));
                if known {
                    // Update the item: same id, one more field.
                    let update = json!({"sessionUpdate": CHECKPOINT, "checkpointId": wanted, "restored": true});
                    append(&mut daemon, session, update).await?;
                    daemon.answer(id, json!({ "restored": wanted })).await?;
                } else {
                    daemon.refuse(id, "no such checkpoint").await?;
                }
            }
            (Some(_), Some(id)) => daemon.refuse(id, "not a method of this extension").await?,
            _ => {}
        }
    }
    Ok(())
}

/// Adds an update to a session. The rule says: it is an item, identified by `checkpointId`,
/// and a later update with the same id changes that item.
async fn append(
    daemon: &mut Connection,
    session: &str,
    update: Value,
) -> Result<(), bridge_client::Error> {
    let params = json!({
        "sessionId": session,
        "update": update,
        "state": {"action": "upsert", "id": "checkpointId"},
    });
    daemon
        .call(ext::METHOD_SESSION_APPEND, params)
        .await
        .map(drop)
}

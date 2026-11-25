//! A daemon whose adapter is scripted by the test, and an app that is a bare connection.
#![allow(dead_code)]

use archipel_agents::{AgentSpec, Launcher, Running};
use archipel_transport::Link;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

/// A launcher whose adapter side is handed to the test.
pub struct Scripted(pub mpsc::UnboundedSender<Link>);

impl Launcher for Scripted {
    fn launch(&self) -> std::io::Result<Running> {
        let (ours, theirs) = Link::pair();
        self.0.send(theirs).map_err(std::io::Error::other)?;
        Ok(Running {
            link: ours,
            guard: None,
        })
    }
}

pub async fn next(link: &mut Link) -> Value {
    tokio::time::timeout(Duration::from_secs(10), link.rx.recv())
        .await
        .expect("a message within 10 seconds")
        .expect("the connection is open")
}

/// Reads until the response to request `id`. Returns it and the notifications before it.
pub async fn response(app: &mut Link, id: u64) -> (Value, Vec<Value>) {
    let mut before = Vec::new();
    loop {
        let message = next(app).await;
        if message.get("method").is_none() && message["id"] == json!(id) {
            return (message, before);
        }
        before.push(message);
    }
}

pub async fn ask(app: &Link, id: u64, method: &str, params: Value) {
    let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    app.tx.send(message).await.unwrap();
}

pub async fn answer(adapter: &Link, request: &Value, result: Value) {
    let message = json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
    adapter.tx.send(message).await.unwrap();
}

pub async fn update(adapter: &Link, session: &str, update: Value) {
    let message = json!({"jsonrpc": "2.0", "method": "session/update",
        "params": {"sessionId": session, "update": update}});
    adapter.tx.send(message).await.unwrap();
}

/// Answers the daemon's `initialize` as the adapter `demo-acp` 1.2.3.
pub async fn started(adapters: &mut mpsc::UnboundedReceiver<Link>) -> Link {
    let mut adapter = adapters.recv().await.unwrap();
    let asked = next(&mut adapter).await;
    assert_eq!(asked["method"], "initialize");
    let result = json!({"protocolVersion": 2, "capabilities": {}, "info": {"name": "demo-acp", "version": "1.2.3"}});
    answer(&adapter, &asked, result).await;
    adapter
}

pub fn agent(id: &str, harness: Option<&str>, launched: mpsc::UnboundedSender<Link>) -> AgentSpec {
    AgentSpec {
        id: id.to_owned(),
        name: id.to_owned(),
        harness: harness.map(str::to_owned),
        launcher: Arc::new(Scripted(launched)),
    }
}

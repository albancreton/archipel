//! Addons see the messages between the daemon and an adapter, and may change or stop them.
//!
//! The adapter here is scripted by the test, so each check reads exactly what the adapter
//! received and what the app received.

mod common;

use bridge_server::{
    Addon, Server,
    addon::{Agent, Info, Keep, Kind, Request, Update, Verdict},
};
use bridge_transport::Link;
use common::{agent, answer, ask, next, response, started, update};
use serde_json::json;
use tokio::sync::mpsc;

/// Uses every hook, for agents of the harness `demo` only.
struct Probe;

impl Addon for Probe {
    fn info(&self) -> Info {
        Info {
            id: "test.probe",
            kind: Kind::Fix,
            summary: "",
            because: "",
            until: "",
            scenario: None,
        }
    }

    fn applies(&self, agent: &Agent<'_>) -> bool {
        agent.harness == Some("demo")
    }

    fn request(&mut self, request: &mut Request<'_>) -> Verdict {
        match request.method {
            "session/new" => {
                request.params["marked"] = json!({
                    "adapter": request.agent.adapter,
                    "version": request.agent.version,
                    "os": request.agent.os,
                });
            }
            "session/resume" => {
                let logged = request
                    .history
                    .and_then(|history| history.last_update("config_option_update"));
                request.params["logged"] =
                    json!(logged.map(|update| update["configOptions"].clone()));
            }
            "session/prompt" if request.params["prompt"][0]["text"] == "forbidden" => {
                return Verdict::Refuse {
                    code: -32000,
                    message: "not this one".to_owned(),
                };
            }
            _ => {}
        }
        Verdict::Pass
    }

    fn update(&mut self, update: &mut Update<'_>) -> Keep {
        match update.update["sessionUpdate"].as_str() {
            Some("noise") => Keep::No,
            Some("agent_message_chunk") => {
                update.update["content"]["text"] = json!("corrected");
                Keep::Yes
            }
            _ => Keep::Yes,
        }
    }
}

#[tokio::test]
async fn an_addon_changes_and_stops_messages_in_both_directions() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("demo", Some("demo"), launched))
        .addon(Probe)
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);

    // A request is changed on its way to the adapter, knowing which adapter it is.
    ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(&mut adapters).await;
    let new = next(&mut adapter).await;
    assert_eq!(
        new["params"],
        json!({"cwd": "/work", "marked": {"adapter": "demo-acp", "version": "1.2.3", "os": std::env::consts::OS}})
    );
    let options =
        json!([{"configId": "model", "type": "select", "currentValue": "small", "options": []}]);
    answer(
        &adapter,
        &new,
        json!({"sessionId": "s1", "configOptions": options}),
    )
    .await;
    response(&mut app, 1).await;

    // A refused request never reaches the adapter. The app gets the addon's error, marked.
    let prompt =
        |text: &str| json!({"sessionId": "s1", "prompt": [{"type": "text", "text": text}]});
    ask(&app, 2, "session/prompt", prompt("forbidden")).await;
    let (refused, _) = response(&mut app, 2).await;
    assert_eq!(
        refused["error"],
        json!({"code": -32000, "message": "not this one", "data": {"addon": "test.probe"}})
    );

    // An update is changed, and another dropped, before the log and the apps see them.
    ask(&app, 3, "session/prompt", prompt("allowed")).await;
    let asked = next(&mut adapter).await;
    assert_eq!(
        asked["params"],
        prompt("allowed"),
        "the refused one was not sent"
    );
    update(&adapter, "s1", json!({"sessionUpdate": "noise"})).await;
    let chunk = json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "wrong"}});
    update(&adapter, "s1", chunk).await;
    answer(&adapter, &asked, json!({})).await;
    let (_, before) = response(&mut app, 3).await;
    let kinds: Vec<&str> = before
        .iter()
        .filter_map(|message| message["params"]["update"]["sessionUpdate"].as_str())
        .filter(|kind| *kind != "config_option_update")
        .collect();
    assert_eq!(kinds, ["agent_message_chunk"], "the noise was dropped");
    let chunk = before
        .iter()
        .find(|message| message["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
        .unwrap();
    assert_eq!(chunk["params"]["update"]["content"]["text"], "corrected");
    // The log holds the changed update too: a second app that replays the session reads it.
    let (mut late, theirs) = Link::pair();
    server.connect(theirs);
    ask(
        &late,
        1,
        "_bridge/session/attach",
        json!({"sessionId": "s1"}),
    )
    .await;
    response(&mut late, 1).await;
    let mut replayed = Vec::new();
    loop {
        let message = next(&mut late).await;
        let kind = message["params"]["update"]["sessionUpdate"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if kind == "replay_complete" {
            break;
        }
        if kind == "agent_message_chunk" {
            replayed.push(message["params"]["update"]["content"]["text"].clone());
        }
        assert_ne!(kind, "noise");
    }
    assert_eq!(replayed, [json!("corrected")]);

    // The daemon's own requests pass the addons too, with the session's log to read: here the
    // `session/resume` that wakes the session after its adapter died.
    drop(adapter);
    ask(&app, 4, "session/prompt", prompt("again")).await;
    let mut adapter = started(&mut adapters).await;
    let resume = next(&mut adapter).await;
    assert_eq!(resume["method"], "session/resume");
    assert_eq!(resume["params"]["logged"], options);

    server.shutdown().await;
}

#[tokio::test]
async fn an_addon_is_only_shown_the_agents_it_applies_to() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("plain", None, launched))
        .addon(Probe)
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);

    ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(&mut adapters).await;
    let new = next(&mut adapter).await;
    assert_eq!(new["params"], json!({"cwd": "/work"}), "unchanged");
    answer(&adapter, &new, json!({"sessionId": "s1"})).await;
    response(&mut app, 1).await;
    ask(
        &app,
        2,
        "session/prompt",
        json!({"sessionId": "s1", "prompt": [{"type": "text", "text": "forbidden"}]}),
    )
    .await;
    assert_eq!(
        next(&mut adapter).await["method"],
        "session/prompt",
        "not refused"
    );

    server.shutdown().await;
}

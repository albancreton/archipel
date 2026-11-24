//! Addons also sit between an app and the daemon: they see every request an app makes
//! before the daemon handles it, the daemon's answer, and everything on its way to one app.
//!
//! As in `addons.rs` the adapter is scripted by the test. One addon, [`Door`], uses the
//! app-side hooks; what it does depends on a word in the message.

mod common;

use bridge_server::{
    Addon, Handle, Scope, Server,
    addon::{
        AppAnswer, AppNotification, AppRequest, AppResponse, Cx, Flow, Info, Keep, Kind, Outcome,
        Request, ToApp, Verdict,
    },
};
use bridge_transport::Link;
use common::{agent, answer, ask, next, response, started, update};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

/// What the doors did, in order: `<name> <hook> <method>`.
type Trace = Arc<Mutex<Vec<String>>>;

struct Door {
    name: &'static str,
    trace: Trace,
    /// The message it holds, if any. Released when an app sends `door/release`.
    held: Option<u64>,
    /// Its own requests of the daemon, each with the app request that waits for it.
    calls: HashMap<u64, u64>,
    /// The first app it saw.
    first: Option<u64>,
}

impl Door {
    fn new(name: &'static str, trace: &Trace) -> Self {
        Self {
            name,
            trace: trace.clone(),
            held: None,
            calls: HashMap::new(),
            first: None,
        }
    }

    fn note(&self, what: &str) {
        self.trace
            .lock()
            .unwrap()
            .push(format!("{} {what}", self.name));
    }

    fn told(&self, value: &Value, word: &str) -> bool {
        value.as_str() == Some(&format!("{}:{word}", self.name))
    }
}

impl Addon for Door {
    fn info(&self) -> Info {
        Info {
            id: if self.name == "d1" {
                "test.d1"
            } else {
                "test.d2"
            },
            kind: Kind::Feature,
            summary: "",
            because: "",
            until: "",
            scenario: None,
        }
    }

    fn app_request(&mut self, cx: &mut Cx<'_>, request: &mut AppRequest<'_>) -> Verdict {
        self.note(&format!("app_request {}", request.method));
        self.first.get_or_insert(request.app.id);
        let word = request.params["to"].clone();
        if self.told(&word, "answer") {
            return Verdict::Answer(json!({"by": self.name, "scope": request.app.scope}));
        } else if self.told(&word, "refuse") {
            return Verdict::Refuse {
                code: -32000,
                message: "no".to_owned(),
            };
        } else if self.told(&word, "hold") {
            self.held = Some(request.id);
            return Verdict::Hold;
        } else if self.told(&word, "whisper") {
            cx.tell(request.app.id, "door/psst", json!({}));
            return Verdict::Answer(json!({}));
        } else if self.told(&word, "new") {
            // The daemon makes the session, as it would for an app. The app waits meanwhile.
            let call = cx.app_call("session/new", json!({"cwd": "/made"}));
            self.calls.insert(call, request.id);
            return Verdict::Hold;
        }
        if self.told(&request.params["prompt"][0]["text"], "rewrite") {
            request.params["prompt"][0]["text"] = json!("rewritten");
            request.params["agent"] = json!(request.agent.map(|agent| agent.id));
        }
        Verdict::Pass
    }

    fn app_response(&mut self, _cx: &mut Cx<'_>, response: &mut AppResponse<'_>) -> Flow {
        self.note(&format!("app_response {}", response.method));
        if self.told(&response.params["to"], "hold-response") {
            self.held = Some(response.id);
            return Flow::Hold;
        }
        if let Outcome::Result(result) = response.outcome {
            match response.method {
                "initialize" => result["capabilities"][self.name] = json!(true),
                // What an app is told can depend on the app.
                "_bridge/sessions/list" if response.app.scope == "observe" => {
                    result["sessions"] = json!([]);
                }
                _ => {}
            }
        }
        Flow::Pass
    }

    fn app_notification(
        &mut self,
        cx: &mut Cx<'_>,
        notification: &mut AppNotification<'_>,
    ) -> Keep {
        self.note(&format!("app_notification {}", notification.method));
        if notification.method == "door/release" {
            if self.told(&notification.params["to"], "me")
                && let Some(held) = self.held.take()
            {
                let content = notification.params["content"].clone();
                match notification.params["how"].as_str() {
                    Some("with") => cx.release_with(held, content),
                    Some("answer") => cx.release(held, Verdict::Answer(content)),
                    Some("discard") => cx.discard(held),
                    _ => cx.release(held, Verdict::Pass),
                }
            }
            return Keep::No; // the daemon never sees it
        }
        if self.told(&notification.params["word"], "block") {
            return Keep::No;
        }
        Keep::Yes
    }

    fn app_answer(&mut self, _cx: &mut Cx<'_>, answer: &mut AppAnswer<'_>) -> Keep {
        self.note("app_answer");
        if let Outcome::Result(result) = answer.outcome {
            let option = result["outcome"]["optionId"].clone();
            if self.told(&option, "veto") {
                return Keep::No;
            } else if self.told(&option, "swap") {
                result["outcome"]["optionId"] = json!("swapped");
            }
        }
        Keep::Yes
    }

    fn to_app(&mut self, _cx: &mut Cx<'_>, message: &mut ToApp<'_>) -> Keep {
        if message.method == "door/psst" {
            message.params[self.name] = json!(true);
        }
        // A request is shown to the first app only, if its title says so.
        if message.request.is_some()
            && self.told(&message.params["title"], "first-only")
            && self.first != Some(message.app.id)
        {
            return Keep::No;
        }
        let Some(update) = message.params.get_mut("update") else {
            return Keep::Yes;
        };
        match update["sessionUpdate"].as_str() {
            Some("secret") if message.app.scope == "observe" => return Keep::No,
            Some("slow") => {
                self.held = Some(message.id);
                return Keep::Hold;
            }
            Some("agent_message_chunk") => {
                let text = update["content"]["text"].as_str().unwrap_or_default();
                update["content"]["text"] = json!(format!("{text}~{}", self.name));
            }
            _ => {}
        }
        Keep::Yes
    }

    fn request(&mut self, _cx: &mut Cx<'_>, request: &mut Request<'_>) -> Verdict {
        self.note(&format!(
            "request {} from {:?}",
            request.method, request.from
        ));
        Verdict::Pass
    }

    fn reply(&mut self, cx: &mut Cx<'_>, id: u64, outcome: &Outcome) {
        if let (Some(waiting), Outcome::Result(result)) = (self.calls.remove(&id), outcome) {
            cx.release(waiting, Verdict::Answer(result.clone()));
        }
    }
}

/// A daemon with one `demo` agent, the given doors, an app, and a session `s1` on a started
/// adapter.
struct Setup {
    server: Handle,
    app: Link,
    adapter: Link,
    trace: Trace,
}

impl Setup {
    async fn with(doors: &[&'static str]) -> Self {
        let trace = Trace::default();
        let (launched, mut adapters) = mpsc::unbounded_channel();
        let mut builder = Server::builder().agent(agent("demo", Some("demo"), launched));
        for name in doors {
            builder = builder.addon(Door::new(name, &trace));
        }
        let server = builder.start();
        let (mut app, theirs) = Link::pair();
        server.connect(theirs);
        ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
        let mut adapter = started(&mut adapters).await;
        let new = next(&mut adapter).await;
        answer(&adapter, &new, json!({"sessionId": "s1"})).await;
        response(&mut app, 1).await;
        trace.lock().unwrap().clear();
        Self {
            server,
            app,
            adapter,
            trace,
        }
    }

    fn traced(&self) -> Vec<String> {
        std::mem::take(&mut *self.trace.lock().unwrap())
    }

    /// An app tells the door `d1` to end its hold.
    async fn release(&self, how: &str, content: Value) {
        let params = json!({"to": "d1:me", "how": how, "content": content});
        let message = json!({"jsonrpc": "2.0", "method": "door/release", "params": params});
        self.app.tx.send(message).await.unwrap();
    }

    /// Another app, attached to the session `s1`. Returns it and what the replay held.
    async fn attached(&self, scope: Scope) -> (Link, Vec<Value>) {
        let (mut app, theirs) = Link::pair();
        self.server.connect_as(theirs, scope);
        ask(
            &app,
            1,
            "_bridge/session/attach",
            json!({"sessionId": "s1"}),
        )
        .await;
        response(&mut app, 1).await;
        let mut replayed = Vec::new();
        loop {
            let message = next(&mut app).await;
            if message["params"]["update"]["sessionUpdate"] == "replay_complete" {
                return (app, replayed);
            }
            replayed.push(message);
        }
    }
}

fn prompt(word: &str) -> Value {
    json!({"sessionId": "s1", "prompt": [{"type": "text", "text": word}]})
}

fn chunk(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": text}})
}

/// The kinds of the session updates among `messages`.
fn kinds(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| message["params"]["update"]["sessionUpdate"].as_str())
        .collect()
}

/// The session updates among `messages`.
fn updates(messages: &[Value]) -> Vec<&Value> {
    messages
        .iter()
        .map(|message| &message["params"]["update"])
        .filter(|update| update.is_object())
        .collect()
}

/// Reads until the session update of this kind. Returns what came before it.
async fn until_update(app: &mut Link, kind: &str) -> Vec<Value> {
    let mut before = Vec::new();
    loop {
        let message = next(app).await;
        if message["params"]["update"]["sessionUpdate"] == kind {
            return before;
        }
        before.push(message);
    }
}

/// Reads until a message with this method.
async fn until(app: &mut Link, method: &str) -> Value {
    loop {
        let message = next(app).await;
        if message["method"] == method {
            return message;
        }
    }
}

fn selected(option: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option}})
}

const DROPPED: &str = "an addon dropped the message";

#[tokio::test]
async fn an_apps_request_is_changed_answered_refused_or_held_before_the_daemon_handles_it() {
    let mut t = Setup::with(&["d1"]).await;

    // Answered or refused by the addon: neither the daemon nor the adapter sees it. The
    // addon knows which app asked.
    ask(&t.app, 11, "door/echo", json!({"to": "d1:answer"})).await;
    let (answered, _) = response(&mut t.app, 11).await;
    assert_eq!(answered["result"], json!({"by": "d1", "scope": "admin"}));
    ask(&t.app, 12, "door/echo", json!({"to": "d1:refuse"})).await;
    let (refused, _) = response(&mut t.app, 12).await;
    assert_eq!(
        refused["error"],
        json!({"code": -32000, "message": "no", "data": {"addon": "test.d1"}})
    );

    // Changed: the daemon handles the addon's version. The adapter got nothing before it.
    ask(&t.app, 13, "session/prompt", prompt("d1:rewrite")).await;
    let asked = next(&mut t.adapter).await;
    assert_eq!(asked["params"]["prompt"][0]["text"], "rewritten");
    assert_eq!(
        asked["params"]["agent"], "demo",
        "the session's agent is known"
    );
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, 13).await;

    // Held, then let through, answered or dropped.
    let hold = json!({"to": "d1:hold"});
    ask(&t.app, 14, "_bridge/agents/list", hold.clone()).await;
    t.release("pass", Value::Null).await;
    let (listed, _) = response(&mut t.app, 14).await;
    assert_eq!(listed["result"]["agents"][0]["agentId"], "demo");
    ask(&t.app, 15, "_bridge/agents/list", hold.clone()).await;
    t.release("answer", json!({"agents": []})).await;
    let (listed, _) = response(&mut t.app, 15).await;
    assert_eq!(listed["result"], json!({"agents": []}));
    ask(&t.app, 16, "_bridge/agents/list", hold).await;
    t.release("discard", Value::Null).await;
    let (dropped, _) = response(&mut t.app, 16).await;
    assert_eq!(dropped["error"]["message"], DROPPED);
    t.server.shutdown().await;
}

#[tokio::test]
async fn the_daemons_own_answers_are_changed_or_held_and_may_depend_on_the_app() {
    let mut t = Setup::with(&["d1"]).await;

    // What the daemon tells an app in `initialize`.
    let hello = json!({"protocolVersion": 2, "info": {"name": "test"}});
    ask(&t.app, 11, "initialize", hello).await;
    let (ready, _) = response(&mut t.app, 11).await;
    assert_eq!(ready["result"]["capabilities"]["d1"], json!(true));
    assert_eq!(ready["result"]["info"]["name"], "harness-bridge");

    // The session list: the same request, answered differently for an observer.
    let logged = json!({"loggedOnly": true});
    ask(&t.app, 12, "_bridge/sessions/list", logged.clone()).await;
    let (listed, _) = response(&mut t.app, 12).await;
    assert_eq!(listed["result"]["sessions"][0]["sessionId"], "s1");
    let (mut observer, theirs) = Link::pair();
    t.server.connect_as(theirs, Scope::Observe);
    ask(&observer, 1, "_bridge/sessions/list", logged).await;
    let (listed, _) = response(&mut observer, 1).await;
    assert_eq!(listed["result"]["sessions"], json!([]));

    // Held on its way back, then released with another result.
    ask(
        &t.app,
        13,
        "_bridge/agents/list",
        json!({"to": "d1:hold-response"}),
    )
    .await;
    ask(&t.app, 14, "_bridge/agents/list", json!({})).await;
    let (_, before) = response(&mut t.app, 14).await;
    assert!(before.iter().all(|message| message["id"] != json!(13)));
    t.release("with", json!({"agents": "later"})).await;
    let (late, _) = response(&mut t.app, 13).await;
    assert_eq!(late["result"], json!({"agents": "later"}));

    // An adapter's answer passes here too, after the adapter-side hooks.
    ask(&t.app, 15, "session/prompt", prompt("plain")).await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &asked, json!({"stopReason": "end_turn"})).await;
    response(&mut t.app, 15).await;
    let traced = t.traced();
    let tail: Vec<&str> = traced
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        [
            "d1 app_request session/prompt",
            "d1 request session/prompt from App { name: Some(\"test\") }",
            "d1 app_response session/prompt",
        ]
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn what_an_app_is_sent_is_changed_dropped_or_held_per_app_live_and_on_replay() {
    let mut t = Setup::with(&["d1"]).await;
    let (mut observer, _) = t.attached(Scope::Observe).await;

    update(&t.adapter, "s1", json!({"sessionUpdate": "secret"})).await;
    update(&t.adapter, "s1", chunk("said")).await;
    update(&t.adapter, "s1", json!({"sessionUpdate": "marker"})).await;
    // Live: the observer is not sent the secret; both get the chunk as the addon left it.
    let seen = until_update(&mut t.app, "marker").await;
    assert_eq!(kinds(&seen), ["secret", "agent_message_chunk"]);
    assert_eq!(updates(&seen)[1]["content"]["text"], "said~d1");
    let seen = until_update(&mut observer, "marker").await;
    assert_eq!(kinds(&seen), ["agent_message_chunk"]);

    // On replay the same holds. The log has the update as it was: the addon's change is
    // made once on the way out, not stored.
    let (_, replayed) = t.attached(Scope::Interact).await;
    assert_eq!(
        kinds(&replayed),
        ["secret", "agent_message_chunk", "marker"]
    );
    assert_eq!(updates(&replayed)[1]["content"]["text"], "said~d1");
    let (_, replayed) = t.attached(Scope::Observe).await;
    assert_eq!(kinds(&replayed), ["agent_message_chunk", "marker"]);

    // Held for one app: that app gets it when the addon lets go. `observer` was dropped by
    // now, so only the first app is attached.
    drop(observer);
    update(&t.adapter, "s1", json!({"sessionUpdate": "slow"})).await;
    update(&t.adapter, "s1", json!({"sessionUpdate": "marker"})).await;
    let seen = until_update(&mut t.app, "marker").await;
    assert_eq!(kinds(&seen), Vec::<&str>::new());
    t.release("pass", Value::Null).await;
    until_update(&mut t.app, "slow").await;
    t.server.shutdown().await;
}

#[tokio::test]
async fn an_apps_notifications_and_answers_are_stopped_before_the_daemon() {
    let mut t = Setup::with(&["d1"]).await;

    // A notification the addon drops never reaches the daemon, so not the adapter either.
    let cancel = |word: &str| json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": "s1", "word": word}});
    t.app.tx.send(cancel("d1:block")).await.unwrap();
    t.app.tx.send(cancel("go")).await.unwrap();
    assert_eq!(next(&mut t.adapter).await["params"]["word"], "go");

    // An answer the addon drops does not count: the request stays open for the next one,
    // which the addon may change.
    let approval = |title: &str| {
        json!({"jsonrpc": "2.0", "id": 9, "method": "session/request_permission",
        "params": {"sessionId": "s1", "title": title, "options": []}})
    };
    t.adapter.tx.send(approval("rm")).await.unwrap();
    let shown = until(&mut t.app, "session/request_permission").await;
    for option in ["d1:veto", "d1:swap"] {
        let reply = json!({"jsonrpc": "2.0", "id": shown["id"], "result": selected(option)});
        t.app.tx.send(reply).await.unwrap();
    }
    assert_eq!(next(&mut t.adapter).await["result"], selected("swapped"));

    // A request the daemon sends apps can be kept from some of them.
    let (mut second, _) = t.attached(Scope::Interact).await;
    t.adapter.tx.send(approval("d1:first-only")).await.unwrap();
    update(&t.adapter, "s1", json!({"sessionUpdate": "marker"})).await;
    let shown = until(&mut t.app, "session/request_permission").await;
    assert_eq!(shown["params"]["title"], "d1:first-only");
    let seen = until_update(&mut second, "marker").await;
    assert!(
        seen.iter()
            .all(|message| message["method"] != "session/request_permission")
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn an_addon_asks_the_daemon_what_an_app_would() {
    let mut t = Setup::with(&["d1"]).await;

    // The addon has the daemon make a session. The daemon does its part: it asks the
    // adapter, records the session and announces it.
    ask(&t.app, 11, "door/make", json!({"to": "d1:new"})).await;
    let new = next(&mut t.adapter).await;
    assert_eq!(new["method"], "session/new");
    assert_eq!(new["params"], json!({"cwd": "/made"}));
    answer(&t.adapter, &new, json!({"sessionId": "s2"})).await;
    let (made, _) = response(&mut t.app, 11).await;
    assert_eq!(
        made["result"],
        json!({"sessionId": "s2"}),
        "the addon had the answer"
    );
    let announced = until(&mut t.app, "_bridge/sessions/updated").await;
    assert_eq!(announced["params"]["session"]["sessionId"], "s2");
    ask(
        &t.app,
        12,
        "_bridge/sessions/list",
        json!({"loggedOnly": true}),
    )
    .await;
    let (listed, _) = response(&mut t.app, 12).await;
    let ids: Vec<&Value> = listed["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| &session["sessionId"])
        .collect();
    assert!(ids.contains(&&json!("s2")), "{ids:?}");

    // Its request did not pass the app-side hooks, and the adapter-side ones saw it as the
    // addon's.
    let traced = t.traced();
    assert!(!traced.contains(&"d1 app_request session/new".to_owned()));
    assert!(
        traced.contains(&"d1 request session/new from Addon(\"test.d1\")".to_owned()),
        "{traced:?}"
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn the_app_side_has_the_same_layers_and_comes_before_the_adapter_side() {
    let mut t = Setup::with(&["d1", "d2"]).await;

    // In: every addon's app-side hook, then every addon's adapter-side hook. Back: the
    // reverse.
    ask(&t.app, 11, "session/prompt", prompt("plain")).await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, 11).await;
    assert_eq!(
        t.traced(),
        [
            "d1 app_request session/prompt",
            "d2 app_request session/prompt",
            "d1 request session/prompt from App { name: None }",
            "d2 request session/prompt from App { name: None }",
            "d2 app_response session/prompt",
            "d1 app_response session/prompt",
        ]
    );

    // The inner layer answers: the outer one sees the response, the inner one does not see
    // its own.
    ask(&t.app, 12, "door/echo", json!({"to": "d2:answer"})).await;
    response(&mut t.app, 12).await;
    assert_eq!(
        t.traced(),
        [
            "d1 app_request door/echo",
            "d2 app_request door/echo",
            "d1 app_response door/echo",
        ]
    );

    // An addon sends one app a notification. On its way to the app it passes every
    // addon's `to_app`, the sender's too.
    ask(&t.app, 13, "door/echo", json!({"to": "d2:whisper"})).await;
    let (_, before) = response(&mut t.app, 13).await;
    let psst: Vec<&Value> = before
        .iter()
        .filter(|message| message["method"] == "door/psst")
        .map(|message| &message["params"])
        .collect();
    assert_eq!(psst, [&json!({"d1": true, "d2": true})]);
    t.server.shutdown().await;
}

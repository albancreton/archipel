//! Addons see every message between an app and an adapter, in both directions, and may
//! change, stop or answer it, and send messages of their own.
//!
//! The adapter here is scripted by the test, so each check reads exactly what the adapter
//! received and what the app received. One addon, [`Probe`], uses every hook; what it does to
//! a message depends on a word in the message, so each test says what it triggers.

mod common;

use bridge_server::{
    Addon, Handle, Server,
    addon::{
        Agent, AgentRequest, Answer, Cx, Info, Keep, Kind, MethodCall, Notification, Outcome,
        Request, Response, Update, Verdict,
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

/// What the probes did, in order: `<name> <hook> <detail>`.
type Trace = Arc<Mutex<Vec<String>>>;

/// Uses every hook. A probe reacts to words that start with its own name, such as `a:refuse`,
/// so two probes in one daemon can be told what to do separately.
struct Probe {
    name: &'static str,
    trace: Trace,
    /// The request it holds, if any.
    held: Option<u64>,
    /// Its own calls that are not answered yet, and what each was for.
    calls: HashMap<u64, &'static str>,
    /// The ids of the requests it saw, to check that a response names its request.
    seen: HashMap<u64, String>,
}

impl Probe {
    fn new(name: &'static str, trace: &Trace) -> Self {
        Self {
            name,
            trace: trace.clone(),
            held: None,
            calls: HashMap::new(),
            seen: HashMap::new(),
        }
    }

    fn note(&self, what: &str) {
        self.trace
            .lock()
            .unwrap()
            .push(format!("{} {what}", self.name));
    }

    /// Whether `value` is this probe's word `word`, such as `a:refuse` for the probe `a`.
    fn told(&self, value: &Value, word: &str) -> bool {
        value.as_str() == Some(&format!("{}:{word}", self.name))
    }
}

impl Addon for Probe {
    fn info(&self) -> Info {
        Info {
            id: if self.name == "a" { "test.a" } else { "test.b" },
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

    fn request(&mut self, cx: &mut Cx<'_>, request: &mut Request<'_>) -> Verdict {
        self.note(&format!("request {}", request.method));
        self.seen.insert(request.id, request.method.to_owned());
        let session = request.params["sessionId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        match request.method {
            "initialize" => request.params["capabilities"][self.name] = json!(true),
            "session/new" => {
                request.params["sees"] = json!({
                    "adapter": request.agent.adapter,
                    "version": request.agent.version,
                    "fork": request.agent.capabilities.map(|all| all["session"]["fork"].clone()),
                    "os": request.agent.os,
                });
            }
            "session/resume" => {
                let logged = cx.last_update(&session, "config_option_update");
                request.params["logged"] =
                    json!(logged.map(|update| update["configOptions"].clone()));
            }
            "session/prompt" => {
                let word = request.params["prompt"][0]["text"].clone();
                if self.told(&word, "refuse") {
                    return Verdict::Refuse {
                        code: -32000,
                        message: "not this one".to_owned(),
                    };
                } else if self.told(&word, "answer") {
                    return Verdict::Answer(json!({"stopReason": "end_turn", "by": self.name}));
                } else if self.told(&word, "change") {
                    request.params["prompt"][0]["text"] = json!("changed");
                } else if self.told(&word, "call") {
                    let call = cx.call("probe/ping", json!({"from": self.name}));
                    self.calls.insert(call, "ping");
                } else if self.told(&word, "append") {
                    cx.append(
                        &session,
                        json!({"sessionUpdate": "x.probe.note", "by": self.name}),
                    );
                    cx.append("no-such-session", json!({"sessionUpdate": "x.probe.note"}));
                } else if self.told(&word, "hold") || self.told(&word, "hold-refuse") {
                    self.held = Some(request.id);
                    let then = if self.told(&word, "hold") {
                        "release"
                    } else {
                        "refuse"
                    };
                    let call = cx.call("probe/prepare", json!({}));
                    self.calls.insert(call, then);
                    return Verdict::Hold;
                } else if self.told(&word, "hold-forever") {
                    return Verdict::Hold;
                }
            }
            _ => {}
        }
        Verdict::Pass
    }

    fn response(&mut self, _cx: &mut Cx<'_>, response: &mut Response<'_>) {
        let known = self.seen.remove(&response.id).as_deref() == Some(response.method);
        let flaw = if known { "" } else { " (unknown id)" };
        self.note(&format!("response {}{flaw}", response.method));
        match response.method {
            "initialize" => {
                if let Outcome::Result(result) = response.outcome {
                    result["capabilities"][format!("{}Saw", self.name)] = json!(true);
                }
            }
            "session/prompt" => {
                let word = &response.params["prompt"][0]["text"];
                if self.told(word, "rescue") {
                    *response.outcome =
                        Outcome::Result(json!({"stopReason": "end_turn", "rescued": true}));
                } else if self.told(word, "break") {
                    *response.outcome = Outcome::Error {
                        code: -32001,
                        message: "broken on the way back".to_owned(),
                        data: None,
                    };
                } else if let Outcome::Result(result) = response.outcome
                    && result.is_object()
                {
                    // Each layer adds its name: the order shows which saw it first.
                    let mut via = result["via"].as_array().cloned().unwrap_or_default();
                    via.push(json!(self.name));
                    result["via"] = json!(via);
                }
            }
            _ => {}
        }
    }

    fn notification(&mut self, _cx: &mut Cx<'_>, notification: &mut Notification<'_>) -> Keep {
        self.note(&format!("notification {}", notification.method));
        if self.told(&notification.params["word"], "drop") {
            return Keep::No;
        }
        notification.params[self.name] = json!(true);
        Keep::Yes
    }

    fn update(&mut self, _cx: &mut Cx<'_>, update: &mut Update<'_>) -> Keep {
        let kind = update.update["sessionUpdate"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.note(&format!("update {kind}"));
        if kind == "noise" {
            return Keep::No;
        }
        if kind == "agent_message_chunk" {
            let text = update.update["content"]["text"]
                .as_str()
                .unwrap_or_default();
            update.update["content"]["text"] = json!(format!("{text}+{}", self.name));
        }
        Keep::Yes
    }

    fn agent_request(&mut self, _cx: &mut Cx<'_>, request: &mut AgentRequest<'_>) -> Verdict {
        self.note(&format!("agent_request {}", request.method));
        let title = request.params["title"].clone();
        if self.told(&title, "allow") {
            return Verdict::Answer(
                json!({"outcome": {"outcome": "selected", "optionId": "allow"}}),
            );
        } else if self.told(&title, "deny") {
            return Verdict::Refuse {
                code: -32000,
                message: "denied".to_owned(),
            };
        } else if self.told(&title, "hold") {
            return Verdict::Hold; // not available here: the same as passing
        }
        let seen = request.params["seenBy"].as_str().unwrap_or_default();
        request.params["seenBy"] = json!(format!("{seen}{}", self.name));
        Verdict::Pass
    }

    fn answer(&mut self, _cx: &mut Cx<'_>, answer: &mut Answer<'_>) {
        self.note(&format!("answer {}", answer.method));
        if let Outcome::Result(result) = answer.outcome {
            if self.told(&result["outcome"]["optionId"], "swap") {
                result["outcome"]["optionId"] = json!("swapped");
            }
            result["title"] = answer.params["title"].clone();
        }
    }

    fn reply(&mut self, cx: &mut Cx<'_>, call: u64, outcome: &Outcome) {
        let why = self.calls.remove(&call).unwrap_or("unknown");
        let said = match outcome {
            Outcome::Result(result) => result.to_string(),
            Outcome::Error { message, .. } => format!("error: {message}"),
        };
        self.note(&format!("reply {why} {said}"));
        let Some(held) = self.held.take() else {
            return;
        };
        match (why, outcome) {
            ("release", Outcome::Result(result)) => {
                let text = result["text"].as_str().unwrap_or_default();
                cx.release_with(held, prompt(text));
            }
            ("slow", Outcome::Result(result)) => cx.release(held, Verdict::Answer(result.clone())),
            _ => cx.release(
                held,
                Verdict::Refuse {
                    code: -32000,
                    message: "could not prepare".to_owned(),
                },
            ),
        }
    }

    fn method(&mut self, cx: &mut Cx<'_>, call: &MethodCall<'_>) -> Verdict {
        self.note(&format!("method {}", call.method));
        if !self.told(&call.params["to"], "me") {
            return Verdict::Pass;
        }
        match call.method {
            "_x/probe/echo" => Verdict::Answer(call.params.clone()),
            "_x/probe/no" => Verdict::Refuse {
                code: -32000,
                message: "no".to_owned(),
            },
            "_x/probe/slow" => {
                self.held = Some(call.id);
                let own = cx.call("probe/prepare", json!({}));
                self.calls.insert(own, "slow");
                Verdict::Hold
            }
            _ => Verdict::Pass,
        }
    }
}

/// A daemon with one `demo` agent, the given probes, an app, and a session `s1` on a started
/// adapter.
struct Setup {
    server: Handle,
    app: Link,
    adapter: Link,
    trace: Trace,
    next_id: u64,
}

impl Setup {
    async fn with(probes: &[&'static str]) -> Self {
        let trace = Trace::default();
        let (launched, mut adapters) = mpsc::unbounded_channel();
        let mut builder = Server::builder().agent(agent("demo", Some("demo"), launched));
        for name in probes {
            builder = builder.addon(Probe::new(name, &trace));
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
            next_id: 10,
        }
    }

    /// Sends a prompt whose text is `word` and returns the request's id.
    async fn prompt(&mut self, word: &str) -> u64 {
        self.next_id += 1;
        ask(&self.app, self.next_id, "session/prompt", prompt(word)).await;
        self.next_id
    }

    /// The adapter's process ends: the daemon finds its connection closed.
    fn adapter_dies(&mut self) {
        self.adapter = Link::pair().0;
    }

    /// What the probes did since the last call.
    fn traced(&self) -> Vec<String> {
        std::mem::take(&mut *self.trace.lock().unwrap())
    }
}

fn prompt(word: &str) -> Value {
    json!({"sessionId": "s1", "prompt": [{"type": "text", "text": word}]})
}

/// The kinds of the session updates among `messages`.
fn kinds(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| message["params"]["update"]["sessionUpdate"].as_str())
        .collect()
}

/// The params of the first session update among `messages`.
fn first_update(messages: &[Value]) -> &Value {
    messages
        .iter()
        .find(|message| message["method"] == "session/update")
        .map_or(&Value::Null, |message| &message["params"])
}

/// The next message that is not the daemon telling every app about a session's summary.
async fn next_of_session(app: &mut Link) -> Value {
    loop {
        let message = next(app).await;
        if message["method"] != "_bridge/sessions/updated" {
            return message;
        }
    }
}

#[tokio::test]
async fn a_request_is_changed_refused_or_answered_before_it_reaches_the_adapter() {
    let mut t = Setup::with(&["a"]).await;

    // Changed: the adapter gets the addon's version.
    let id = t.prompt("a:change").await;
    let asked = next(&mut t.adapter).await;
    assert_eq!(asked["params"], prompt("changed"));
    answer(&t.adapter, &asked, json!({"stopReason": "end_turn"})).await;
    response(&mut t.app, id).await;

    // Refused: the app gets the addon's error, marked with its id.
    let id = t.prompt("a:refuse").await;
    let (refused, _) = response(&mut t.app, id).await;
    assert_eq!(
        refused["error"],
        json!({"code": -32000, "message": "not this one", "data": {"addon": "test.a"}})
    );

    // Answered: the app gets the addon's result as if the adapter had given it.
    let id = t.prompt("a:answer").await;
    let (answered, _) = response(&mut t.app, id).await;
    assert_eq!(
        answered["result"],
        json!({"stopReason": "end_turn", "by": "a"})
    );

    // The adapter saw neither of the two.
    let id = t.prompt("plain").await;
    let asked = next(&mut t.adapter).await;
    assert_eq!(asked["params"], prompt("plain"));
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, id).await;
    t.server.shutdown().await;
}

#[tokio::test]
async fn a_response_is_changed_on_its_way_back_and_names_its_request() {
    let mut t = Setup::with(&["a"]).await;

    // A result gains a field.
    let id = t.prompt("plain").await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &asked, json!({"stopReason": "end_turn"})).await;
    let (changed, _) = response(&mut t.app, id).await;
    assert_eq!(
        changed["result"],
        json!({"stopReason": "end_turn", "via": ["a"]})
    );
    assert_eq!(
        t.traced(),
        ["a request session/prompt", "a response session/prompt"],
        "the response carried the id of the request the addon saw"
    );

    // An error becomes a result.
    let id = t.prompt("a:rescue").await;
    let asked = next(&mut t.adapter).await;
    let failure =
        json!({"jsonrpc": "2.0", "id": asked["id"], "error": {"code": -32603, "message": "boom"}});
    t.adapter.tx.send(failure).await.unwrap();
    let (rescued, _) = response(&mut t.app, id).await;
    assert_eq!(
        rescued["result"],
        json!({"stopReason": "end_turn", "rescued": true})
    );

    // A result becomes an error.
    let id = t.prompt("a:break").await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &asked, json!({"stopReason": "end_turn"})).await;
    let (broken, _) = response(&mut t.app, id).await;
    assert_eq!(
        broken["error"],
        json!({"code": -32001, "message": "broken on the way back"})
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn the_daemons_own_requests_pass_the_addons_too() {
    let trace = Trace::default();
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("demo", Some("demo"), launched))
        .addon(Probe::new("a", &trace))
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);

    // `initialize`: the addon adds to what the daemon declares, and to what the adapter
    // advertises. An app is shown the adapter's capabilities as the addon left them.
    let hello = json!({"protocolVersion": 2, "info": {"name": "test"}});
    ask(&app, 1, "initialize", hello).await;
    let mut adapter = adapters.recv().await.unwrap();
    let asked = next(&mut adapter).await;
    assert_eq!(asked["method"], "initialize");
    assert_eq!(asked["params"]["capabilities"]["a"], json!(true));
    assert!(
        asked["params"]["capabilities"]["elicitation"].is_object(),
        "the daemon's own stay"
    );
    let advertised = json!({"protocolVersion": 2, "capabilities": {"session": {"fork": {}}},
        "info": {"name": "demo-acp", "version": "1.2.3"}});
    answer(&adapter, &asked, advertised).await;
    let (ready, _) = response(&mut app, 1).await;
    assert_eq!(
        ready["result"]["capabilities"],
        json!({"session": {"fork": {}}, "aSaw": true})
    );

    // From then on an addon knows which adapter it is talking to.
    ask(&app, 2, "session/new", json!({"cwd": "/work"})).await;
    let new = next(&mut adapter).await;
    assert_eq!(
        new["params"]["sees"],
        json!({"adapter": "demo-acp", "version": "1.2.3", "fork": {}, "os": std::env::consts::OS})
    );
    let options =
        json!([{"configId": "model", "type": "select", "currentValue": "small", "options": []}]);
    let created = json!({"sessionId": "s1", "configOptions": options});
    answer(&adapter, &new, created).await;
    response(&mut app, 2).await;

    // The `session/resume` that wakes a session after its adapter died, with the log to read.
    drop(adapter);
    ask(&app, 3, "session/prompt", prompt("plain")).await;
    let mut adapter = started(&mut adapters).await;
    let resume = next(&mut adapter).await;
    assert_eq!(resume["method"], "session/resume");
    assert_eq!(resume["params"]["logged"], options);
    server.shutdown().await;
}

#[tokio::test]
async fn an_update_is_changed_or_dropped_before_the_log_and_the_apps_see_it() {
    let mut t = Setup::with(&["a"]).await;
    let id = t.prompt("plain").await;
    let asked = next(&mut t.adapter).await;
    update(&t.adapter, "s1", json!({"sessionUpdate": "noise"})).await;
    let chunk = json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "said"}});
    update(&t.adapter, "s1", chunk).await;
    answer(&t.adapter, &asked, json!({})).await;
    let (_, before) = response(&mut t.app, id).await;
    assert_eq!(
        kinds(&before),
        ["agent_message_chunk"],
        "the noise was dropped"
    );
    assert_eq!(first_update(&before)["update"]["content"]["text"], "said+a");

    // The log holds the changed update too: a second app that replays the session reads it.
    let (mut late, theirs) = Link::pair();
    t.server.connect(theirs);
    let attach = json!({"sessionId": "s1"});
    ask(&late, 1, "_bridge/session/attach", attach).await;
    response(&mut late, 1).await;
    let mut replayed = Vec::new();
    loop {
        let message = next(&mut late).await;
        match message["params"]["update"]["sessionUpdate"].as_str() {
            Some("replay_complete") => break,
            Some("agent_message_chunk") => {
                replayed.push(message["params"]["update"]["content"]["text"].clone());
            }
            kind => assert_ne!(kind, Some("noise")),
        }
    }
    assert_eq!(replayed, [json!("said+a")]);
    t.server.shutdown().await;
}

/// The adapter asks for an approval titled `title`, with JSON-RPC id 9.
async fn approval(adapter: &Link, title: &str) {
    let request = json!({"jsonrpc": "2.0", "id": 9, "method": "session/request_permission", "params": {
        "sessionId": "s1", "title": title,
        "subject": {"type": "tool_call", "toolCall": {"toolCallId": "t1", "title": title}}, "options": []}});
    adapter.tx.send(request).await.unwrap();
}

#[tokio::test]
async fn a_request_of_the_adapter_is_changed_or_answered_in_the_users_place() {
    let mut t = Setup::with(&["a"]).await;

    // Changed: the app is asked the addon's version, and its answer goes back as usual.
    approval(&t.adapter, "rm -rf").await;
    let asked = next_of_session(&mut t.app).await;
    assert_eq!(asked["method"], "session/request_permission");
    assert_eq!(asked["params"]["seenBy"], "a");
    let reply = json!({"jsonrpc": "2.0", "id": asked["id"],
        "result": {"outcome": {"outcome": "selected", "optionId": "a:swap"}}});
    t.app.tx.send(reply).await.unwrap();
    // The answer is changed on its way to the adapter, and the log records what was sent.
    let got = next(&mut t.adapter).await;
    assert_eq!(got["id"], json!(9));
    assert_eq!(
        got["result"],
        json!({"outcome": {"outcome": "selected", "optionId": "swapped"}, "title": "rm -rf"})
    );
    let resolved = next_of_session(&mut t.app).await;
    assert_eq!(resolved["method"], "_bridge/request/resolved");
    assert_eq!(
        resolved["params"]["outcome"]["outcome"]["optionId"],
        "swapped"
    );
    assert_eq!(
        t.traced(),
        [
            "a agent_request session/request_permission",
            "a answer session/request_permission"
        ]
    );

    // Answered by the addon: the adapter has its answer at once, nobody has to act, and the
    // session's history shows the request and who answered it.
    approval(&t.adapter, "a:allow").await;
    let got = next(&mut t.adapter).await;
    assert_eq!(
        got["result"],
        json!({"outcome": {"outcome": "selected", "optionId": "allow"}})
    );
    let shown = next_of_session(&mut t.app).await;
    assert_eq!(shown["method"], "session/request_permission");
    let resolved = next_of_session(&mut t.app).await;
    assert_eq!(resolved["method"], "_bridge/request/resolved");
    assert_eq!(resolved["params"]["requestId"], shown["id"]);
    assert_eq!(resolved["params"]["resolvedBy"], "addon test.a");
    // An answer an app sends anyway is too late and goes nowhere.
    let late = json!({"jsonrpc": "2.0", "id": shown["id"], "result": {"outcome": {"outcome": "cancelled"}}});
    t.app.tx.send(late).await.unwrap();

    // Refused by the addon: the adapter gets the error.
    approval(&t.adapter, "a:deny").await;
    let got = next(&mut t.adapter).await;
    assert_eq!(
        got["error"],
        json!({"code": -32000, "message": "denied", "data": {"addon": "test.a"}})
    );
    next_of_session(&mut t.app).await;
    let resolved = next_of_session(&mut t.app).await;
    assert_eq!(resolved["params"]["outcome"]["message"], "denied");

    // A hold is not available for the adapter's requests: the app is asked.
    approval(&t.adapter, "a:hold").await;
    assert_eq!(
        next_of_session(&mut t.app).await["method"],
        "session/request_permission"
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn a_notification_from_an_app_is_changed_or_dropped() {
    let mut t = Setup::with(&["a"]).await;
    let cancel = |word: &str| json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": "s1", "word": word}});
    t.app.tx.send(cancel("a:drop")).await.unwrap();
    t.app.tx.send(cancel("go")).await.unwrap();
    let got = next(&mut t.adapter).await;
    assert_eq!(got["method"], "session/cancel");
    assert_eq!(
        got["params"],
        json!({"sessionId": "s1", "word": "go", "a": true}),
        "the dropped one never came"
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn an_addon_makes_requests_of_its_own_and_gets_the_answers() {
    let mut t = Setup::with(&["a"]).await;

    // Its request goes out before the message it was looking at moves on.
    let id = t.prompt("a:call").await;
    let ping = next(&mut t.adapter).await;
    assert_eq!(ping["method"], "probe/ping");
    assert_eq!(ping["params"], json!({"from": "a"}));
    let asked = next(&mut t.adapter).await;
    assert_eq!(asked["method"], "session/prompt");
    answer(&t.adapter, &ping, json!({"pong": 1})).await;
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, id).await;
    assert_eq!(
        t.traced(),
        [
            "a request session/prompt",
            "a reply ping {\"pong\":1}",
            "a response session/prompt"
        ],
        "its own request and the answer to it do not pass its own hooks"
    );

    // If the adapter stops before answering, the addon is told.
    let id = t.prompt("a:call").await;
    next(&mut t.adapter).await;
    next(&mut t.adapter).await;
    t.adapter_dies();
    let (failed, _) = response(&mut t.app, id).await;
    assert!(failed["error"].is_object());
    assert!(
        t.traced()
            .contains(&"a reply ping error: the agent process stopped".to_owned()),
        "the addon's call was failed"
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn a_held_request_waits_for_the_addon_while_others_go_on() {
    let mut t = Setup::with(&["a"]).await;

    // Held: only the addon's own request reaches the adapter.
    let held = t.prompt("a:hold").await;
    let prepare = next(&mut t.adapter).await;
    assert_eq!(prepare["method"], "probe/prepare");
    // Another request is not kept waiting behind it.
    let other = t.prompt("plain").await;
    let asked = next(&mut t.adapter).await;
    assert_eq!(asked["params"], prompt("plain"));
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, other).await;
    // Released with other params once the addon has its answer.
    answer(&t.adapter, &prepare, json!({"text": "prepared"})).await;
    let asked = next(&mut t.adapter).await;
    assert_eq!(asked["params"], prompt("prepared"));
    answer(&t.adapter, &asked, json!({"stopReason": "end_turn"})).await;
    let (done, _) = response(&mut t.app, held).await;
    assert_eq!(done["result"]["stopReason"], "end_turn");

    // Released with a refusal: the request never reaches the adapter.
    let held = t.prompt("a:hold-refuse").await;
    let prepare = next(&mut t.adapter).await;
    answer(&t.adapter, &prepare, json!({})).await;
    let (refused, _) = response(&mut t.app, held).await;
    assert_eq!(refused["error"]["message"], "could not prepare");
    assert_eq!(refused["error"]["data"], json!({"addon": "test.a"}));

    // Still held when the adapter stops: whoever asked is told, as for any open request.
    let held = t.prompt("a:hold-forever").await;
    let seen = t.prompt("plain").await;
    next(&mut t.adapter).await;
    t.adapter_dies();
    let mut failures = 0;
    while failures < 2 {
        let message = next(&mut t.app).await;
        if message["id"] == json!(held) || message["id"] == json!(seen) {
            assert_eq!(message["error"]["message"], "the agent process stopped");
            failures += 1;
        }
    }
    t.server.shutdown().await;
}

#[tokio::test]
async fn an_addon_adds_updates_of_its_own_to_a_session() {
    let mut t = Setup::with(&["a"]).await;
    let id = t.prompt("a:append").await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &asked, json!({})).await;
    let (_, before) = response(&mut t.app, id).await;
    assert_eq!(
        kinds(&before),
        ["x.probe.note"],
        "the one for an unknown session was dropped"
    );
    let note = first_update(&before);
    assert_eq!(note["update"]["by"], "a");
    assert_eq!(
        note["update"]["_meta"]["bridge"],
        json!({"origin": "addon", "addon": "test.a"})
    );
    assert!(
        note["_meta"]["bridge"]["seq"].is_u64(),
        "it has a place in the log"
    );
    assert!(
        !t.traced().contains(&"a update x.probe.note".to_owned()),
        "an addon is not shown its own update"
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn an_addon_serves_methods_of_its_own() {
    let mut t = Setup::with(&["a"]).await;

    let call = json!({"to": "a:me", "n": 1});
    ask(&t.app, 20, "_x/probe/echo", call.clone()).await;
    let (echoed, _) = response(&mut t.app, 20).await;
    assert_eq!(echoed["result"], call);

    ask(&t.app, 21, "_x/probe/no", json!({"to": "a:me"})).await;
    let (refused, _) = response(&mut t.app, 21).await;
    assert_eq!(
        refused["error"],
        json!({"code": -32000, "message": "no", "data": {"addon": "test.a"}})
    );

    // It may need the adapter first: it holds the call and answers when it can.
    let call = json!({"to": "a:me", "sessionId": "s1"});
    ask(&t.app, 22, "_x/probe/slow", call).await;
    let prepare = next(&mut t.adapter).await;
    assert_eq!(prepare["method"], "probe/prepare");
    answer(&t.adapter, &prepare, json!({"ready": true})).await;
    let (slow, _) = response(&mut t.app, 22).await;
    assert_eq!(slow["result"], json!({"ready": true}));

    // A method no addon takes is an extension's, as before.
    ask(&t.app, 23, "_x/probe/echo", json!({"to": "nobody"})).await;
    let (unserved, _) = response(&mut t.app, 23).await;
    assert_eq!(
        unserved["error"]["message"],
        "no extension serves this method"
    );

    // An observer may not call them.
    let (mut observer, theirs) = Link::pair();
    t.server.connect_as(theirs, bridge_server::Scope::Observe);
    ask(&observer, 1, "_x/probe/echo", json!({"to": "a:me"})).await;
    let (forbidden, _) = response(&mut observer, 1).await;
    assert_eq!(forbidden["error"]["code"], -32003);
    t.server.shutdown().await;
}

#[tokio::test]
async fn addons_are_layers_with_the_first_added_nearest_the_app() {
    let mut t = Setup::with(&["a", "b"]).await;

    // Towards the adapter a then b; towards the app b then a.
    let id = t.prompt("plain").await;
    let asked = next(&mut t.adapter).await;
    let chunk = json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "said"}});
    update(&t.adapter, "s1", chunk).await;
    answer(&t.adapter, &asked, json!({})).await;
    let (done, before) = response(&mut t.app, id).await;
    assert_eq!(done["result"]["via"], json!(["b", "a"]));
    assert_eq!(
        first_update(&before)["update"]["content"]["text"],
        "said+b+a"
    );
    assert_eq!(
        t.traced(),
        [
            "a request session/prompt",
            "b request session/prompt",
            "b update agent_message_chunk",
            "a update agent_message_chunk",
            "b response session/prompt",
            "a response session/prompt",
        ]
    );

    // The outer layer answers: the inner one and the adapter see nothing.
    let id = t.prompt("a:answer").await;
    response(&mut t.app, id).await;
    assert_eq!(t.traced(), ["a request session/prompt"]);

    // The inner layer answers: the outer one sees the response as if the adapter had sent it.
    let id = t.prompt("b:answer").await;
    let (done, _) = response(&mut t.app, id).await;
    assert_eq!(
        done["result"],
        json!({"stopReason": "end_turn", "by": "b", "via": ["a"]})
    );
    assert_eq!(
        t.traced(),
        [
            "a request session/prompt",
            "b request session/prompt",
            "a response session/prompt"
        ]
    );

    // The outer layer's own request passes the inner layer, there and back, and not itself.
    let id = t.prompt("a:call").await;
    let ping = next(&mut t.adapter).await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &ping, json!({})).await;
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, id).await;
    assert_eq!(
        t.traced(),
        [
            "a request session/prompt",
            "b request probe/ping",
            "b request session/prompt",
            "b response probe/ping",
            "a reply ping {}",
            "b response session/prompt",
            "a response session/prompt",
        ]
    );

    // The inner layer's own request is seen by nobody else. Its own update passes the outer
    // layer on its way to the app.
    let id = t.prompt("b:call").await;
    let ping = next(&mut t.adapter).await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &ping, json!({})).await;
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, id).await;
    assert!(
        !t.traced()
            .iter()
            .any(|line| line.starts_with("a ") && line.contains("probe/ping"))
    );
    let id = t.prompt("b:append").await;
    let asked = next(&mut t.adapter).await;
    answer(&t.adapter, &asked, json!({})).await;
    response(&mut t.app, id).await;
    let traced = t.traced();
    assert!(
        traced.contains(&"a update x.probe.note".to_owned()),
        "{traced:?}"
    );
    assert!(
        !traced.contains(&"b update x.probe.note".to_owned()),
        "{traced:?}"
    );

    // A request of the adapter: b is nearer the adapter and sees it first. When a answers it
    // in the user's place, b sees that answer on its way to the adapter.
    approval(&t.adapter, "rm").await;
    assert_eq!(next_of_session(&mut t.app).await["params"]["seenBy"], "ba");
    t.traced();
    approval(&t.adapter, "a:allow").await;
    let got = next(&mut t.adapter).await;
    assert_eq!(got["result"]["title"], "a:allow", "b's answer hook ran");
    assert_eq!(
        t.traced(),
        [
            "b agent_request session/request_permission",
            "a agent_request session/request_permission",
            "b answer session/request_permission",
        ]
    );
    t.server.shutdown().await;
}

#[tokio::test]
async fn an_addon_is_only_shown_the_agents_it_applies_to() {
    let trace = Trace::default();
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("plain", None, launched))
        .addon(Probe::new("a", &trace))
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);

    ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(&mut adapters).await;
    let new = next(&mut adapter).await;
    assert_eq!(new["params"], json!({"cwd": "/work"}), "unchanged");
    answer(&adapter, &new, json!({"sessionId": "s1"})).await;
    response(&mut app, 1).await;
    ask(&app, 2, "session/prompt", prompt("a:refuse")).await;
    let asked = next(&mut adapter).await;
    assert_eq!(asked["method"], "session/prompt", "not refused");
    update(&adapter, "s1", json!({"sessionUpdate": "noise"})).await;
    answer(&adapter, &asked, json!({"stopReason": "end_turn"})).await;
    let (done, before) = response(&mut app, 2).await;
    assert_eq!(
        done["result"],
        json!({"stopReason": "end_turn"}),
        "unchanged"
    );
    assert_eq!(kinds(&before), ["noise"], "not dropped");
    assert_eq!(*trace.lock().unwrap(), Vec::<String>::new());
    server.shutdown().await;
}

//! Runs the daemon in-process against a fake adapter that replays recorded sessions.
//!
//! The test client sends what the recorded client sent, checks that everything the recorded
//! adapter sent arrives unchanged and in order, and compares what it ends up holding with the
//! renderings in `testdata/vectors`.

use bridge_agents::{AgentSpec, Fake, Launcher, Running};
use bridge_core::store::{MemoryStore, SessionRecord, Store};
use bridge_core::{
    mirror::{Mirror, seq_of},
    render,
    state::Direction,
    transcript::{Frame, Transcript},
};
use bridge_schema::{ext, rules::RuleSet};
use bridge_server::{Handle, Scope, Server, SqliteStore};
use bridge_transport::Link;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn transcript(name: &str) -> Transcript {
    let path = testdata().join(format!("transcripts/claude-v2/{name}.ndjson"));
    Transcript::parse(&fs::read_to_string(path).expect("readable")).expect("parses")
}

fn all_transcripts() -> Vec<(String, Transcript)> {
    let mut names: Vec<String> = fs::read_dir(testdata().join("transcripts/claude-v2"))
        .expect("transcripts exist")
        .flatten()
        .filter_map(|file| {
            let path = file.path();
            (path.extension()? == "ndjson")
                .then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    assert!(names.len() >= 17);
    names
        .into_iter()
        .map(|name| {
            let transcript = transcript(&name);
            (name, transcript)
        })
        .collect()
}

/// The rendering stored for a transcript, without its heading lines.
fn vector(name: &str) -> String {
    let text = fs::read_to_string(testdata().join(format!("vectors/claude-v2/{name}.txt")))
        .expect("vector exists");
    let (_, body) = text.split_once("\n\n").expect("heading, blank line, body");
    body.to_owned()
}

fn fake(transcript: &Transcript) -> AgentSpec {
    AgentSpec {
        id: "claude".into(),
        name: "Fake Claude".into(),
        launcher: Arc::new(Fake {
            transcript: transcript.clone(),
            speed: 0.0,
        }),
    }
}

/// The session id the recorded adapter gave out.
fn session_of(transcript: &Transcript) -> String {
    transcript
        .frames
        .iter()
        .find_map(|frame| frame.m.pointer("/result/sessionId")?.as_str())
        .expect("a session/new response")
        .to_owned()
}

/// The daemon wrote this message itself: an update it adds to the log, or a session summary.
fn from_daemon(message: &Value) -> bool {
    message.pointer("/params/update/_meta/bridge/origin") == Some(&json!("daemon"))
        || message["method"] == ext::NOTIFICATION_SESSIONS_UPDATED
}

fn without_meta(params: &Value) -> Value {
    let mut params = params.clone();
    if let Some(object) = params.as_object_mut() {
        object.remove("_meta");
    }
    params
}

/// An app connected to the daemon, with its copy of one session.
struct App {
    link: Link,
    next_id: u64,
    rules: RuleSet,
    mirror: Mirror,
}

impl App {
    async fn connect(server: &Handle) -> Self {
        Self::connect_with(server, Mirror::default()).await
    }

    /// Connects an app that already holds part of a session, as after losing its connection.
    async fn connect_with(server: &Handle, mirror: Mirror) -> Self {
        Self::connect_as(server, mirror, Scope::Admin)
    }

    fn observer(server: &Handle) -> Self {
        Self::connect_as(server, Mirror::default(), Scope::Observe)
    }

    fn connect_as(server: &Handle, mirror: Mirror, scope: Scope) -> Self {
        let (ours, theirs) = Link::pair();
        server.connect_as(theirs, scope);
        Self {
            link: ours,
            next_id: 100,
            rules: RuleSet::pinned(),
            mirror,
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.mirror.sent(&self.rules, &message).expect("applies");
        self.link.tx.send(message).await.expect("daemon is up");
        id
    }

    async fn notify(&mut self, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.link.tx.send(message).await.expect("daemon is up");
    }

    async fn answer(&mut self, id: &Value, result: Value) {
        let message = json!({"jsonrpc": "2.0", "id": id, "result": result});
        self.link.tx.send(message).await.expect("daemon is up");
    }

    /// The next message from the daemon, applied to the mirror.
    async fn next(&mut self) -> Value {
        let message = tokio::time::timeout(Duration::from_secs(10), self.link.rx.recv())
            .await
            .expect("the daemon sends something within 10 seconds")
            .expect("the daemon keeps the connection");
        self.mirror
            .received(&self.rules, &message)
            .expect("applies");
        message
    }

    /// The next message that came from the agent, skipping what the daemon added itself.
    async fn next_from_agent(&mut self) -> Value {
        loop {
            let message = self.next().await;
            if !from_daemon(&message) {
                return message;
            }
        }
    }

    /// Reads until `done` says so, and returns that message.
    async fn until(&mut self, done: impl Fn(&Value) -> bool) -> Value {
        loop {
            let message = self.next().await;
            if done(&message) {
                return message;
            }
        }
    }

    /// Sends a request and reads until its response.
    async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.request(method, params).await;
        self.until(|message| message.get("method").is_none() && message["id"] == json!(id))
            .await
    }

    /// Attaches to a session, from the mirror's position if it has one, and reads until the
    /// app is up to date.
    async fn attach(&mut self, session_id: &str) {
        let after = (self.mirror.seq > 0).then_some(self.mirror.seq);
        let params = json!({"sessionId": session_id, "afterSeq": after});
        let response = self.call(ext::METHOD_SESSION_ATTACH, params).await;
        let result = &response["result"];
        let head = result["headSeq"].as_u64().expect("a head");
        if result["catchUp"] == json!("replay") {
            self.mirror.reset();
            self.until(|message| message["params"]["update"]["sessionUpdate"] == "replay_complete")
                .await;
        } else {
            assert!(after.is_some(), "a delta only when a position was given");
            while self.mirror.seq < head {
                self.next().await;
            }
        }
        assert_eq!(self.mirror.seq, head);
    }

    fn rendering(&self) -> String {
        render::conversation(&self.mirror.conversation)
    }
}

/// Plays the client's part of a recorded session against the daemon.
#[derive(Default)]
struct Driver {
    /// Recorded request id to the id this client used.
    ids: HashMap<String, u64>,
    /// Requests from the agent that are waiting for the recorded answer.
    unanswered: VecDeque<Value>,
    last_seq: u64,
}

impl Driver {
    async fn play(&mut self, app: &mut App, frames: &[Frame]) {
        for frame in frames {
            let method = frame.m.get("method").and_then(Value::as_str);
            let params = frame.m.get("params").cloned().unwrap_or(Value::Null);
            match (frame.dir, method, frame.m.get("id")) {
                (Direction::Out, Some(method), Some(id)) => {
                    let ours = app.request(method, params).await;
                    self.ids.insert(id.to_string(), ours);
                }
                (Direction::Out, Some(method), None) => app.notify(method, params).await,
                (Direction::Out, None, _) => {
                    let id = self.unanswered.pop_front().expect("a request to answer");
                    app.answer(&id, frame.m["result"].clone()).await;
                    let told = self.numbered(app.next_from_agent().await);
                    assert_eq!(told["method"], ext::NOTIFICATION_REQUEST_RESOLVED);
                    assert_eq!(told["params"]["requestId"], id);
                    assert_eq!(told["params"]["outcome"], frame.m["result"]);
                }
                (Direction::In, None, Some(id)) => {
                    let got = app.next_from_agent().await;
                    assert_eq!(got["id"], json!(self.ids[&id.to_string()]), "a response");
                    if method_of(frames, id) != Some("initialize") {
                        assert_eq!(
                            got.get("result"),
                            frame.m.get("result"),
                            "passed on unchanged"
                        );
                    }
                }
                (Direction::In, Some("session/update"), None) => {
                    let got = self.numbered(app.next_from_agent().await);
                    assert_eq!(got["method"], "session/update");
                    assert_eq!(without_meta(&got["params"]), params, "passed on unchanged");
                }
                (Direction::In, Some(method), Some(_)) => {
                    let got = self.numbered(app.next_from_agent().await);
                    assert_eq!(got["method"], method);
                    assert_eq!(without_meta(&got["params"]), without_meta(&params));
                    self.unanswered.push_back(got["id"].clone());
                }
                // Other notifications from the adapter (account status) are not passed on.
                (Direction::In, _, None) => {}
            }
        }
    }

    /// Checks that a logged message carries the next sequence number.
    fn numbered(&mut self, message: Value) -> Value {
        let seq = seq_of(&message).expect("a sequence number");
        assert!(
            seq > self.last_seq,
            "sequence numbers grow: {seq} after {}",
            self.last_seq
        );
        self.last_seq = seq;
        message
    }
}

/// The method of the recorded request with the given id.
fn method_of<'a>(frames: &'a [Frame], id: &Value) -> Option<&'a str> {
    frames
        .iter()
        .find(|frame| frame.dir == Direction::Out && frame.m.get("id") == Some(id))
        .and_then(|frame| frame.m.get("method")?.as_str())
}

#[tokio::test]
async fn every_recorded_session_passes_through_unchanged_and_replays_the_same() {
    for (name, transcript) in all_transcripts() {
        let server = Server::builder().agent(fake(&transcript)).start();
        let session = session_of(&transcript);

        // Live: what the app holds at the end is what a direct client would hold.
        let mut live = App::connect(&server).await;
        Driver::default().play(&mut live, &transcript.frames).await;
        assert_eq!(live.rendering(), vector(&name), "{name}: live");

        // Another app arrives afterwards and gets the session from the log.
        let mut late = App::connect(&server).await;
        late.call("initialize", json!({"protocolVersion": 2})).await;
        late.attach(&session).await;
        assert_eq!(late.rendering(), vector(&name), "{name}: replay");

        // A plain ACP v2 client, using only `session/resume`.
        let mut plain = App::connect(&server).await;
        let params = json!({"sessionId": session, "cwd": "/", "mcpServers": [], "replayFrom": {"type": "start"}});
        let response = plain.call("session/resume", params).await;
        assert!(response.get("result").is_some(), "{name}: {response}");
        assert_eq!(plain.rendering(), vector(&name), "{name}: session/resume");

        let listed = live.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
        let listed = &listed["result"]["sessions"][0];
        assert_eq!(listed["sessionId"], json!(session), "{name}");
        assert_eq!(listed["live"], json!(true), "{name}");
        assert_eq!(listed["state"], json!("idle"), "{name}");
        assert_eq!(listed["headSeq"], json!(live.mirror.seq), "{name}");
        server.shutdown().await;
    }
}

#[tokio::test]
async fn an_app_that_lost_its_connection_catches_up_from_where_it_was() {
    for (name, transcript) in all_transcripts() {
        let server = Server::builder().agent(fake(&transcript)).start();
        let session = session_of(&transcript);
        // Stop just before the client's last message. The fake adapter waits for it, so
        // everything after it is certain to happen while the watcher is away.
        let cut = transcript
            .frames
            .iter()
            .rposition(|frame| frame.dir == Direction::Out)
            .expect("the client says something");

        let mut driver = Driver::default();
        let mut live = App::connect(&server).await;
        driver.play(&mut live, &transcript.frames[..cut]).await;

        let mut watcher = App::connect(&server).await;
        watcher.attach(&session).await;
        let seen = watcher.mirror.seq;
        let App { mirror, link, .. } = watcher;
        drop(link); // the connection is lost; the session goes on

        driver.play(&mut live, &transcript.frames[cut..]).await;

        let mut back = App::connect_with(&server, mirror).await;
        back.attach(&session).await;
        assert!(
            back.mirror.seq > seen,
            "{name}: something happened meanwhile"
        );
        assert_eq!(back.rendering(), vector(&name), "{name}");
        server.shutdown().await;
    }
}

#[tokio::test]
async fn an_unanswered_approval_waits_and_the_first_answer_wins() {
    let transcript = transcript("approval_denied");
    let session = session_of(&transcript);
    let asked = transcript
        .frames
        .iter()
        .position(|frame| frame.m["method"] == "session/request_permission")
        .expect("an approval request");
    let server = Server::builder().agent(fake(&transcript)).start();

    // The first app sees the request and goes away without answering.
    let mut first = App::connect(&server).await;
    Driver::default()
        .play(&mut first, &transcript.frames[..=asked])
        .await;
    drop(first);

    // Two apps arrive later. Both are asked.
    let mut second = App::connect(&server).await;
    second
        .call(
            "initialize",
            json!({"protocolVersion": 2, "info": {"name": "second app"}}),
        )
        .await;
    second.attach(&session).await;
    let mut third = App::connect(&server).await;
    third.attach(&session).await;
    let pending = |app: &App| -> Vec<String> {
        let (_, root) = app.mirror.conversation.root().expect("a session");
        root.pending_requests().map(|(id, _)| id.clone()).collect()
    };
    assert_eq!(pending(&second).len(), 1);
    assert_eq!(pending(&second), pending(&third));
    let request_id = json!(pending(&second)[0]);

    // The second app answers as recorded; the session runs to its recorded end.
    let mut driver = Driver {
        unanswered: VecDeque::from([request_id.clone()]),
        ..Driver::default()
    };
    driver
        .play(&mut second, &transcript.frames[asked + 1..])
        .await;
    assert_eq!(second.rendering(), vector("approval_denied"));

    // The third app answers too late: nothing changes, and it was told who answered.
    third
        .answer(
            &request_id,
            json!({"outcome": {"outcome": "selected", "optionId": "allow"}}),
        )
        .await;
    let told = third
        .until(|message| message["method"] == ext::NOTIFICATION_REQUEST_RESOLVED)
        .await;
    assert_eq!(told["params"]["resolvedBy"], json!("second app"));
    third.attach(&session).await;
    assert_eq!(third.rendering(), vector("approval_denied"));
    server.shutdown().await;
}

#[tokio::test]
async fn a_restarted_daemon_serves_sessions_from_its_log_and_wakes_them() {
    let path = std::env::temp_dir().join(format!("bridge-daemon-{}.sqlite", std::process::id()));
    let _ = fs::remove_file(&path);
    let finished = transcript("subagent");
    let interrupted = transcript("approval_command");

    // First daemon: one session runs to its end, another stops while an approval is open.
    let cut = interrupted
        .frames
        .iter()
        .position(|frame| frame.m["method"] == "session/request_permission")
        .expect("an approval request")
        + 1;
    for (transcript, upto) in [(&finished, finished.frames.len()), (&interrupted, cut)] {
        let server = Server::builder()
            .agent(fake(transcript))
            .store(SqliteStore::open(&path).unwrap())
            .start();
        let mut app = App::connect(&server).await;
        Driver::default()
            .play(&mut app, &transcript.frames[..upto])
            .await;
        server.shutdown().await;
    }

    // Second daemon, same file.
    let server = Server::builder()
        .agent(fake(&finished))
        .store(SqliteStore::open(&path).unwrap())
        .start();
    let mut app = App::connect(&server).await;
    let listed = app.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
    let sessions = listed["result"]["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(
        sessions
            .iter()
            .all(|session| session["live"] == json!(false))
    );

    // The finished session reads exactly as before, subagent included, with no adapter running.
    app.attach(&session_of(&finished)).await;
    assert_eq!(app.rendering(), vector("subagent"));

    // The interrupted one is reported as stopped, not left "running" for ever.
    let mut other = App::connect(&server).await;
    other.attach(&session_of(&interrupted)).await;
    let (_, root) = other.mirror.conversation.root().unwrap();
    assert_eq!(root.values["state"]["state"], json!("idle"));
    assert_eq!(root.values["state"]["stopReason"], json!("cancelled"));
    assert_eq!(
        root.events.last().unwrap()["sessionUpdate"],
        json!("notice")
    );
    let (_, approval) = root
        .requests
        .first()
        .expect("the approval is in the history");
    assert_eq!(
        approval.outcome,
        Some(json!({"outcome": {"outcome": "cancelled"}}))
    );

    // A prompt wakes the finished session: the adapter is started and asked to resume it.
    // (The fake then refuses the prompt, because its recording starts with a new session.)
    let params =
        json!({"sessionId": session_of(&finished), "prompt": [{"type": "text", "text": "more"}]});
    let response = app.call("session/prompt", params).await;
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("session/prompt"),
        "the adapter was reached: {response}"
    );
    let listed = app.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
    let woken = listed["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["sessionId"] == json!(session_of(&finished)))
        .unwrap();
    assert_eq!(woken["live"], json!(true));

    server.shutdown().await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[tokio::test]
async fn content_the_daemon_does_not_know_passes_through() {
    let frames = [
        json!({"dir": "out", "t": 0, "m": {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 2}}}),
        json!({"dir": "in", "t": 1, "m": {"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 2, "capabilities": {}}}}),
        json!({"dir": "out", "t": 2, "m": {"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": "/work", "mcpServers": []}}}),
        json!({"dir": "in", "t": 3, "m": {"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s1"}}}),
        json!({"dir": "out", "t": 4, "m": {"jsonrpc": "2.0", "id": 3, "method": "_x/acme/checkpoint/create", "params": {"sessionId": "s1", "label": "before"}}}),
        json!({"dir": "in", "t": 5, "m": {"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {
            "sessionUpdate": "x.acme.checkpoint", "checkpointId": "c1", "ref": "refs/acme/1", "nested": {"anything": [1, 2, {"deep": true}]}}}}}),
        json!({"dir": "in", "t": 6, "m": {"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {
            "sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "hi"}, "futureField": {"a": 1},
            "_meta": {"x.acme": {"cost": 3}}}}}}),
        json!({"dir": "in", "t": 7, "m": {"jsonrpc": "2.0", "id": 3, "result": {"checkpointId": "c1", "extra": [true]}}}),
    ];
    let header = json!({"transcript": 1, "scenario": "unknown", "agent": "x", "harness": "x", "adapter": "x",
        "protocol": 2, "model": "x", "prompts": [], "recordedAt": "2026-11-23"});
    let text: Vec<String> = std::iter::once(&header)
        .chain(frames.iter())
        .map(Value::to_string)
        .collect();
    let transcript = Transcript::parse(&text.join("\n")).unwrap();
    let server = Server::builder().agent(fake(&transcript)).start();

    // The driver checks that the unknown method's answer and both updates arrive unchanged.
    let mut live = App::connect(&server).await;
    Driver::default().play(&mut live, &transcript.frames).await;

    let mut late = App::connect(&server).await;
    late.attach("s1").await;
    for app in [&live, &late] {
        let (_, root) = app.mirror.conversation.root().unwrap();
        assert_eq!(root.unknown.len(), 1);
        assert_eq!(
            root.unknown[0]["nested"]["anything"][2]["deep"],
            json!(true)
        );
        assert_eq!(
            root.collections["agentMessage"]["m1"]["messageId"],
            json!("m1")
        );
    }
    server.shutdown().await;
}

/// A launcher whose adapter side is handed to the test, to script what the adapter does.
struct Scripted(mpsc::UnboundedSender<Link>);

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

#[tokio::test]
async fn when_the_adapter_dies_its_sessions_are_closed_cleanly_and_the_next_request_restarts_it() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let spec = AgentSpec {
        id: "claude".into(),
        name: "Scripted".into(),
        launcher: Arc::new(Scripted(launched)),
    };
    let server = Server::builder().agent(spec).start();
    let mut app = App::connect(&server).await;

    // The adapter starts on the app's first request and answers the daemon's `initialize`.
    let initialize = app
        .request(
            "initialize",
            json!({"protocolVersion": 2, "info": {"name": "test"}}),
        )
        .await;
    let mut adapter = adapters.recv().await.unwrap();
    let asked = adapter.rx.recv().await.unwrap();
    assert_eq!(asked["method"], "initialize");
    assert_eq!(asked["params"]["protocolVersion"], json!(2));
    let answer = json!({"jsonrpc": "2.0", "id": asked["id"], "result": {"protocolVersion": 2, "capabilities": {"session": {"fork": {}}}}});
    adapter.tx.send(answer).await.unwrap();
    let ready = app
        .until(|message| message["id"] == json!(initialize))
        .await;
    assert_eq!(
        ready["result"]["capabilities"]["session"]["fork"],
        json!({})
    );
    assert_eq!(ready["result"]["info"]["name"], json!("harness-bridge"));

    // A session with a running turn and an open approval.
    let servers = json!([{"name": "docs", "command": "docs-mcp", "args": [], "env": []}]);
    let new = app
        .request(
            "session/new",
            json!({"cwd": "/work", "mcpServers": servers}),
        )
        .await;
    let asked = adapter.rx.recv().await.unwrap();
    assert_eq!(asked["method"], "session/new");
    // An update that overtakes the response is kept, not lost.
    for message in [
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "state_update", "state": "running", "background": []}}}),
        json!({"jsonrpc": "2.0", "id": asked["id"], "result": {"sessionId": "s1"}}),
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": {"sessionUpdate": "tool_call_update", "toolCallId": "t1", "title": "rm -rf", "status": "pending"}}}),
        json!({"jsonrpc": "2.0", "id": 9, "method": "session/request_permission", "params": {"sessionId": "s1", "title": "rm -rf",
            "subject": {"type": "tool_call", "toolCall": {"toolCallId": "t1", "title": "rm -rf"}}, "options": []}}),
    ] {
        adapter.tx.send(message).await.unwrap();
    }
    app.until(|message| message["id"] == json!(new)).await;
    app.until(|message| message["method"] == "session/request_permission")
        .await;
    let (_, root) = app.mirror.conversation.root().unwrap();
    assert_eq!(root.values["state"]["state"], json!("running"));

    // The adapter dies.
    drop(adapter);
    let stopped = app
        .until(|message| message["params"]["update"]["stopReason"] == "cancelled")
        .await;
    assert!(from_daemon(&stopped));
    let (_, root) = app.mirror.conversation.root().unwrap();
    assert_eq!(
        root.pending_requests().count(),
        0,
        "the approval was cancelled"
    );
    assert_eq!(
        root.collections["toolCall"]["t1"]["status"],
        json!("cancelled"),
        "a tool call that was waiting does not stay pending for ever"
    );
    assert_eq!(
        root.events.last().unwrap()["sessionUpdate"],
        json!("notice")
    );
    let agents = app.call(ext::METHOD_AGENTS_LIST, json!({})).await;
    assert_eq!(agents["result"]["agents"][0]["readiness"], json!("error"));

    // The next prompt starts a new adapter, resumes the session there, then delivers the prompt.
    let prompt = app
        .request("session/prompt", json!({"sessionId": "s1", "prompt": []}))
        .await;
    let mut adapter = adapters.recv().await.unwrap();
    for (method, result) in [
        ("initialize", json!({"protocolVersion": 2})),
        ("session/resume", json!({})),
        ("session/prompt", json!({"messageId": "m1"})),
    ] {
        let asked = adapter.rx.recv().await.unwrap();
        assert_eq!(asked["method"], method);
        if method == "session/resume" {
            assert_eq!(
                asked["params"],
                json!({"sessionId": "s1", "cwd": "/work", "mcpServers": servers}),
                "the session is resumed with the servers it was created with"
            );
        }
        let answer = json!({"jsonrpc": "2.0", "id": asked["id"], "result": result});
        adapter.tx.send(answer).await.unwrap();
    }
    let answered = app.until(|message| message["id"] == json!(prompt)).await;
    assert_eq!(answered["result"]["messageId"], json!("m1"));
    server.shutdown().await;
}

#[tokio::test]
async fn an_observer_reads_everything_and_changes_nothing() {
    let transcript = transcript("approval_denied");
    let session = session_of(&transcript);
    let asked = transcript
        .frames
        .iter()
        .position(|frame| frame.m["method"] == "session/request_permission")
        .expect("an approval request");
    let server = Server::builder().agent(fake(&transcript)).start();
    let mut owner = App::connect(&server).await;
    let mut driver = Driver::default();
    driver.play(&mut owner, &transcript.frames[..=asked]).await;

    let mut observer = App::observer(&server);
    let welcome = observer
        .call("initialize", json!({"protocolVersion": 2}))
        .await;
    assert_eq!(
        welcome["result"]["_meta"]["bridge"]["scope"],
        json!("observe")
    );
    observer.attach(&session).await;
    let (_, root) = observer.mirror.conversation.root().unwrap();
    let (request_id, _) = root
        .pending_requests()
        .next()
        .expect("it sees the approval");
    let request_id = json!(request_id);

    // It may not act: prompting, creating and changing settings are refused.
    for (method, params) in [
        (
            "session/prompt",
            json!({"sessionId": session, "prompt": []}),
        ),
        ("session/new", json!({"cwd": "/", "mcpServers": []})),
        (
            "session/set_config_option",
            json!({"sessionId": session, "configId": "mode", "type": "id", "value": "bypassPermissions"}),
        ),
    ] {
        let refused = observer.call(method, params).await;
        assert_eq!(
            refused["error"]["code"],
            json!(-32003),
            "{method}: {refused}"
        );
    }
    // Its answer to the approval and its cancel are ignored: the owner's answer is the one
    // that counts, and the session runs to its recorded end.
    observer
        .answer(
            &request_id,
            json!({"outcome": {"outcome": "selected", "optionId": "allow-once"}}),
        )
        .await;
    observer
        .notify("session/cancel", json!({"sessionId": session}))
        .await;
    observer.call(ext::METHOD_AGENTS_LIST, json!({})).await; // both were read by now
    driver
        .play(&mut owner, &transcript.frames[asked + 1..])
        .await;
    assert_eq!(owner.rendering(), vector("approval_denied"));

    observer.attach(&session).await;
    assert_eq!(
        observer.rendering(),
        vector("approval_denied"),
        "and it saw all of it"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn sessions_only_the_harness_knows_are_listed_and_adopted_when_opened() {
    for (name, transcript) in all_transcripts() {
        // A replay from the harness has the updates but not the approvals and questions, so
        // only sessions without any can be compared with the stored rendering.
        let has_requests = transcript.frames.iter().any(|frame| {
            frame.dir == Direction::In
                && frame.m.get("method").is_some()
                && frame.m.get("id").is_some()
        });
        let session = session_of(&transcript);
        // A new daemon with an empty log. The harness has one session from earlier.
        let server = Server::builder().agent(fake(&transcript)).start();
        let mut app = App::connect(&server).await;

        let listed = app.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
        let sessions = listed["result"]["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1, "{name}");
        assert_eq!(sessions[0]["sessionId"], json!(session), "{name}");
        assert_eq!(sessions[0]["logged"], json!(false), "{name}");
        let logged = app
            .call(ext::METHOD_SESSIONS_LIST, json!({"loggedOnly": true}))
            .await;
        assert_eq!(logged["result"]["sessions"], json!([]), "{name}");

        // An observer cannot make the daemon load it.
        let mut observer = App::observer(&server);
        observer.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
        let params = json!({"sessionId": session});
        let refused = observer.call(ext::METHOD_SESSION_ATTACH, params).await;
        assert_eq!(refused["error"]["code"], json!(-32003), "{name}");

        // Opening it loads its history from the harness into the log.
        app.attach(&session).await;
        if !has_requests {
            assert_eq!(app.rendering(), vector(&name), "{name}");
        }
        let listed = app.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
        let sessions = listed["result"]["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1, "{name}: listed once, not twice");
        assert_eq!(sessions[0]["logged"], json!(true), "{name}");
        assert_eq!(sessions[0]["live"], json!(true), "{name}");
        assert_eq!(sessions[0]["headSeq"], json!(app.mirror.seq), "{name}");

        // From now on it is an ordinary session: the observer can read it too.
        observer.attach(&session).await;
        assert_eq!(observer.rendering(), app.rendering(), "{name}");
        server.shutdown().await;
    }
}

#[tokio::test]
async fn every_app_is_told_when_a_session_appears_or_its_summary_changes() {
    let transcript = transcript("simple");
    let session = session_of(&transcript);
    let server = Server::builder().agent(fake(&transcript)).start();
    // An app that is connected but attached to nothing, such as one showing a session list.
    let mut sidebar = App::observer(&server);
    sidebar
        .call("initialize", json!({"protocolVersion": 2}))
        .await;

    let mut live = App::connect(&server).await;
    Driver::default().play(&mut live, &transcript.frames).await;

    let mut states = Vec::new();
    loop {
        let told = sidebar.next().await;
        assert_eq!(
            told["method"],
            ext::NOTIFICATION_SESSIONS_UPDATED,
            "nothing else: {told}"
        );
        let summary = &told["params"]["session"];
        assert_eq!(summary["sessionId"], json!(session));
        states.push(summary["state"].clone());
        if summary["state"] == json!("idle") {
            assert!(summary["title"].is_string(), "the title arrived on the way");
            break;
        }
    }
    assert!(states.contains(&json!("running")), "{states:?}");
    server.shutdown().await;
}

#[tokio::test]
async fn sessions_unused_for_longer_than_the_retention_time_are_removed() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let record = |id: &str, at: u64| SessionRecord {
        session_id: id.into(),
        agent_id: "claude".into(),
        cwd: "/work".into(),
        title: None,
        state: None,
        setup: json!({}),
        head_seq: 0,
        created_at: at,
        updated_at: at,
    };
    let mut store = MemoryStore::default();
    store
        .put_session(&record("old", now - 3 * 86_400_000))
        .unwrap();
    store
        .put_session(&record("recent", now - 3_600_000))
        .unwrap();
    let server = Server::builder()
        .store(store)
        .retention(Duration::from_secs(86_400))
        .start();
    let mut app = App::connect(&server).await;
    // The first clean-up runs at start, on its own task: give it a moment.
    for _ in 0..50 {
        let listed = app.call(ext::METHOD_SESSIONS_LIST, json!({})).await;
        let ids: Vec<&str> = listed["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|session| session["sessionId"].as_str())
            .collect();
        if ids == ["recent"] {
            server.shutdown().await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the old session was not removed");
}

//! The daemon when things around it fail: the adapter, the store, an app that stops reading.
//! Whatever fails, an app that asked something gets an answer, and the daemon carries on.

mod common;

use bridge_agents::{AgentSpec, Launcher, Running};
use bridge_core::{
    log::{LogEntry, Record},
    store::{MemoryStore, SessionRecord, Store, StoreError},
};
use bridge_schema::ext;
use bridge_server::Server;
use bridge_transport::Link;
use common::{agent, answer, ask, next, response, started, update};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::mpsc;

/// A session `s1` on a started adapter.
async fn session(app: &mut Link, adapters: &mut mpsc::UnboundedReceiver<Link>) -> Link {
    ask(app, 1, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(adapters).await;
    let new = next(&mut adapter).await;
    answer(&adapter, &new, json!({"sessionId": "s1"})).await;
    response(app, 1).await;
    adapter
}

/// The step at which the adapter dies, and the request of the app that is waiting on it.
#[derive(Debug, Clone, Copy)]
enum Step {
    Initialize,
    New,
    Prompt,
    Resume,
    List,
    Delete,
    Options,
}

#[tokio::test]
async fn an_adapter_that_dies_fails_what_waited_on_it_and_the_next_request_starts_another() {
    for step in [
        Step::Initialize,
        Step::New,
        Step::Prompt,
        Step::Resume,
        Step::List,
        Step::Delete,
        Step::Options,
    ] {
        let (launched, mut adapters) = mpsc::unbounded_channel();
        let server = Server::builder()
            .agent(agent("claude", None, launched))
            .start();
        let (mut app, theirs) = Link::pair();
        server.connect(theirs);

        // Bring the daemon to the step, with the app's request 50 waiting on the adapter.
        let prompt = json!({"sessionId": "s1", "prompt": []});
        let adapter = match step {
            Step::Initialize => {
                ask(&app, 50, "session/new", json!({"cwd": "/work"})).await;
                let mut adapter = adapters.recv().await.unwrap();
                assert_eq!(next(&mut adapter).await["method"], "initialize");
                adapter
            }
            Step::New => {
                ask(&app, 50, "session/new", json!({"cwd": "/work"})).await;
                let mut adapter = started(&mut adapters).await;
                assert_eq!(next(&mut adapter).await["method"], "session/new");
                adapter
            }
            Step::Resume => {
                // The session loses its process, so the next prompt has to wake it first.
                drop(session(&mut app, &mut adapters).await);
                ask(&app, 50, "session/prompt", prompt.clone()).await;
                let mut adapter = started(&mut adapters).await;
                assert_eq!(next(&mut adapter).await["method"], "session/resume");
                adapter
            }
            _ => {
                let mut adapter = session(&mut app, &mut adapters).await;
                let (method, params) = match step {
                    Step::Prompt => ("session/prompt", prompt.clone()),
                    Step::List => (ext::METHOD_SESSIONS_LIST, json!({})),
                    Step::Delete => ("session/delete", json!({"sessionId": "s1"})),
                    _ => ("session/options", json!({"cwd": "/work"})),
                };
                ask(&app, 50, method, params).await;
                let asked = next(&mut adapter).await;
                assert!(
                    asked["method"].is_string(),
                    "{step:?}: the adapter was asked"
                );
                adapter
            }
        };

        drop(adapter);
        let (answered, _) = response(&mut app, 50).await;
        match step {
            // The daemon's own sessions are still listed; the harness's are left out.
            Step::List => assert_eq!(
                answered["result"]["sessions"][0]["sessionId"], "s1",
                "{step:?}: {answered}"
            ),
            _ => assert!(
                answered["error"]["message"].is_string(),
                "{step:?}: {answered}"
            ),
        }

        // The daemon carries on: the next request starts a new adapter and gets through.
        let known = !matches!(step, Step::Initialize | Step::New);
        if known {
            ask(&app, 60, "session/prompt", prompt).await;
            let mut adapter = started(&mut adapters).await;
            let resume = next(&mut adapter).await;
            assert_eq!(resume["method"], "session/resume", "{step:?}");
            answer(&adapter, &resume, json!({})).await;
            let asked = next(&mut adapter).await;
            assert_eq!(asked["method"], "session/prompt", "{step:?}");
            answer(&adapter, &asked, json!({"messageId": "m1"})).await;
        } else {
            ask(&app, 60, "session/new", json!({"cwd": "/work"})).await;
            let mut adapter = started(&mut adapters).await;
            let asked = next(&mut adapter).await;
            answer(&adapter, &asked, json!({"sessionId": "s2"})).await;
        }
        let (answered, _) = response(&mut app, 60).await;
        assert!(answered["result"].is_object(), "{step:?}: {answered}");
        server.shutdown().await;
    }
}

/// A launcher that fails until told otherwise.
struct Flaky {
    broken: Arc<AtomicBool>,
    tried: Arc<AtomicUsize>,
    launched: mpsc::UnboundedSender<Link>,
}

impl Launcher for Flaky {
    fn launch(&self) -> std::io::Result<Running> {
        self.tried.fetch_add(1, Ordering::SeqCst);
        if self.broken.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such program",
            ));
        }
        let (ours, theirs) = Link::pair();
        self.launched.send(theirs).map_err(std::io::Error::other)?;
        Ok(Running {
            link: ours,
            guard: None,
        })
    }
}

#[tokio::test]
async fn an_adapter_that_cannot_be_started_is_reported_and_tried_again_on_the_next_request() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let broken = Arc::new(AtomicBool::new(true));
    let tried = Arc::new(AtomicUsize::new(0));
    let launcher = Flaky {
        broken: broken.clone(),
        tried: tried.clone(),
        launched,
    };
    let spec = AgentSpec {
        id: "claude".into(),
        name: "claude".into(),
        harness: None,
        launcher: Arc::new(launcher),
    };
    let server = Server::builder().agent(spec).start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);

    ask(&app, 1, "session/new", json!({"cwd": "/work"})).await;
    let (refused, _) = response(&mut app, 1).await;
    let why = refused["error"]["message"].as_str().unwrap();
    assert!(
        why.contains("no such program"),
        "the reason is given: {why}"
    );
    ask(&app, 2, ext::METHOD_AGENTS_LIST, json!({})).await;
    let (listed, _) = response(&mut app, 2).await;
    assert_eq!(
        listed["result"]["agents"][0]["readiness"], "not_installed",
        "a program that was not found is told apart from one that failed"
    );
    assert!(
        listed["result"]["agents"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("no such program")
    );
    // Reading still works with no agent at all.
    ask(&app, 3, ext::METHOD_SESSIONS_LIST, json!({})).await;
    let (sessions, _) = response(&mut app, 3).await;
    assert_eq!(sessions["result"]["sessions"], json!([]));

    // The program is installed: the next request tries again and gets through.
    broken.store(false, Ordering::SeqCst);
    ask(&app, 4, "session/new", json!({"cwd": "/work"})).await;
    let mut adapter = started(&mut adapters).await;
    let asked = next(&mut adapter).await;
    answer(&adapter, &asked, json!({"sessionId": "s1"})).await;
    let (made, _) = response(&mut app, 4).await;
    assert_eq!(made["result"]["sessionId"], "s1");
    assert!(tried.load(Ordering::SeqCst) >= 2);
    ask(&app, 5, ext::METHOD_AGENTS_LIST, json!({})).await;
    let (listed, _) = response(&mut app, 5).await;
    assert_eq!(listed["result"]["agents"][0]["readiness"], "ready");
    server.shutdown().await;
}

#[tokio::test]
async fn what_an_adapter_should_not_send_is_survived() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("claude", None, launched))
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);
    let mut adapter = session(&mut app, &mut adapters).await;

    for odd in [
        json!("not an object"),
        json!({"jsonrpc": "2.0"}),
        // An answer to a question nobody asked.
        json!({"jsonrpc": "2.0", "id": 9999, "result": {}}),
        json!({"jsonrpc": "2.0", "id": "x", "error": {"code": 1, "message": "?"}}),
        // About a session that does not exist.
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "ghost",
            "update": {"sessionUpdate": "agent_message_chunk", "messageId": "m", "content": {"type": "text", "text": "boo"}}}}),
        // Without the fields an update has.
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1"}}),
        json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": 7}}),
    ] {
        adapter.tx.send(odd).await.unwrap();
    }
    // A request about a session that does not exist is refused, so the adapter is not left waiting.
    let ghost = json!({"jsonrpc": "2.0", "id": 77, "method": "session/request_permission",
        "params": {"sessionId": "ghost", "title": "rm", "options": []}});
    adapter.tx.send(ghost).await.unwrap();
    let refused = next(&mut adapter).await;
    assert_eq!(refused["id"], 77);
    assert!(refused["error"].is_object(), "{refused}");

    // The session still works, and the app was sent nothing about the ghost.
    let chunk = json!({"sessionUpdate": "agent_message_chunk", "messageId": "m1", "content": {"type": "text", "text": "hello"}});
    update(&adapter, "s1", chunk).await;
    loop {
        let message = next(&mut app).await;
        assert_ne!(message["params"]["sessionId"], "ghost");
        if message["params"]["update"]["messageId"] == "m1" {
            break;
        }
    }
    server.shutdown().await;
}

#[tokio::test]
async fn an_app_that_stops_reading_is_disconnected_and_catches_up_when_it_returns() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let server = Server::builder()
        .agent(agent("claude", None, launched))
        .start();
    let (mut slow, theirs) = Link::pair();
    server.connect(theirs);
    let adapter = session(&mut slow, &mut adapters).await;
    let (mut other, theirs) = Link::pair();
    server.connect(theirs);
    ask(
        &other,
        1,
        ext::METHOD_SESSION_ATTACH,
        json!({"sessionId": "s1"}),
    )
    .await;
    let (attached, _) = response(&mut other, 1).await;
    let before = attached["result"]["headSeq"].as_u64().unwrap();

    // More updates than an app's queue holds, while `slow` reads nothing.
    let total = bridge_transport::QUEUE as u64 + 500;
    let reader = tokio::spawn(async move {
        let mut last = 0;
        while last < before + total {
            let message = next(&mut other).await;
            if let Some(seq) = message
                .pointer("/params/_meta/bridge/seq")
                .and_then(Value::as_u64)
            {
                last = seq;
            }
        }
        other
    });
    for number in 0..total {
        let chunk = json!({"sessionUpdate": "agent_message_chunk", "messageId": format!("m{number}"), "content": {"type": "text", "text": "x"}});
        update(&adapter, "s1", chunk).await;
    }
    // The app that reads got all of it.
    reader.await.unwrap();

    // The slow one was cut off: its connection ends after what was queued for it.
    let mut last = 0;
    while let Some(message) = slow.rx.recv().await {
        if let Some(seq) = message
            .pointer("/params/_meta/bridge/seq")
            .and_then(Value::as_u64)
        {
            last = seq;
        }
    }
    assert!(last < before + total, "it did not get everything");

    // It comes back and asks for what it missed: exactly the rest, in order.
    let (mut back, theirs) = Link::pair();
    server.connect(theirs);
    ask(
        &back,
        1,
        ext::METHOD_SESSION_ATTACH,
        json!({"sessionId": "s1", "afterSeq": last}),
    )
    .await;
    let mut expected = last + 1;
    while expected <= before + total {
        let message = next(&mut back).await;
        if let Some(seq) = message
            .pointer("/params/_meta/bridge/seq")
            .and_then(Value::as_u64)
        {
            assert_eq!(seq, expected);
            expected += 1;
        }
    }
    server.shutdown().await;
}

/// A store that fails every write while `broken` is set.
struct Unreliable {
    inner: MemoryStore,
    broken: Arc<AtomicBool>,
}

impl Unreliable {
    fn check(&self) -> Result<(), StoreError> {
        if self.broken.load(Ordering::SeqCst) {
            return Err(StoreError("the disk is full".into()));
        }
        Ok(())
    }
}

impl Store for Unreliable {
    fn put_session(&mut self, record: &SessionRecord) -> Result<(), StoreError> {
        self.check()?;
        self.inner.put_session(record)
    }
    fn sessions(&self) -> Result<Vec<SessionRecord>, StoreError> {
        self.inner.sessions()
    }
    fn session(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        self.inner.session(session_id)
    }
    fn delete_session(&mut self, session_id: &str) -> Result<(), StoreError> {
        self.check()?;
        self.inner.delete_session(session_id)
    }
    fn append(
        &mut self,
        root: &str,
        session_id: &str,
        record: &Record,
        at: u64,
    ) -> Result<u64, StoreError> {
        self.check()?;
        self.inner.append(root, session_id, record, at)
    }
    fn after(&self, root: &str, seq: u64) -> Result<Vec<LogEntry>, StoreError> {
        self.inner.after(root, seq)
    }
}

#[tokio::test]
async fn a_store_that_fails_is_reported_to_the_apps_and_the_daemon_carries_on() {
    let (launched, mut adapters) = mpsc::unbounded_channel();
    let broken = Arc::new(AtomicBool::new(false));
    let store = Unreliable {
        inner: MemoryStore::default(),
        broken: broken.clone(),
    };
    let server = Server::builder()
        .agent(agent("claude", None, launched))
        .store(store)
        .start();
    let (mut app, theirs) = Link::pair();
    server.connect(theirs);
    let mut adapter = session(&mut app, &mut adapters).await;
    let chunk = |id: &str| json!({"sessionUpdate": "agent_message_chunk", "messageId": id, "content": {"type": "text", "text": id}});

    update(&adapter, "s1", chunk("before")).await;
    let seen = loop {
        let message = next(&mut app).await;
        if message["params"]["update"]["messageId"] == "before" {
            break message["params"]["_meta"]["bridge"]["seq"]
                .as_u64()
                .unwrap();
        }
    };

    // While the store fails, the attached app is told once, however many updates are lost,
    // and what could not be stored is not shown: no app ever holds more than the log.
    broken.store(true, Ordering::SeqCst);
    update(&adapter, "s1", chunk("lost")).await;
    update(&adapter, "s1", chunk("lost too")).await;
    let told = next(&mut app).await;
    assert_eq!(told["method"], ext::NOTIFICATION_SESSION_ERROR);
    assert_eq!(told["params"]["sessionId"], "s1");
    assert_eq!(told["params"]["code"], ext::ERROR_STORAGE);
    assert!(
        told["params"]["message"]
            .as_str()
            .unwrap()
            .contains("the disk is full")
    );

    // A message of the user's is refused with the same code and never reaches the agent:
    // the turn it would start could not be kept.
    let prompt = json!({"sessionId": "s1", "prompt": []});
    ask(&app, 2, "session/prompt", prompt.clone()).await;
    let (refused, before) = response(&mut app, 2).await;
    assert_eq!(refused["error"]["code"], ext::ERROR_STORAGE);
    assert!(
        before.is_empty(),
        "one notice, and no lost update: {before:?}"
    );
    // Other requests are still passed on and answered.
    ask(
        &app,
        3,
        "session/set_title",
        json!({"sessionId": "s1", "title": "t"}),
    )
    .await;
    let asked = next(&mut adapter).await;
    assert_eq!(asked["method"], "session/set_title");
    answer(&adapter, &asked, json!({})).await;
    response(&mut app, 3).await;

    // The store works again: prompts pass, and numbering continues with no gap.
    broken.store(false, Ordering::SeqCst);
    ask(&app, 4, "session/prompt", prompt).await;
    let asked = next(&mut adapter).await;
    assert_eq!(asked["method"], "session/prompt");
    answer(&adapter, &asked, json!({"messageId": "m"})).await;
    let (answered, _) = response(&mut app, 4).await;
    assert_eq!(answered["result"]["messageId"], "m");
    update(&adapter, "s1", chunk("after")).await;
    let after = loop {
        let message = next(&mut app).await;
        if message["params"]["update"]["messageId"] == "after" {
            break message["params"]["_meta"]["bridge"]["seq"]
                .as_u64()
                .unwrap();
        }
    };
    assert!(after > seen);
    server.shutdown().await;
}

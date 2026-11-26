//! A daemon's sessions followed from a GPUI app.
//!
//! The kit (`archipel-gpui`) draws state and never talks to the daemon. This is the other half
//! for an app that wants it done: a [`Client`] that holds the connection, the session list and
//! the agents; a [`Session`] that stays in step with the daemon, across reconnects; and
//! [`live_chat`], a `ChatView` whose controls act on a session.
//!
//! ```ignore
//! let client = Client::connect(Endpoint::Local(socket), "my-app", cx);
//! let session = client.update(cx, |client, cx| client.session("…", cx));
//! let chat = live_chat(&session, window, cx);
//! ```
//!
//! The connection lives on a thread of its own, since it is written for tokio and GPUI has its
//! own executor. Everything here is used from the app's main thread.

use archipel_client::{Connection, Endpoint, Error};
use archipel_core::mirror::{Mirror, root_of};
use archipel_core::state::Conversation;
use archipel_gpui::sessions::SessionSummary;
use archipel_gpui::{ChatActions, ChatView, OpenElsewhere, Outcome, SettingValue};
use archipel_schema::{ext, rules::RuleSet};
use gpui::{App, AppContext as _, Context, Entity, Task, WeakEntity, Window};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// A request the daemon or the agent refused, or that could not be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The JSON-RPC code; 0 when the connection was lost before an answer.
    pub code: i64,
    pub message: String,
}

impl Refusal {
    fn lost() -> Self {
        Self {
            code: 0,
            message: "the connection to the daemon was lost".to_owned(),
        }
    }

    /// Another process of the harness has the session open.
    #[must_use]
    pub fn open_elsewhere(&self) -> bool {
        self.code == ext::ERROR_OPEN_ELSEWHERE
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Where the connection stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Connected,
    /// Lost or never made, and being tried again. Says why.
    Retrying(String),
}

/// What the main thread asks of the connection's thread.
enum Order {
    Send(Value),
}

/// What the connection's thread tells the main thread.
enum Report {
    Connected(Value),
    Failed(String),
    Message(Value),
}

/// The ids of this client's requests start here, away from the ones `Connection` uses itself.
const FIRST_ID: u64 = 1_000_000;

/// Runs the connection: opens it, passes messages both ways, and opens it again when it is lost.
async fn carry(
    endpoint: Endpoint,
    name: String,
    mut orders: UnboundedReceiver<Order>,
    reports: async_channel::Sender<Report>,
) {
    loop {
        match Connection::open(&endpoint, &name).await {
            Ok((mut connection, welcome)) => {
                if reports.send(Report::Connected(welcome)).await.is_err() {
                    return;
                }
                let why = loop {
                    tokio::select! {
                        order = orders.recv() => match order {
                            Some(Order::Send(message)) => {
                                if let Err(error) = connection.send(message).await {
                                    break error.to_string();
                                }
                            }
                            // The client was dropped.
                            None => return,
                        },
                        message = connection.next() => match message {
                            Some(message) => {
                                if reports.send(Report::Message(message)).await.is_err() {
                                    return;
                                }
                            }
                            None => break Error::Closed.to_string(),
                        },
                    }
                };
                if reports.send(Report::Failed(why)).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                if reports
                    .send(Report::Failed(error.to_string()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        // What was asked while there was no connection is not sent late.
        while orders.try_recv().is_ok() {}
        if orders.is_closed() {
            return;
        }
    }
}

struct Waiting {
    reply: async_channel::Sender<Result<Value, Refusal>>,
    /// The followed session the request is about, whose mirror gets the response.
    session: Option<String>,
}

/// The connection to one daemon: its sessions, its agents, and the sessions the app follows.
pub struct Client {
    /// Where the daemon is, as it was given.
    pub endpoint: Endpoint,
    pub link: Link,
    /// What the daemon answered to `initialize`: its capabilities and this connection's scope.
    pub welcome: Value,
    /// The session list as last asked for ([`Client::refresh_sessions`]), kept current as the
    /// daemon says sessions appear, change and go.
    pub sessions: Vec<Value>,
    /// The agents as last asked for ([`Client::refresh_agents`]).
    pub agents: Vec<Value>,
    rules: Rc<RuleSet>,
    orders: UnboundedSender<Order>,
    next_id: u64,
    waiting: HashMap<u64, Waiting>,
    followed: HashMap<String, WeakEntity<Session>>,
    _reports: Task<()>,
}

impl Client {
    /// Connects, and keeps trying while the daemon cannot be reached. `name` is the app's, shown
    /// to other apps when it answers a request.
    pub fn connect(endpoint: Endpoint, name: &str, cx: &mut App) -> Entity<Self> {
        let (orders, receive_orders) = unbounded_channel();
        let (send_reports, reports) = async_channel::unbounded();
        let (carried, name) = (endpoint.clone(), name.to_owned());
        std::thread::Builder::new()
            .name("archipel-connection".to_owned())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(carry(carried, name, receive_orders, send_reports));
                }
            })
            .ok();
        cx.new(|cx| {
            let listen = cx.spawn(async move |client: WeakEntity<Self>, cx| {
                while let Ok(report) = reports.recv().await {
                    if client
                        .update(cx, |client, cx| client.heard(report, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Self {
                endpoint,
                link: Link::Connecting,
                welcome: Value::Null,
                sessions: Vec::new(),
                agents: Vec::new(),
                rules: Rc::new(RuleSet::pinned()),
                orders,
                next_id: FIRST_ID,
                waiting: HashMap::new(),
                followed: HashMap::new(),
                _reports: listen,
            }
        })
    }

    #[must_use]
    pub fn connected(&self) -> bool {
        self.link == Link::Connected
    }

    fn heard(&mut self, report: Report, cx: &mut Context<Self>) {
        match report {
            Report::Connected(welcome) => {
                self.welcome = welcome;
                self.link = Link::Connected;
                // Every session continues from the last sequence number it applied.
                let followed: Vec<_> = self
                    .followed
                    .values()
                    .filter_map(WeakEntity::upgrade)
                    .collect();
                for session in followed {
                    self.attach(&session, cx);
                }
                self.refresh_sessions(cx);
                self.refresh_agents(cx);
            }
            Report::Failed(why) => {
                self.link = Link::Retrying(why);
                for (_, waiting) in self.waiting.drain() {
                    waiting.reply.try_send(Err(Refusal::lost())).ok();
                }
            }
            Report::Message(message) => self.received(&message, cx),
        }
        cx.notify();
    }

    fn received(&mut self, message: &Value, cx: &mut Context<Self>) {
        let params = message.get("params").unwrap_or(&Value::Null);
        match message.get("method").and_then(Value::as_str) {
            Some(ext::NOTIFICATION_SESSIONS_REMOVED) => {
                if let Some(id) = params.get("sessionId").and_then(Value::as_str) {
                    self.forget(id, cx);
                }
            }
            Some(ext::NOTIFICATION_SESSIONS_UPDATED) => {
                if let Some(summary) = params.get("session")
                    && let Some(id) = summary.get("sessionId")
                {
                    match self
                        .sessions
                        .iter_mut()
                        .find(|known| known.get("sessionId") == Some(id))
                    {
                        Some(known) => *known = summary.clone(),
                        None => self.sessions.insert(0, summary.clone()),
                    }
                }
            }
            Some(ext::NOTIFICATION_SESSION_ERROR) => {
                let said = params
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(session) = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .and_then(|id| self.followed.get(id))
                    .and_then(WeakEntity::upgrade)
                {
                    session.update(cx, |session, cx| {
                        session.error = said;
                        cx.notify();
                    });
                }
            }
            Some(_) => {
                // A subagent's messages belong to the session the user talks to.
                if let Some(session) = root_of(message)
                    .and_then(|id| self.followed.get(id))
                    .and_then(WeakEntity::upgrade)
                {
                    let rules = self.rules.clone();
                    session.update(cx, |session, cx| session.received(&rules, message, cx));
                }
            }
            None => {
                let Some(waiting) = message
                    .get("id")
                    .and_then(Value::as_u64)
                    .and_then(|id| self.waiting.remove(&id))
                else {
                    return;
                };
                if let Some(session) = waiting
                    .session
                    .as_deref()
                    .and_then(|id| self.followed.get(id))
                    .and_then(WeakEntity::upgrade)
                {
                    let rules = self.rules.clone();
                    session.update(cx, |session, cx| session.received(&rules, message, cx));
                }
                let result = match message.get("error") {
                    Some(error) => Err(Refusal {
                        code: error
                            .get("code")
                            .and_then(Value::as_i64)
                            .unwrap_or_default(),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("refused")
                            .to_owned(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                waiting.reply.try_send(result).ok();
            }
        }
    }

    /// Sends any request and gives its result: one of the daemon's, or an extension's
    /// `_x/<namespace>/…` method. Not from inside an update of the session it is about: a
    /// [`Session`] has its own methods for that.
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        cx: &mut Context<Self>,
    ) -> Task<Result<Value, Refusal>> {
        let (id, message) = self.request(method, params);
        let session = message
            .pointer("/params/sessionId")
            .and_then(Value::as_str)
            .filter(|id| self.followed.contains_key(*id))
            .map(str::to_owned);
        if let Some(followed) = session
            .as_deref()
            .and_then(|id| self.followed.get(id))
            .and_then(WeakEntity::upgrade)
        {
            let rules = self.rules.clone();
            // The mirror notes what was asked, to apply the answer when it comes.
            followed.update(cx, |followed, _| {
                followed.mirror.sent(&rules, &message).ok()
            });
        }
        self.dispatch(id, message, session, cx)
    }

    /// A request ready to send, with its id.
    fn request(&mut self, method: &str, params: Value) -> (u64, Value) {
        let id = self.next_id;
        self.next_id += 1;
        (
            id,
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
    }

    /// Sends a request and gives its result. `session` is the followed session whose mirror
    /// gets the response.
    fn dispatch(
        &mut self,
        id: u64,
        message: Value,
        session: Option<String>,
        cx: &App,
    ) -> Task<Result<Value, Refusal>> {
        let (reply, answer) = async_channel::bounded(1);
        if self.connected() && self.orders.send(Order::Send(message)).is_ok() {
            self.waiting.insert(id, Waiting { reply, session });
        } else {
            reply.try_send(Err(Refusal::lost())).ok();
        }
        cx.background_spawn(
            async move { answer.recv().await.unwrap_or_else(|_| Err(Refusal::lost())) },
        )
    }

    fn tell(&self, message: Value) {
        self.orders.send(Order::Send(message)).ok();
    }

    /// Asks the daemon for its sessions, newest first, and keeps them in [`Client::sessions`].
    pub fn refresh_sessions(&mut self, cx: &mut Context<Self>) {
        let listed = self.call(ext::METHOD_SESSIONS_LIST, json!({}), cx);
        cx.spawn(async move |client, cx| {
            if let Ok(result) = listed.await {
                client
                    .update(cx, |client, cx| {
                        client.sessions = result
                            .get("sessions")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        cx.notify();
                    })
                    .ok();
            }
        })
        .detach();
    }

    /// Asks the daemon for its agents and keeps them in [`Client::agents`]. An agent says what
    /// it can do once it has been started.
    pub fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        let listed = self.call(ext::METHOD_AGENTS_LIST, json!({}), cx);
        cx.spawn(async move |client, cx| {
            if let Ok(result) = listed.await {
                client
                    .update(cx, |client, cx| {
                        client.agents = result
                            .get("agents")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        cx.notify();
                    })
                    .ok();
            }
            // The daemon does not say when an agent has finished starting, so it is asked
            // again until none is.
            let starting = client
                .read_with(cx, |client, _| {
                    client
                        .agents
                        .iter()
                        .any(|agent| agent["readiness"] == "starting")
                })
                .unwrap_or(false);
            if starting {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                client
                    .update(cx, |client, cx| client.refresh_agents(cx))
                    .ok();
            }
        })
        .detach();
    }

    /// The session list, read for the kit's `SessionList`.
    #[must_use]
    pub fn summaries(&self) -> Vec<SessionSummary> {
        self.sessions
            .iter()
            .filter_map(SessionSummary::of)
            .collect()
    }

    /// Follows a session: the same entity each time it is asked for, in step with the daemon
    /// for as long as the app holds it.
    pub fn session(&mut self, session_id: &str, cx: &mut Context<Self>) -> Entity<Session> {
        if let Some(known) = self.followed.get(session_id).and_then(WeakEntity::upgrade) {
            return known;
        }
        let client = cx.weak_entity();
        let session = cx.new(|_| Session {
            id: session_id.to_owned(),
            client,
            mirror: Mirror::default(),
            open_elsewhere: None,
            error: None,
            removed: false,
            ready: false,
        });
        self.followed
            .insert(session_id.to_owned(), session.downgrade());
        self.attach(&session, cx);
        session
    }

    fn attach(&mut self, session: &Entity<Session>, cx: &mut Context<Self>) {
        let (id, seq) =
            session.read_with(cx, |session, _| (session.id.clone(), session.mirror.seq));
        let mut params = json!({ "sessionId": id });
        if seq > 0 {
            params["afterSeq"] = json!(seq);
        }
        // Not through the mirror: the answer says how the session catches up, and is read here.
        let request = self.next_id;
        self.next_id += 1;
        let (reply, answer) = async_channel::bounded(1);
        self.waiting.insert(
            request,
            Waiting {
                reply,
                session: None,
            },
        );
        self.tell(json!({ "jsonrpc": "2.0", "id": request, "method": ext::METHOD_SESSION_ATTACH, "params": params }));
        let session = session.downgrade();
        cx.spawn(async move |client, cx| {
            let attached = answer.recv().await.unwrap_or_else(|_| Err(Refusal::lost()));
            match attached {
                Ok(attached) => {
                    session
                        .update(cx, |session, cx| {
                            session.open_elsewhere = attached
                                .get("openElsewhere")
                                .filter(|held| held.is_object())
                                .map(holder);
                            session.ready = true;
                            cx.notify();
                        })
                        .ok();
                }
                // A session the daemon no longer knows was deleted while nobody looked.
                Err(refusal) if refusal.code == -32602 => {
                    client.update(cx, |client, cx| client.forget(&id, cx)).ok();
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    fn forget(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.sessions
            .retain(|known| known.get("sessionId").and_then(Value::as_str) != Some(session_id));
        if let Some(session) = self
            .followed
            .remove(session_id)
            .and_then(|session| session.upgrade())
        {
            session.update(cx, |session, cx| {
                session.removed = true;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Creates a session in a folder and follows it. `agent_id` chooses the agent; `None` is the
    /// daemon's first.
    pub fn new_session(
        &mut self,
        cwd: &str,
        agent_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Session>, Refusal>> {
        let mut params = json!({ "cwd": cwd, "mcpServers": [] });
        if let Some(agent_id) = agent_id {
            params["_meta"] = json!({ "archipel": { "agentId": agent_id } });
        }
        self.created(params, cx)
    }

    /// Makes a copy of a session to continue with and follows it. The original is left as it is.
    pub fn fork_session(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Session>, Refusal>> {
        // Nothing of the original changes, so its mirror is left alone.
        let (id, message) = self.request("session/fork", json!({ "sessionId": session_id }));
        let forked = self.dispatch(id, message, None, cx);
        self.following(forked, cx)
    }

    fn created(
        &mut self,
        params: Value,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Session>, Refusal>> {
        let created = self.call("session/new", params, cx);
        self.following(created, cx)
    }

    /// Follows the session a request made, once it answers with its id.
    fn following(
        &mut self,
        created: Task<Result<Value, Refusal>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Session>, Refusal>> {
        cx.spawn(async move |client, cx| {
            let created = created.await?;
            let id = created
                .get("sessionId")
                .and_then(Value::as_str)
                .ok_or_else(|| Refusal {
                    code: 0,
                    message: "the agent did not return a session id".to_owned(),
                })?;
            client
                .update(cx, |client, cx| client.session(id, cx))
                .map_err(|_| Refusal::lost())
        })
    }

    /// Renames a session. Only where the agent's capabilities have `session.title`.
    pub fn set_title(&mut self, session_id: &str, title: &str, cx: &mut Context<Self>) -> Outcome {
        outcome(
            self.call(
                "session/set_title",
                json!({ "sessionId": session_id, "title": title }),
                cx,
            ),
            cx,
        )
    }

    /// Deletes a session at its harness and in the daemon. It cannot be undone, and needs the
    /// `admin` scope (the local socket has it).
    pub fn delete_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> Outcome {
        outcome(
            self.call("session/delete", json!({ "sessionId": session_id }), cx),
            cx,
        )
    }

    /// Stops following a session. It keeps running in the daemon.
    pub fn detach(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if self.followed.remove(session_id).is_some() {
            self.call(
                ext::METHOD_SESSION_DETACH,
                json!({ "sessionId": session_id }),
                cx,
            )
            .detach();
        }
    }
}

/// A request's result as the kit's controls want it: nothing, or why it failed.
fn outcome(asked: Task<Result<Value, Refusal>>, cx: &App) -> Outcome {
    cx.background_spawn(async move { asked.await.map(|_| ()).map_err(|refusal| refusal.message) })
}

fn holder(held: &Value) -> OpenElsewhere {
    OpenElsewhere {
        pid: held.get("pid").and_then(Value::as_u64),
        status: held
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// A session the app follows. What it holds is what the daemon has, up to its sequence number.
pub struct Session {
    pub id: String,
    client: WeakEntity<Client>,
    mirror: Mirror,
    /// Who else has the session open, as far as the daemon last said. While it does, the daemon
    /// refuses prompts and setting changes; [`Session::fork`] makes a copy to continue with.
    pub open_elsewhere: Option<OpenElsewhere>,
    /// Something went wrong outside any request, as far as the daemon last said. Cleared by the
    /// next change that arrives.
    pub error: Option<String>,
    /// The session was deleted, by this app or another. What it held stays readable.
    pub removed: bool,
    /// The daemon has answered the attach: what is held is the session, or will be once the
    /// replay that follows has arrived.
    pub ready: bool,
}

impl Session {
    /// The session and its subagents' sessions.
    #[must_use]
    pub fn conversation(&self) -> &Conversation {
        &self.mirror.conversation
    }

    /// `running`, `requires_action` or `idle`.
    #[must_use]
    pub fn turn_state(&self) -> Option<&str> {
        self.mirror
            .conversation
            .sessions
            .get(&self.id)?
            .values
            .get("state")?
            .get("state")?
            .as_str()
    }

    fn received(&mut self, rules: &RuleSet, message: &Value, cx: &mut Context<Self>) {
        let before = self.mirror.seq;
        // The answer to an attach that replays means: start from nothing.
        if message.get("method").is_none()
            && message.pointer("/result/catchUp").and_then(Value::as_str) == Some("replay")
        {
            self.mirror.reset();
        }
        self.mirror.received(rules, message).ok();
        if self.mirror.seq > before {
            self.error = None;
        }
        cx.notify();
    }

    fn ask(&mut self, method: &str, mut params: Value, cx: &mut Context<Self>) -> Outcome {
        params["sessionId"] = json!(self.id);
        let Some(client) = self.client.upgrade() else {
            return Task::ready(Err(Refusal::lost().message));
        };
        let (id, message) = client.update(cx, |client, _| client.request(method, params));
        // The mirror notes what was asked, to apply the answer when it comes.
        let rules = client.read(cx).rules.clone();
        self.mirror.sent(&rules, &message).ok();
        let session = Some(self.id.clone());
        let asked = client.update(cx, |client, cx| client.dispatch(id, message, session, cx));
        cx.spawn(async move |session, cx| {
            let result = asked.await;
            session
                .update(cx, |session, cx| {
                    session.open_elsewhere = match &result {
                        Err(refusal) if refusal.open_elsewhere() => {
                            Some(session.open_elsewhere.clone().unwrap_or_default())
                        }
                        _ => None,
                    };
                    cx.notify();
                })
                .ok();
            match result {
                Ok(_) => Ok(()),
                // The notice of the chat already says it; the composer adds nothing.
                Err(refusal) if refusal.open_elsewhere() => Err(String::new()),
                Err(refusal) => Err(refusal.message),
            }
        })
    }

    /// Sends a message to the agent. Done when the agent has taken it, not when the turn is over.
    pub fn prompt(&mut self, content: Vec<Value>, cx: &mut Context<Self>) -> Outcome {
        self.ask("session/prompt", json!({ "prompt": content }), cx)
    }

    /// Changes a setting: one of its values, or a switch on or off.
    pub fn set_config_option(
        &mut self,
        config_id: &str,
        value: SettingValue,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let (kind, value) = match value {
            SettingValue::Choice(choice) => ("id", json!(choice)),
            SettingValue::On(on) => ("boolean", json!(on)),
        };
        self.ask(
            "session/set_config_option",
            json!({ "configId": config_id, "type": kind, "value": value }),
            cx,
        )
    }

    /// The id of a request as it travels: a number when it is one.
    fn request_id(request_id: &str) -> Value {
        request_id
            .parse::<u64>()
            .map_or_else(|_| json!(request_id), |number| json!(number))
    }

    /// Answers a request with a raw result.
    pub fn answer(&mut self, request_id: &str, result: Value, cx: &mut Context<Self>) {
        if let Some(client) = self.client.upgrade() {
            client.read_with(cx, |client, _| client.tell(json!({ "jsonrpc": "2.0", "id": Self::request_id(request_id), "result": result })));
        }
    }

    /// Answers an approval with one of its options.
    pub fn approve(&mut self, request_id: &str, option_id: &str, cx: &mut Context<Self>) {
        self.answer(
            request_id,
            json!({ "outcome": { "outcome": "selected", "optionId": option_id } }),
            cx,
        );
    }

    /// Interrupts the running turn.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(client) = self.client.upgrade() {
            client.read_with(cx, |client, _| client.tell(json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": self.id } })));
        }
    }

    /// Makes a copy of this session to continue with, and follows it.
    pub fn fork(&mut self, cx: &mut Context<Self>) -> Task<Result<Entity<Session>, Refusal>> {
        match self.client.upgrade() {
            Some(client) => client.update(cx, |client, cx| client.fork_session(&self.id, cx)),
            None => Task::ready(Err(Refusal::lost())),
        }
    }
}

/// Called with the copy of a session, once it was forked, to show it.
pub type OnFork = Rc<dyn Fn(Entity<Session>, &mut Window, &mut App)>;

/// What a chat does to a session: every control of the kit, wired. With `on_fork` the chat
/// offers the fork when another process has the session open.
///
/// The actions hold the session: it is followed for as long as the chat that has them lives.
pub fn actions(session: &Entity<Session>, on_fork: Option<OnFork>) -> ChatActions {
    ChatActions {
        approve: Some(Rc::new({
            let session = session.clone();
            move |request, option, _, cx| {
                session.update(cx, |session, cx| session.approve(request, option, cx))
            }
        })),
        answer: Some(Rc::new({
            let session = session.clone();
            move |request, content, _, cx| {
                let result = content.map_or_else(
                    || json!({ "action": "decline" }),
                    |content| json!({ "action": "accept", "content": content }),
                );
                session.update(cx, |session, cx| session.answer(request, result, cx));
            }
        })),
        prompt: Some(Rc::new({
            let session = session.clone();
            move |content, _, cx| session.update(cx, |session, cx| session.prompt(content, cx))
        })),
        cancel: Some(Rc::new({
            let session = session.clone();
            move |_, cx| session.update(cx, |session, cx| session.cancel(cx))
        })),
        set_option: Some(Rc::new({
            let session = session.clone();
            move |id, value, _, cx| {
                session.update(cx, |session, cx| session.set_config_option(id, value, cx))
            }
        })),
        fork: on_fork.map(|on_fork| {
            let session = session.clone();
            Rc::new(move |window: &mut Window, cx: &mut App| {
                let forked = session.update(cx, |session, cx| session.fork(cx));
                let on_fork = on_fork.clone();
                window.spawn(cx, async move |cx| {
                    let copy = forked.await.map_err(|refusal| refusal.message)?;
                    cx.update(|window, cx| on_fork(copy, window, cx))
                        .map_err(|_| Refusal::lost().message)
                })
            }) as archipel_gpui::status::Fork
        }),
    }
}

/// A chat on a followed session: it draws again after every change, and its controls act on
/// the session. The chat keeps the session followed; drop the chat and it is let go.
pub fn live_chat(
    session: &Entity<Session>,
    on_fork: Option<OnFork>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ChatView> {
    let actions = actions(session, on_fork);
    let shown = session.clone();
    cx.new(|cx| {
        let mut chat = ChatView::new(shown.read(cx).conversation().clone(), actions, window, cx);
        chat.set_session(Some(shown.read(cx).id.clone()), cx);
        cx.observe(&shown, |chat: &mut ChatView, session, cx| {
            let (conversation, held, error) = {
                let session = session.read(cx);
                (
                    session.conversation().clone(),
                    session.open_elsewhere.clone(),
                    session.error.clone(),
                )
            };
            chat.set_conversation(conversation, cx);
            chat.set_open_elsewhere(held, cx);
            chat.set_error(error, cx);
        })
        .detach();
        chat
    })
}

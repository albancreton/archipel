//! The daemon's single task: it owns all state and handles one event at a time.
//!
//! What it does with each kind of message:
//!
//! | From | Message | Action |
//! |---|---|---|
//! | adapter | `session/update` | number it, append it to the log, send it to every attached app |
//! | adapter | a request (approval, question) | number and log it, send it to every attached app, remember it as pending |
//! | app | an answer to a pending request | first answer wins: pass it to the adapter, log it, tell every app |
//! | app | `_bridge/session/attach` | send what the app missed (or a full replay), then live messages |
//! | app | `_bridge/sessions/list` | the daemon's sessions, plus those each harness reports |
//! | app | `session/new` | pass to the adapter, record the session, attach the app |
//! | app | `session/delete` | pass to the adapter; once it agrees, drop the session and its log and tell every app |
//! | app | any other request for a session | pass to the adapter, waking the session first if needed |
//!
//! A session is *live* while an adapter process holds it. A session that is not live can still be
//! attached to and replayed from the log; the first request for it restarts the adapter and
//! resumes the session there.
//!
//! A session the daemon has never seen (started in the harness's own UI) is *adopted* the first
//! time an app opens it: the adapter replays its history, which becomes the session's log.
//!
//! What an app may do depends on its scope: `observe` reads, `interact` also acts.
//!
//! An *addon* is code compiled into the daemon that sees every message to and from an adapter,
//! may change, stop, hold or answer it, and may send any of them itself (`bridge-addon-api`).
//! Addons are layers: the first added is nearest the apps. Four pairs of functions take the
//! messages through them:
//!
//! | Message | Through the layers | Then |
//! |---|---|---|
//! | a request to an adapter | [`Daemon::dispatch`] | [`Daemon::settle`] takes its outcome back |
//! | a request made of the user | [`Daemon::ask`] | [`Daemon::answered`] takes the answer back |
//! | a notification or an update, either way | [`Daemon::pass`] | |
//! | an app's call of a method under `_x/` | [`Daemon::serve`] | |
//! | anything between an app and the daemon | [`Daemon::edge`] | |
//!
//! The last one is the app side: [`Daemon::client_message`] and [`Daemon::send`] are the only
//! ways in from and out to an app, and both go through it. Since a send can run addon code,
//! nothing after a send may assume a session is still there.
//!
//! A message an addon holds waits in `held` until [`Daemon::release`] sends it on from the
//! layer that held it.
//!
//! An *extension* is a connection that registered a namespace. It adds updates of its own kinds
//! (`x.<namespace>.…`) to sessions and serves its own methods (`_x/<namespace>/…`). The daemon
//! stores and carries both without knowing what they mean.

use crate::wire;
use bridge_addon_api as addon;
use bridge_agents::{AgentSpec, initialize_params};
use bridge_core::{
    log::{LogEntry, Record, condense},
    state::SessionState,
    store::{SessionRecord, Store, rfc3339},
};
use bridge_schema::{
    ext::{self, Scope},
    rules::RuleSet,
};
use bridge_transport::Link;
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::{
    any::Any,
    collections::{BTreeSet, HashMap},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};

/// How many updates for a session the daemon does not know yet are kept while `session/new` is
/// still being answered.
const MAX_EARLY_UPDATES: usize = 1000;

pub(crate) enum Event {
    ClientOpened {
        client: u64,
        scope: Scope,
        tx: mpsc::Sender<Value>,
    },
    FromClient {
        client: u64,
        message: Value,
    },
    ClientClosed {
        client: u64,
    },
    FromAgent {
        agent: String,
        generation: u64,
        message: Value,
    },
    AgentClosed {
        agent: String,
        generation: u64,
    },
    /// Time for housekeeping.
    Tick,
    /// The time an addon asked to be woken after has passed.
    Timer {
        addon: usize,
        agent: String,
        id: u64,
    },
    Shutdown(oneshot::Sender<()>),
}

pub(crate) struct Daemon {
    rules: RuleSet,
    store: Box<dyn Store>,
    /// For the tasks that read from adapters.
    events: mpsc::Sender<Event>,
    agents: IndexMap<String, Agent>,
    /// Sessions the daemon has in memory, by root session id.
    sessions: HashMap<String, Session>,
    /// Every known session id, including subagents' child sessions, to its root session.
    roots: HashMap<String, String>,
    /// Pending request id to its root session.
    requests: HashMap<String, String>,
    clients: HashMap<u64, Client>,
    /// Counts adapter starts, to tell a restarted adapter's messages from its predecessor's.
    generations: u64,
    /// Sessions a harness reported that are not in the log, by session id.
    harness: HashMap<String, HarnessSession>,
    /// `_bridge/sessions/list` requests waiting for adapters to answer.
    lists: HashMap<u64, ListJob>,
    next_list: u64,
    /// Sessions untouched for longer than this many milliseconds are removed from the log.
    retention: Option<u64>,
    /// Act on sessions that another process of the harness has open, instead of refusing.
    share: bool,
    /// Registered extensions: namespace to the connection that serves it.
    extensions: HashMap<String, u64>,
    /// Requests passed to an extension: our id to the extension, the asking app and its id.
    extension_calls: HashMap<String, (u64, u64, Value)>,
    next_extension_call: u64,
    /// Code between the daemon and the adapters, in the order it was added: the first is
    /// nearest the apps. A slot is empty while its addon's hook runs. See [`addon`].
    addons: Vec<Option<Box<dyn addon::Addon>>>,
    /// The last number given to a message or to something an addon started, for addons to
    /// tell them apart.
    ids: u64,
    /// Messages an addon holds, by the number the daemon gave them.
    held: HashMap<u64, Held>,
    /// App requests the daemon has not answered yet, by connection and JSON-RPC id: the
    /// number addons know each by, its method and its params. Only kept when there are addons.
    asked: HashMap<(u64, String), (u64, String, Value)>,
    /// Requests addons made of the daemon as an app would, by the connection number that
    /// stands for the addon and the id it was given: the agent the addon was looking at.
    app_calls: HashMap<(u64, u64), String>,
}

/// Connection numbers from here on stand for addons that act as an app
/// ([`addon::Cx::app_call`]): this plus the addon's layer.
const ADDON_CLIENTS: u64 = 1 << 62;

/// A message between an app and the daemon, on its way through the addons.
struct Edge {
    /// The number addons know it by.
    id: u64,
    client: u64,
    kind: EdgeKind,
    /// The whole JSON-RPC message.
    message: Value,
}

enum EdgeKind {
    /// From the app.
    Request,
    Notification,
    Answer,
    /// To the app: the answer to its request, with that request's method and params.
    Response(String, Value),
    /// To the app: a notification or a request.
    Push,
}

impl EdgeKind {
    fn inward(&self) -> bool {
        matches!(self, Self::Request | Self::Notification | Self::Answer)
    }
}

fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Observe => "observe",
        Scope::Interact => "interact",
        Scope::Extend => "extend",
        Scope::Admin => "admin",
    }
}

fn step_of_flow(flow: addon::Flow) -> addon::Verdict {
    match flow {
        addon::Flow::Pass => addon::Verdict::Pass,
        addon::Flow::Hold => addon::Verdict::Hold,
    }
}

/// A dropped message is told apart from a passed one by any verdict that ends it.
fn step_of_keep(keep: addon::Keep) -> addon::Verdict {
    match keep {
        addon::Keep::Yes => addon::Verdict::Pass,
        addon::Keep::Hold => addon::Verdict::Hold,
        addon::Keep::No => addon::Verdict::Answer(Value::Null),
    }
}

/// A request to an adapter: why it was made, and what was sent, for the addons that see
/// its answer.
struct Sent {
    call: Call,
    method: String,
    params: Value,
    /// The number addons know it by.
    request: u64,
}

/// Who a message is from, in a form that can be kept.
#[derive(Clone)]
enum Source {
    /// An app, with the name it gave.
    App(Option<String>),
    Daemon,
    Adapter,
    /// The addon at this layer.
    Addon(usize),
}

/// A message nobody answers, on its way through the addons.
struct OneWay {
    /// The number addons know it by.
    id: u64,
    source: Source,
    what: What,
    /// The params, or for a session update its body.
    body: Value,
}

enum What {
    /// Towards the adapter, with its method.
    Notification(String),
    /// A `session/update` towards the apps, with its session id.
    Update(String),
    /// Any other notification towards the apps, with its method.
    AgentNotification(String),
}

/// A request made of the user, by an adapter or by an addon.
struct Ask {
    asker: Asker,
    method: String,
    params: Value,
    /// The number addons know it by.
    request: u64,
}

enum Asker {
    /// With the JSON-RPC id the adapter used, for the answer.
    Adapter(Value),
    /// With the id the addon was given, for [`addon::Addon::reply`].
    Addon { addon: usize, id: u64 },
}

/// The answer to an [`Ask`], on its way back to whoever asked.
struct Answered {
    ask: Ask,
    outcome: addon::Outcome,
    source: Source,
    /// For the log: who answered.
    by: Option<String>,
    /// The request's place in the log, if it has one.
    entry: Option<LogEntry>,
}

/// An app's call of a method under `_x/`.
struct OwnCall {
    client: u64,
    id: Value,
    method: String,
    params: Value,
    /// The number addons know it by.
    request: u64,
}

/// A message the addon at layer `addon` said to keep until it releases it.
struct Held {
    addon: usize,
    agent: String,
    what: HeldWhat,
}

enum HeldWhat {
    Request(Box<Sent>),
    Response(Box<Sent>, addon::Outcome),
    Method(Box<OwnCall>),
    OneWay(Box<OneWay>),
    Edge(Box<Edge>),
    Ask(Box<Ask>),
    Answer(Box<Answered>),
}

/// How an addon ends a hold.
enum Release {
    /// The message goes on, with other content if given.
    Pass(Option<Value>),
    /// A request gets this outcome here; a response or an answer goes on as this.
    Outcome(addon::Outcome),
    Discard,
}

/// The log, as an addon may read it.
struct Logs<'a> {
    store: &'a dyn Store,
    roots: &'a HashMap<String, String>,
}

impl Logs<'_> {
    /// The updates of the session itself, oldest first.
    fn own_updates(&self, session_id: &str) -> Vec<Value> {
        let entries = self
            .roots
            .get(session_id)
            .and_then(|root| self.store.after(root, 0).ok())
            .unwrap_or_default();
        entries
            .iter()
            .filter(|entry| entry.session_id == session_id)
            .filter_map(|entry| entry.update().cloned())
            .collect()
    }
}

impl addon::Sessions for Logs<'_> {
    fn last_update(&self, session_id: &str, kind: &str) -> Option<Value> {
        self.own_updates(session_id)
            .into_iter()
            .rev()
            .find(|update| text(update, "/sessionUpdate") == Some(kind))
    }

    fn updates(&self, session_id: &str) -> Vec<Value> {
        self.own_updates(session_id)
    }

    fn session(&self, session_id: &str) -> Option<addon::SessionInfo> {
        let root = self.roots.get(session_id)?;
        let record = self.store.session(root).ok()??;
        Some(addon::SessionInfo {
            root: root.clone(),
            agent: record.agent_id,
            cwd: record.cwd,
            title: record.title,
            state: record.state,
        })
    }
}

/// A session only the harness knows, as its adapter listed it.
#[derive(Clone)]
struct HarnessSession {
    agent: String,
    cwd: String,
    title: Option<String>,
}

struct ListJob {
    client: u64,
    id: Value,
    sessions: Vec<ext::SessionSummary>,
    /// Adapters that have not finished answering.
    waiting: usize,
    cwd: Option<String>,
}

struct Client {
    scope: Scope,
    tx: mpsc::Sender<Value>,
    /// From the client's `initialize`, used to say who answered a request.
    name: Option<String>,
}

struct Agent {
    spec: AgentSpec,
    running: Option<Adapter>,
    last_error: Option<String>,
}

/// A started adapter process and the daemon's open business with it.
struct Adapter {
    tx: mpsc::Sender<Value>,
    _guard: Option<Box<dyn Any + Send>>,
    generation: u64,
    /// The adapter's `initialize` result. `None` until it has answered.
    initialized: Option<Value>,
    next_id: u64,
    /// Requests the daemon sent and has no answer to yet, by JSON-RPC id.
    calls: HashMap<u64, Sent>,
    /// App requests held back until the adapter has answered `initialize`.
    waiting: Vec<(u64, Value)>,
    /// Updates for a session id not known yet. See [`MAX_EARLY_UPDATES`].
    early: Vec<Value>,
    /// The login state the adapter last reported, such as `authenticated`.
    auth: Option<String>,
}

/// Why the daemon sent a request to an adapter, and what to do with the answer.
enum Call {
    Initialize,
    /// An app's `session/new`.
    New {
        client: u64,
        id: Value,
        agent: String,
        cwd: String,
        setup: Value,
    },
    /// An app's `session/fork`.
    Fork {
        client: u64,
        id: Value,
        agent: String,
        cwd: String,
        setup: Value,
    },
    /// The daemon asking a harness for its sessions, for a [`ListJob`].
    List {
        job: u64,
        agent: String,
    },
    /// The daemon waking a session that is not live, or adopting one.
    Resume {
        root: String,
    },
    /// An app's `session/delete`.
    Delete {
        client: u64,
        id: Value,
        session: String,
    },
    /// Any other app request: the answer goes back to the app unchanged.
    Forward {
        client: u64,
        id: Value,
    },
    /// A request an addon made itself: the answer goes to that addon.
    Addon {
        addon: usize,
        id: u64,
    },
}

struct Session {
    record: SessionRecord,
    /// An adapter process holds the session.
    live: bool,
    /// The daemon has asked the adapter to resume the session and waits for the answer.
    resuming: bool,
    /// The session's history is being loaded from the harness. Until that is done its log is
    /// incomplete, so apps that want it wait.
    adopting: bool,
    /// App requests held back until the session is live again.
    queued: Vec<(u64, Value)>,
    /// Attached apps.
    subscribers: BTreeSet<u64>,
    /// Requests from the agent that nobody has answered, by request id.
    pending: IndexMap<String, Pending>,
}

struct Pending {
    ask: Ask,
    entry: LogEntry,
}

enum Ready {
    Yes,
    /// The request was put aside and comes back when the adapter has started.
    Later,
    No(String),
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

impl Daemon {
    pub(crate) fn new(
        agents: Vec<AgentSpec>,
        store: Box<dyn Store>,
        events: mpsc::Sender<Event>,
        retention: Option<u64>,
        share: bool,
        addons: Vec<Box<dyn addon::Addon>>,
    ) -> Self {
        Self {
            rules: RuleSet::pinned(),
            store,
            events,
            agents: agents
                .into_iter()
                .map(|spec| {
                    let agent = Agent {
                        spec,
                        running: None,
                        last_error: None,
                    };
                    (agent.spec.id.clone(), agent)
                })
                .collect(),
            sessions: HashMap::new(),
            roots: HashMap::new(),
            requests: HashMap::new(),
            clients: HashMap::new(),
            generations: 0,
            harness: HashMap::new(),
            lists: HashMap::new(),
            next_list: 0,
            retention,
            share,
            extensions: HashMap::new(),
            extension_calls: HashMap::new(),
            next_extension_call: 0,
            addons: addons.into_iter().map(Some).collect(),
            ids: 0,
            held: HashMap::new(),
            asked: HashMap::new(),
            app_calls: HashMap::new(),
        }
    }

    pub(crate) async fn run(mut self, mut inbox: mpsc::Receiver<Event>) {
        while let Some(event) = inbox.recv().await {
            match event {
                Event::ClientOpened { client, scope, tx } => {
                    let entry = Client {
                        scope,
                        tx,
                        name: None,
                    };
                    self.clients.insert(client, entry);
                }
                Event::Tick => self.prune(),
                Event::Timer { addon, agent, id } => {
                    self.hook(addon, &agent, false, |addon, cx, _| addon.timer(cx, id));
                }
                Event::FromClient { client, message } => self.client_message(client, message),
                Event::ClientClosed { client } => self.drop_client(client),
                Event::FromAgent {
                    agent,
                    generation,
                    message,
                } => self.agent_message(&agent, generation, message),
                Event::AgentClosed { agent, generation } => {
                    self.agent_stopped(&agent, generation, "the agent process stopped");
                }
                Event::Shutdown(done) => {
                    self.agents.clear();
                    let _ = done.send(());
                    return;
                }
            }
        }
    }

    // ---- sending to apps -------------------------------------------------------------------

    /// Sends a message to an app, through the addons' app-side hooks.
    fn send(&mut self, client: u64, message: Value) {
        if client >= ADDON_CLIENTS {
            return self.answer_addon(client, &message);
        }
        if self.addons.is_empty() {
            return self.queue(client, message);
        }
        let (kind, id) = if message.get("method").is_some() {
            self.ids += 1;
            (EdgeKind::Push, self.ids)
        } else {
            let asked = message
                .get("id")
                .and_then(|id| self.asked.remove(&(client, id.to_string())));
            match asked {
                Some((number, method, params)) => (EdgeKind::Response(method, params), number),
                None => return self.queue(client, message),
            }
        };
        let edge = Edge {
            id,
            client,
            kind,
            message,
        };
        self.edge(self.addons.len(), edge);
    }

    /// Queues a message for an app. An app whose queue is full is disconnected: it can attach
    /// again and catch up from the log, which is cheaper than holding everything for it.
    fn queue(&mut self, client: u64, message: Value) {
        if let Some(entry) = self.clients.get(&client)
            && entry.tx.try_send(message).is_err()
        {
            self.drop_client(client);
        }
    }

    /// The answer to a request an addon made of the daemon as an app would. Whatever else
    /// the daemon would send such an app is dropped.
    fn answer_addon(&mut self, client: u64, message: &Value) {
        let id = message.get("id").and_then(Value::as_u64);
        if message.get("method").is_some() {
            return;
        }
        let Some((id, agent)) = id.and_then(|id| Some((id, self.app_calls.remove(&(client, id))?)))
        else {
            return;
        };
        let outcome = addon::Outcome::of(message);
        let index = (client - ADDON_CLIENTS) as usize;
        self.hook(index, &agent, false, |addon, cx, _| {
            addon.reply(cx, id, &outcome);
        });
    }

    /// A message from an app, as it arrives: it passes the addons' app-side hooks before
    /// the daemon handles it.
    fn client_message(&mut self, client: u64, message: Value) {
        if !self.clients.contains_key(&client) {
            return;
        }
        if self.addons.is_empty() {
            return self.handle(client, message);
        }
        let kind = match (message.get("method").is_some(), message.get("id").is_some()) {
            (true, true) => EdgeKind::Request,
            (true, false) => EdgeKind::Notification,
            (false, true) => EdgeKind::Answer,
            (false, false) => return,
        };
        self.ids += 1;
        let edge = Edge {
            id: self.ids,
            client,
            kind,
            message,
        };
        self.edge(0, edge);
    }

    /// Passes a message between an app and the daemon through the addons, then acts on it.
    /// One from the app starts at layer `next`; one to the app starts below layer `next`.
    fn edge(&mut self, next: usize, mut edge: Edge) {
        let Some(entry) = self.clients.get(&edge.client) else {
            return; // the app is gone
        };
        let (name, scope) = (entry.name.clone(), scope_name(entry.scope));
        let method = text(&edge.message, "/method")
            .unwrap_or_default()
            .to_owned();
        let layers = self.addons.len();
        let order: Vec<usize> = if edge.kind.inward() {
            (next..layers).collect()
        } else {
            (0..next.min(layers)).rev().collect()
        };
        for index in order {
            let about = match &edge.kind {
                EdgeKind::Response(_, params) => params,
                _ => &edge.message["params"],
            };
            let named = text(about, "/sessionId").is_some_and(|id| self.roots.contains_key(id));
            let agent_id = self.agent_for(about);
            let app = addon::App {
                id: edge.client,
                name: name.as_deref(),
                scope,
            };
            let id = edge.id;
            let step = self.hook(index, &agent_id, false, |addon, cx, agent| {
                let agent = named.then_some(agent);
                let message = &mut edge.message;
                match &edge.kind {
                    EdgeKind::Request => {
                        let mut params = message["params"].take();
                        let shown = &mut addon::AppRequest {
                            id,
                            app,
                            agent,
                            method: &method,
                            params: &mut params,
                        };
                        let verdict = addon.app_request(cx, shown);
                        message["params"] = params;
                        verdict
                    }
                    EdgeKind::Response(method, params) => {
                        let mut outcome = addon::Outcome::of(message);
                        let shown = &mut addon::AppResponse {
                            id,
                            app,
                            agent,
                            method,
                            params,
                            outcome: &mut outcome,
                        };
                        let flow = addon.app_response(cx, shown);
                        *message = wire::relay(&message["id"].clone(), &outcome.to_json());
                        step_of_flow(flow)
                    }
                    EdgeKind::Notification => {
                        let mut params = message["params"].take();
                        let shown = &mut addon::AppNotification {
                            id,
                            app,
                            agent,
                            method: &method,
                            params: &mut params,
                        };
                        let keep = addon.app_notification(cx, shown);
                        message["params"] = params;
                        step_of_keep(keep)
                    }
                    EdgeKind::Answer => {
                        let request = message["id"].clone();
                        let mut outcome = addon::Outcome::of(message);
                        let shown = &mut addon::AppAnswer {
                            id,
                            app,
                            request: &request,
                            outcome: &mut outcome,
                        };
                        let keep = addon.app_answer(cx, shown);
                        *message = wire::relay(&request, &outcome.to_json());
                        step_of_keep(keep)
                    }
                    EdgeKind::Push => {
                        let request = message.get("id").cloned();
                        let mut params = message["params"].take();
                        let shown = &mut addon::ToApp {
                            id,
                            app,
                            agent,
                            request: request.as_ref(),
                            method: &method,
                            params: &mut params,
                        };
                        let keep = addon.to_app(cx, shown);
                        message["params"] = params;
                        step_of_keep(keep)
                    }
                }
            });
            let outcome = match step {
                None | Some(addon::Verdict::Pass) => continue,
                Some(addon::Verdict::Hold) => {
                    return self.hold(index, &agent_id, id, HeldWhat::Edge(edge.into()));
                }
                Some(addon::Verdict::Answer(result)) => addon::Outcome::Result(result),
                Some(addon::Verdict::Refuse { code, message }) => addon::Outcome::Error {
                    code,
                    message,
                    data: Some(json!({"addon": self.addon_id(index)})),
                },
            };
            // Only a request is answered. Anything else an addon stops is dropped.
            if matches!(edge.kind, EdgeKind::Request) {
                self.edge_answer(index, edge, &outcome);
            }
            return;
        }
        match edge.kind {
            EdgeKind::Request => {
                let key = (edge.client, edge.message["id"].to_string());
                let params = edge.message["params"].clone();
                self.asked.insert(key, (edge.id, method, params));
                self.handle(edge.client, edge.message);
            }
            EdgeKind::Notification | EdgeKind::Answer => self.handle(edge.client, edge.message),
            EdgeKind::Response(..) | EdgeKind::Push => self.queue(edge.client, edge.message),
        }
    }

    /// The addon at layer `index` answers an app's request itself. The layers nearer the app
    /// see that answer as they would the daemon's.
    fn edge_answer(&mut self, index: usize, mut request: Edge, outcome: &addon::Outcome) {
        let method = text(&request.message, "/method")
            .unwrap_or_default()
            .to_owned();
        let params = request.message["params"].take();
        let response = Edge {
            id: request.id,
            client: request.client,
            kind: EdgeKind::Response(method, params),
            message: wire::relay(&request.message["id"], &outcome.to_json()),
        };
        self.edge(index, response);
    }

    fn reply(&mut self, client: u64, id: &Value, result: Value) {
        self.send(client, wire::result(id, result));
    }

    fn refuse(&mut self, client: u64, id: &Value, code: i64, message: &str) {
        self.send(client, wire::error(id, code, message));
    }

    fn drop_client(&mut self, client: u64) {
        self.clients.remove(&client);
        for session in self.sessions.values_mut() {
            session.subscribers.remove(&client);
        }
        self.asked.retain(|(asker, _), _| *asker != client);
        self.held
            .retain(|_, held| !matches!(&held.what, HeldWhat::Edge(edge) if edge.client == client));
        // If it was an extension, its namespace is free again and what waited on it fails.
        self.extensions.retain(|_, serving| *serving != client);
        let orphaned: Vec<String> = self
            .extension_calls
            .iter()
            .filter(|(_, (extension, _, _))| *extension == client)
            .map(|(id, _)| id.clone())
            .collect();
        for call in orphaned {
            if let Some((_, asker, id)) = self.extension_calls.remove(&call) {
                self.refuse(asker, &id, wire::INTERNAL_ERROR, "the extension went away");
            }
        }
    }

    // ---- messages from apps ----------------------------------------------------------------

    /// Handles a message from an app, or from an addon acting as one.
    fn handle(&mut self, client: u64, message: Value) {
        if client < ADDON_CLIENTS && !self.clients.contains_key(&client) {
            return;
        }
        let may_act = self.scope(client) >= Scope::Interact;
        let method = text(&message, "/method").map(str::to_owned);
        match (method, message.get("id").cloned()) {
            (Some(method), Some(id)) => self.client_request(client, &method, id, message),
            // Notifications and answers act on a session. From an observer they are ignored.
            (Some(_), None) if may_act => self.client_notification(client, &message),
            (None, Some(id)) if may_act => self.client_answer(client, &id, &message),
            _ => {}
        }
    }

    fn scope(&self, client: u64) -> Scope {
        if client >= ADDON_CLIENTS {
            return Scope::Extend;
        }
        self.clients
            .get(&client)
            .map_or(Scope::Observe, |entry| entry.scope)
    }

    /// Refuses a request the app's scope does not allow. Returns whether it was allowed.
    fn allowed(&mut self, client: u64, id: &Value, needed: Scope) -> bool {
        let allowed = self.scope(client) >= needed;
        if !allowed {
            self.refuse(
                client,
                id,
                wire::FORBIDDEN,
                "this connection may only observe",
            );
        }
        allowed
    }

    fn client_request(&mut self, client: u64, method: &str, id: Value, message: Value) {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let reads = matches!(
            method,
            "initialize"
                | "session/resume"
                | ext::METHOD_AGENTS_LIST
                | ext::METHOD_SESSIONS_LIST
                | ext::METHOD_SESSION_ATTACH
                | ext::METHOD_SESSION_DETACH
        );
        // Everything else acts on a session or reaches an agent.
        if !reads && !self.allowed(client, &id, Scope::Interact) {
            return;
        }
        match method {
            "initialize" => {
                if let (Some(name), Some(entry)) =
                    (text(&params, "/info/name"), self.clients.get_mut(&client))
                {
                    entry.name = Some(name.to_owned());
                }
                // The capabilities an app sees are those of the default agent, so it has to be up.
                let ready = match self.agents.keys().next().cloned() {
                    Some(agent) => self.ensure_ready(&agent, client, &message),
                    None => Ready::No("no agent is configured".to_owned()),
                };
                if !matches!(ready, Ready::Later) {
                    let result = self.initialize_result(client);
                    self.reply(client, &id, result);
                }
            }
            ext::METHOD_AGENTS_LIST => {
                let agents = self.agent_infos();
                self.reply(client, &id, json!({ "agents": agents }));
            }
            ext::METHOD_SESSIONS_LIST => self.list_sessions(client, &id, &message),
            ext::METHOD_SESSION_ATTACH => self.attach(client, &id, &message),
            ext::METHOD_SESSION_DETACH => {
                if let Some(root) = text(&params, "/sessionId").and_then(|id| self.roots.get(id))
                    && let Some(session) = self.sessions.get_mut(root)
                {
                    session.subscribers.remove(&client);
                }
                self.reply(client, &id, json!({}));
            }
            "session/resume" => self.resume_for_client(client, &id, &message),
            ext::METHOD_EXTENSION_REGISTER => self.register_extension(client, &id, &params),
            ext::METHOD_SESSION_APPEND => self.append(client, &id, &message),
            _ if method.starts_with("_x/") => self.own_method(client, method, id, params),
            "session/new" => self.new_session(client, id, &message),
            "session/fork" => self.fork(client, id, &message),
            "session/delete" => self.delete(client, id, &message),
            _ => match text(&params, "/sessionId").map(str::to_owned) {
                Some(session_id) => self.forward(client, id, &session_id, message),
                None => self.forward_to_default_agent(client, id, &message),
            },
        }
    }

    /// What the daemon answers to an app's `initialize`: the default agent's capabilities under
    /// the daemon's own name. Without a working agent the capabilities are empty, and the app
    /// can still list and read sessions.
    fn initialize_result(&self, client: u64) -> Value {
        let agent = self
            .agents
            .values()
            .next()
            .and_then(|agent| agent.running.as_ref())
            .and_then(|adapter| adapter.initialized.as_ref());
        json!({
            "protocolVersion": 2,
            "info": {"name": "harness-bridge", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": agent.and_then(|result| result.get("capabilities")).cloned().unwrap_or_else(|| json!({})),
            "authMethods": [],
            "_meta": {ext::META_KEY: {
                "extensions": [
                    ext::METHOD_AGENTS_LIST,
                    ext::METHOD_SESSIONS_LIST,
                    ext::METHOD_SESSION_ATTACH,
                    ext::METHOD_SESSION_DETACH,
                    ext::NOTIFICATION_REQUEST_RESOLVED,
                    ext::NOTIFICATION_SESSIONS_UPDATED,
                    ext::NOTIFICATION_SESSIONS_REMOVED,
                ],
                "scope": self.scope(client),
                "namespaces": self.extensions.keys().collect::<Vec<_>>(),
                "agent": agent.and_then(|result| result.get("info")),
            }},
        })
    }

    fn agent_infos(&self) -> Vec<ext::AgentInfo> {
        self.agents
            .values()
            .map(|agent| {
                let info = agent
                    .running
                    .as_ref()
                    .and_then(|adapter| adapter.initialized.as_ref());
                let failed = agent.running.is_none() && agent.last_error.is_some();
                ext::AgentInfo {
                    agent_id: agent.spec.id.clone(),
                    name: agent.spec.name.clone(),
                    version: info
                        .and_then(|result| text(result, "/info/version"))
                        .map(str::to_owned),
                    readiness: if failed {
                        ext::AgentReadiness::Error
                    } else if agent.running.as_ref().and_then(|a| a.auth.as_deref())
                        == Some("unauthenticated")
                    {
                        ext::AgentReadiness::NeedsLogin
                    } else {
                        ext::AgentReadiness::Ready
                    },
                    detail: agent.last_error.clone().filter(|_| failed),
                }
            })
            .collect()
    }

    fn summary(&self, record: &SessionRecord) -> ext::SessionSummary {
        ext::SessionSummary {
            live: self
                .sessions
                .get(&record.session_id)
                .is_some_and(|session| session.live),
            logged: true,
            open_elsewhere: self
                .elsewhere(&record.agent_id, &record.session_id)
                .is_some(),
            updated_at: Some(rfc3339(record.updated_at)),
            session_id: record.session_id.clone(),
            agent_id: record.agent_id.clone(),
            title: record.title.clone(),
            cwd: record.cwd.clone(),
            state: record.state.clone(),
            head_seq: record.head_seq,
        }
    }

    /// Tells every connected app that a session appeared or that its summary changed.
    fn announce(&mut self, root: &str) {
        let Some(session) = self.sessions.get(root) else {
            return;
        };
        let summary = self.summary(&session.record);
        let message = wire::notification(
            ext::NOTIFICATION_SESSIONS_UPDATED,
            json!({ "session": summary }),
        );
        let clients: Vec<u64> = self.clients.keys().copied().collect();
        for client in clients {
            self.send(client, message.clone());
        }
    }

    /// Lists the daemon's sessions and, unless asked not to, those each harness reports.
    fn list_sessions(&mut self, client: u64, id: &Value, message: &Value) {
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let Ok(request) = serde_json::from_value::<ext::ListSessionsRequest>(params) else {
            return self.refuse(client, id, wire::INVALID_PARAMS, "unexpected params");
        };
        let records = match self.store.sessions() {
            Ok(records) => records,
            Err(error) => {
                return self.refuse(client, id, wire::INTERNAL_ERROR, &error.to_string());
            }
        };
        let wanted = |agent: &str, cwd: &str| {
            request.agent_id.as_deref().is_none_or(|only| only == agent)
                && request.cwd.as_deref().is_none_or(|only| only == cwd)
        };
        let sessions: Vec<ext::SessionSummary> = records
            .iter()
            .filter(|record| wanted(&record.agent_id, &record.cwd))
            .map(|record| self.summary(record))
            .collect();
        // Ask every agent that can be reached. One that cannot start is left out.
        let mut asked = Vec::new();
        if !request.logged_only {
            let agents: Vec<String> = self
                .agents
                .keys()
                .filter(|agent| {
                    request
                        .agent_id
                        .as_deref()
                        .is_none_or(|only| only == *agent)
                })
                .cloned()
                .collect();
            for agent in agents {
                match self.ensure_ready(&agent, client, message) {
                    Ready::Yes => asked.push(agent),
                    Ready::Later => return, // comes back when that adapter is up
                    Ready::No(_) => {}
                }
            }
        }
        let job = self.next_list;
        self.next_list += 1;
        self.lists.insert(
            job,
            ListJob {
                client,
                id: id.clone(),
                sessions,
                waiting: asked.len(),
                cwd: request.cwd.clone(),
            },
        );
        if asked.is_empty() {
            return self.finish_list(job);
        }
        for agent in asked {
            self.ask_for_sessions(job, &agent, None);
        }
    }

    fn ask_for_sessions(&mut self, job: u64, agent: &str, cursor: Option<&str>) {
        let Some(list) = self.lists.get(&job) else {
            return;
        };
        let mut params = json!({});
        if let Some(cwd) = &list.cwd {
            params["cwd"] = cwd.as_str().into();
        }
        if let Some(cursor) = cursor {
            params["cursor"] = cursor.into();
        }
        let call = Call::List {
            job,
            agent: agent.to_owned(),
        };
        self.call(agent, call, "session/list", params);
    }

    /// One adapter answered (or failed) a list request.
    fn sessions_listed(&mut self, job: u64, agent: &str, response: &Value) {
        let Some(list) = self.lists.get_mut(&job) else {
            return;
        };
        let found = response
            .pointer("/result/sessions")
            .and_then(Value::as_array);
        for session in found.into_iter().flatten() {
            let (Some(session_id), Some(cwd)) =
                (text(session, "/sessionId"), text(session, "/cwd"))
            else {
                continue;
            };
            if self.roots.contains_key(session_id)
                || list
                    .sessions
                    .iter()
                    .any(|known| known.session_id == session_id)
                || matches!(self.store.session(session_id), Ok(Some(_)))
            {
                continue; // already in the log
            }
            let title = text(session, "/title").map(str::to_owned);
            self.harness.insert(
                session_id.to_owned(),
                HarnessSession {
                    agent: agent.to_owned(),
                    cwd: cwd.to_owned(),
                    title: title.clone(),
                },
            );
            list.sessions.push(ext::SessionSummary {
                session_id: session_id.to_owned(),
                agent_id: agent.to_owned(),
                title,
                cwd: cwd.to_owned(),
                live: false,
                logged: false,
                open_elsewhere: self
                    .agents
                    .get(agent)
                    .and_then(|known| known.spec.launcher.elsewhere(session_id))
                    .is_some(),
                state: None,
                head_seq: 0,
                updated_at: text(session, "/updatedAt").map(str::to_owned),
            });
        }
        if let Some(cursor) = text(response, "/result/nextCursor") {
            return self.ask_for_sessions(job, agent, Some(cursor));
        }
        list.waiting = list.waiting.saturating_sub(1);
        if list.waiting == 0 {
            self.finish_list(job);
        }
    }

    fn finish_list(&mut self, job: u64) {
        let Some(mut list) = self.lists.remove(&job) else {
            return;
        };
        list.sessions
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.reply(list.client, &list.id, json!({ "sessions": list.sessions }));
    }

    /// Gets a session ready for an app: finds it in memory or in the store, or starts adopting
    /// it from its harness. Returns the root session id when the app's request can go ahead.
    /// Otherwise the request has been refused, or put aside to come back later.
    fn open(
        &mut self,
        client: u64,
        id: &Value,
        session_id: &str,
        message: &Value,
    ) -> Option<String> {
        if let Some(root) = self.load(session_id) {
            if let Some(session) = self.sessions.get_mut(&root)
                && session.adopting
            {
                session.queued.push((client, message.clone()));
                return None;
            }
            return Some(root);
        }
        let Some(known) = self.harness.get(session_id).cloned() else {
            self.refuse(
                client,
                id,
                wire::INVALID_PARAMS,
                "unknown session; list the sessions first",
            );
            return None;
        };
        // Adopting starts the harness, so reading alone does not allow it.
        if !self.allowed(client, id, Scope::Interact) {
            return None;
        }
        match self.ensure_ready(&known.agent, client, message) {
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, id, wire::INTERNAL_ERROR, &why),
            Ready::Yes => {
                let at = now();
                let record = SessionRecord {
                    session_id: session_id.to_owned(),
                    agent_id: known.agent.clone(),
                    cwd: known.cwd,
                    title: known.title,
                    state: None,
                    setup: json!({}),
                    head_seq: 0,
                    created_at: at,
                    updated_at: at,
                };
                if let Err(error) = self.store.put_session(&record) {
                    self.refuse(client, id, wire::INTERNAL_ERROR, &error.to_string());
                    return None;
                }
                let params = resume_params(&record, true);
                self.harness.remove(session_id);
                self.roots
                    .insert(session_id.to_owned(), session_id.to_owned());
                self.sessions.insert(
                    session_id.to_owned(),
                    Session {
                        record,
                        live: false,
                        resuming: true,
                        adopting: true,
                        queued: vec![(client, message.clone())],
                        subscribers: BTreeSet::new(),
                        pending: IndexMap::new(),
                    },
                );
                let call = Call::Resume {
                    root: session_id.to_owned(),
                };
                self.call(&known.agent, call, "session/resume", params);
            }
        }
        None
    }

    /// Forgets a session entirely: memory and store.
    fn forget(&mut self, root: &str) {
        self.sessions.remove(root);
        self.roots.retain(|_, of| of != root);
        self.requests.retain(|_, of| of != root);
        if let Err(error) = self.store.delete_session(root) {
            eprintln!("bridge: {error}");
        }
    }

    /// Removes sessions nobody has touched for longer than the retention time. The harness
    /// still has them, so they stay listed and can be adopted again.
    fn prune(&mut self) {
        let Some(retention) = self.retention else {
            return;
        };
        let limit = now().saturating_sub(retention);
        let old: Vec<String> = self
            .store
            .sessions()
            .unwrap_or_default()
            .into_iter()
            .filter(|record| record.updated_at < limit)
            .map(|record| record.session_id)
            .filter(|id| {
                self.sessions
                    .get(id)
                    .is_none_or(|session| !session.live && session.subscribers.is_empty())
            })
            .collect();
        for root in old {
            self.forget(&root);
        }
    }

    fn attach(&mut self, client: u64, id: &Value, message: &Value) {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Ok(request) = serde_json::from_value::<ext::AttachSessionRequest>(params) else {
            return self.refuse(client, id, wire::INVALID_PARAMS, "expected a sessionId");
        };
        let Some(root) = self.open(client, id, &request.session_id, message) else {
            return;
        };
        // A hook may have run since: the session can be gone.
        let Some(session) = self.sessions.get(&root) else {
            return;
        };
        let head = session.record.head_seq;
        let agent = session.record.agent_id.clone();
        // A position beyond the log's end is from another log: start over.
        let after = request.after_seq.filter(|&seq| seq <= head);
        let catch_up = if after.is_some() {
            ext::CatchUp::Delta
        } else {
            ext::CatchUp::Replay
        };
        self.reply(
            client,
            id,
            json!(ext::AttachSessionResponse {
                catch_up,
                head_seq: head,
                open_elsewhere: self.held_elsewhere(&agent, &root),
            }),
        );
        self.catch_up(client, &root, after);
        if let Some(session) = self.sessions.get_mut(&root) {
            session.subscribers.insert(client);
        }
    }

    /// Sends an app what it has not seen: the entries after `after`, or with `None` the whole
    /// session ending in `replay_complete`. Pending requests are always sent (again), because an
    /// app can only answer a request it received on its current connection.
    fn catch_up(&mut self, client: u64, root: &str, after: Option<u64>) {
        let from = after.unwrap_or(0);
        let entries = self.store.after(root, from).unwrap_or_default();
        let Some(session) = self.sessions.get(root) else {
            return;
        };
        let mut messages = Vec::new();
        for mut entry in condense(&self.rules, &entries) {
            if let Record::Request {
                request_id,
                method,
                outcome: outcome @ None,
                ..
            } = &mut entry.record
                && !session.pending.contains_key(request_id.as_str())
            {
                // Unanswered, but its adapter is gone: nobody can answer it any more.
                *outcome = Some(wire::cancelled(method));
            }
            messages.push(wire::entry(root, &entry));
        }
        if after.is_none() {
            messages.push(wire::notification(
                "session/update",
                json!({
                    "sessionId": root,
                    "update": {"sessionUpdate": "replay_complete"},
                    "_meta": {ext::META_KEY: {"seq": session.record.head_seq}},
                }),
            ));
        }
        messages.extend(
            session
                .pending
                .values()
                .filter(|pending| pending.entry.seq <= from)
                .map(|pending| wire::entry(root, &pending.entry)),
        );
        for message in messages {
            self.send(client, message);
        }
    }

    /// ACP's own way to open an existing session, so a plain ACP v2 client can use the daemon.
    /// As ACP asks: any replay first, then the current state, then the response.
    fn resume_for_client(&mut self, client: u64, id: &Value, message: &Value) {
        let params = &message["params"];
        let session_id = text(params, "/sessionId").unwrap_or_default().to_owned();
        let Some(root) = self.open(client, id, &session_id, message) else {
            return;
        };
        let entries = self.store.after(&root, 0).unwrap_or_default();
        let mut result = json!({});
        let mut state = None;
        for entry in entries.iter().filter(|entry| entry.session_id == root) {
            let Some(update) = entry.update() else {
                continue;
            };
            match text(update, "/sessionUpdate") {
                Some("config_option_update") => {
                    result["configOptions"] = update["configOptions"].clone()
                }
                Some("available_commands_update") => {
                    result["availableCommands"] = update["availableCommands"].clone();
                }
                Some("state_update") => state = Some(entry),
                _ => {}
            }
        }
        if params.get("replayFrom").is_some() {
            self.catch_up(client, &root, None);
        } else {
            if let Some(entry) = state {
                self.send(client, wire::entry(&root, entry));
            }
            let head = self
                .sessions
                .get(&root)
                .map(|session| session.record.head_seq);
            self.catch_up(client, &root, head);
        }
        if let Some(session) = self.sessions.get_mut(&root) {
            session.subscribers.insert(client);
        }
        self.reply(client, id, result);
    }

    fn new_session(&mut self, client: u64, id: Value, message: &Value) {
        let agent = text(message, "/params/_meta/bridge/agentId")
            .map(str::to_owned)
            .or_else(|| self.agents.keys().next().cloned())
            .filter(|agent| self.agents.contains_key(agent));
        let Some(agent) = agent else {
            return self.refuse(client, &id, wire::INVALID_PARAMS, "no such agent");
        };
        match self.ensure_ready(&agent, client, message) {
            Ready::Yes => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let cwd = text(&params, "/cwd").unwrap_or_default().to_owned();
                // Kept to resume the session later with the same servers and directories.
                let mut setup = json!({});
                for field in ["mcpServers", "additionalDirectories"] {
                    if let Some(value) = params.get(field) {
                        setup[field] = value.clone();
                    }
                }
                let call = Call::New {
                    client,
                    id,
                    agent: agent.clone(),
                    cwd,
                    setup,
                };
                self.call(&agent, call, "session/new", params);
            }
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, &id, wire::INTERNAL_ERROR, &why),
        }
    }

    /// Passes an app's request about a session to the adapter that holds it.
    fn forward(&mut self, client: u64, id: Value, session_id: &str, message: Value) {
        // This comes before anything is loaded: a refused request starts nothing.
        if self.refused_as_held(client, &id, session_id) {
            return;
        }
        let Some(root) = self.open(client, &id, session_id, &message) else {
            return;
        };
        let Some(session) = self.sessions.get(&root) else {
            return;
        };
        let agent = session.record.agent_id.clone();
        if session.live {
            let method = text(&message, "/method").unwrap_or_default().to_owned();
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            return self.call(&agent, Call::Forward { client, id }, &method, params);
        }
        // Not live: start the adapter if needed, resume the session there, then try again.
        match self.ensure_ready(&agent, client, &message) {
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, &id, wire::INTERNAL_ERROR, &why),
            Ready::Yes => {
                let Some(session) = self.sessions.get_mut(&root) else {
                    return;
                };
                session.queued.push((client, message));
                if !session.resuming {
                    session.resuming = true;
                    let params = resume_params(&session.record, false);
                    self.call(&agent, Call::Resume { root }, "session/resume", params);
                }
            }
        }
    }

    /// The root session and the agent of a session, whether it is in the log or only in a
    /// harness's list.
    fn whose(&mut self, session_id: &str) -> Option<(String, String)> {
        match self.load(session_id) {
            Some(root) => self
                .sessions
                .get(&root)
                .map(|session| (root.clone(), session.record.agent_id.clone())),
            None => self
                .harness
                .get(session_id)
                .map(|known| (session_id.to_owned(), known.agent.clone())),
        }
    }

    /// Refuses a request about a session that another process of the harness has open, and
    /// says whether it did. Two processes on one session do not see each other's turns, and
    /// the harness keeps only one of the two branches for later readers. So the daemon does
    /// not become the second one.
    fn refused_as_held(&mut self, client: u64, id: &Value, session_id: &str) -> bool {
        let Some(held) = self
            .whose(session_id)
            .and_then(|(root, agent)| self.held_elsewhere(&agent, &root))
        else {
            return false;
        };
        let message = format!(
            "this session is open in another process of the agent (pid {}{}); fork it to continue here",
            held.pid,
            held.status
                .as_deref()
                .map(|status| format!(", {status}"))
                .unwrap_or_default()
        );
        self.send(
            client,
            wire::error_with(id, ext::ERROR_OPEN_ELSEWHERE, &message, json!(held)),
        );
        true
    }

    /// Deletes a session at its harness. Once the harness agrees, the daemon drops its own
    /// copy too. The session need not be loaded or live for this.
    fn delete(&mut self, client: u64, id: Value, message: &Value) {
        let session = text(message, "/params/sessionId")
            .unwrap_or_default()
            .to_owned();
        if self.refused_as_held(client, &id, &session) {
            return;
        }
        let Some((_, agent)) = self.whose(&session) else {
            return self.refuse(
                client,
                &id,
                wire::INVALID_PARAMS,
                "unknown session; list the sessions first",
            );
        };
        match self.ensure_ready(&agent, client, message) {
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, &id, wire::INTERNAL_ERROR, &why),
            Ready::Yes => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let call = Call::Delete {
                    client,
                    id,
                    session,
                };
                self.call(&agent, call, "session/delete", params);
            }
        }
    }

    /// A session is gone at its harness: drop it here and tell every app.
    fn removed(&mut self, session_id: &str) {
        self.harness.remove(session_id);
        // Only a whole session has a log of its own; a subagent's is part of its root's.
        if self
            .roots
            .get(session_id)
            .is_some_and(|root| root != session_id)
        {
            return;
        }
        self.forget(session_id);
        let message = wire::notification(
            ext::NOTIFICATION_SESSIONS_REMOVED,
            json!(ext::SessionsRemovedNotification {
                session_id: session_id.to_owned(),
            }),
        );
        let clients: Vec<u64> = self.clients.keys().copied().collect();
        for client in clients {
            self.send(client, message.clone());
        }
    }

    /// Another process of the harness that has the session open, if the agent can tell.
    fn elsewhere(&self, agent: &str, session_id: &str) -> Option<bridge_agents::Elsewhere> {
        self.agents.get(agent)?.spec.launcher.elsewhere(session_id)
    }

    /// Who else has the session open, when that stops the daemon from acting on it.
    fn held_elsewhere(&self, agent: &str, root: &str) -> Option<ext::OpenElsewhere> {
        if self.share {
            return None;
        }
        let other = self.elsewhere(agent, root)?;
        Some(ext::OpenElsewhere {
            session_id: root.to_owned(),
            pid: other.pid,
            status: other.status,
            client: other.client,
        })
    }

    /// Makes a copy of a session to continue with: a new session with the same history, which
    /// only this daemon has open. The original is left as it is, for whoever else has it.
    fn fork(&mut self, client: u64, id: Value, message: &Value) {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let source = text(&params, "/sessionId").unwrap_or_default();
        // The session need not be loaded here: the harness copies it from its own transcript.
        let known = match self.load(source) {
            Some(root) => self.sessions.get(&root).map(|session| {
                (
                    session.record.agent_id.clone(),
                    session.record.cwd.clone(),
                    session.record.setup.clone(),
                )
            }),
            None => self
                .harness
                .get(source)
                .map(|known| (known.agent.clone(), known.cwd.clone(), json!({}))),
        };
        let Some((agent, cwd, setup)) = known else {
            return self.refuse(
                client,
                &id,
                wire::INVALID_PARAMS,
                "unknown session; list the sessions first",
            );
        };
        match self.ensure_ready(&agent, client, message) {
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, &id, wire::INTERNAL_ERROR, &why),
            Ready::Yes => {
                let mut params = params;
                if params.get("cwd").is_none() {
                    params["cwd"] = cwd.as_str().into();
                }
                if params.get("mcpServers").is_none() {
                    params["mcpServers"] = setup
                        .get("mcpServers")
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                }
                let cwd = text(&params, "/cwd").unwrap_or(&cwd).to_owned();
                let call = Call::Fork {
                    client,
                    id,
                    agent: agent.clone(),
                    cwd,
                    setup,
                };
                self.call(&agent, call, "session/fork", params);
            }
        }
    }

    fn forward_to_default_agent(&mut self, client: u64, id: Value, message: &Value) {
        let Some(agent) = self.agents.keys().next().cloned() else {
            return self.refuse(
                client,
                &id,
                wire::METHOD_NOT_FOUND,
                "no agent is configured",
            );
        };
        match self.ensure_ready(&agent, client, message) {
            Ready::Yes => {
                let method = text(message, "/method").unwrap_or_default().to_owned();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                self.call(&agent, Call::Forward { client, id }, &method, params);
            }
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, &id, wire::INTERNAL_ERROR, &why),
        }
    }

    /// A notification from an app. One for a live session, such as `session/cancel`, is
    /// passed to its adapter. One under `_x/` is for the addons and goes no further.
    fn client_notification(&mut self, client: u64, message: &Value) {
        let method = text(message, "/method").unwrap_or_default().to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let agent_id = if method.starts_with("_x/") {
            self.agent_for(&params)
        } else {
            let live = text(&params, "/sessionId")
                .and_then(|id| self.roots.get(id))
                .and_then(|root| self.sessions.get(root))
                .filter(|session| session.live)
                .map(|session| session.record.agent_id.clone());
            match live {
                Some(agent) => agent,
                None => return,
            }
        };
        self.ids += 1;
        let notification = OneWay {
            id: self.ids,
            source: self.app(client),
            what: What::Notification(method),
            body: params,
        };
        self.pass(&agent_id, 0, notification);
    }

    fn to_adapter(&self, agent_id: &str, message: Value) {
        if let Some(adapter) = self
            .agents
            .get(agent_id)
            .and_then(|agent| agent.running.as_ref())
        {
            let _ = adapter.tx.try_send(message);
        }
    }

    /// The agent an addon talks to while it looks at a message that belongs to no adapter:
    /// the one of the session the params name, or the first.
    fn agent_for(&self, params: &Value) -> String {
        text(params, "/sessionId")
            .and_then(|id| self.roots.get(id))
            .and_then(|root| self.sessions.get(root))
            .map(|session| session.record.agent_id.clone())
            .or_else(|| self.agents.keys().next().cloned())
            .unwrap_or_default()
    }

    fn app(&self, client: u64) -> Source {
        if client >= ADDON_CLIENTS {
            return Source::Addon((client - ADDON_CLIENTS) as usize);
        }
        Source::App(
            self.clients
                .get(&client)
                .and_then(|entry| entry.name.clone()),
        )
    }

    /// An app calls a method under `_x/`. An addon may serve it; otherwise it is an
    /// extension's.
    fn own_method(&mut self, client: u64, method: &str, id: Value, params: Value) {
        let agent_id = self.agent_for(&params);
        self.ids += 1;
        let call = OwnCall {
            client,
            id,
            method: method.to_owned(),
            params,
            request: self.ids,
        };
        self.serve(&agent_id, 0, call);
    }

    /// Offers an app's `_x/` call to the addons from layer `from` on, then to an extension.
    fn serve(&mut self, agent_id: &str, from: usize, call: OwnCall) {
        let source = self.app(call.client);
        for index in from..self.addons.len() {
            let origin = self.origin(&source);
            let verdict = self.hook(index, agent_id, false, |addon, cx, _| {
                let shown = addon::MethodCall {
                    id: call.request,
                    from: origin,
                    method: &call.method,
                    params: &call.params,
                };
                addon.method(cx, &shown)
            });
            match verdict {
                None | Some(addon::Verdict::Pass) => {}
                Some(addon::Verdict::Hold) => {
                    return self.hold(index, agent_id, call.request, HeldWhat::Method(call.into()));
                }
                Some(addon::Verdict::Answer(result)) => {
                    return self.reply(call.client, &call.id, result);
                }
                Some(addon::Verdict::Refuse { code, message }) => {
                    let data = json!({"addon": self.addon_id(index)});
                    let refusal = wire::error_with(&call.id, code, &message, data);
                    return self.send(call.client, refusal);
                }
            }
        }
        self.call_extension(call.client, &call.method, call.id, call.params);
    }

    /// An extension says who it is.
    fn register_extension(&mut self, client: u64, id: &Value, params: &Value) {
        if !self.allowed(client, id, Scope::Extend) {
            return;
        }
        let namespace = text(params, "/namespace").unwrap_or_default();
        let well_formed = !namespace.is_empty()
            && namespace
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !well_formed {
            return self.refuse(
                client,
                id,
                wire::INVALID_PARAMS,
                "a namespace is lower-case letters, digits and `-`",
            );
        }
        if self
            .extensions
            .get(namespace)
            .is_some_and(|serving| *serving != client)
        {
            return self.refuse(
                client,
                id,
                wire::INVALID_PARAMS,
                "another connection serves this namespace",
            );
        }
        self.extensions.insert(namespace.to_owned(), client);
        self.reply(client, id, json!({}));
    }

    /// An extension adds an update of its own kind to a session.
    fn append(&mut self, client: u64, id: &Value, message: &Value) {
        if !self.allowed(client, id, Scope::Extend) {
            return;
        }
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Ok(request) = serde_json::from_value::<ext::AppendRequest>(params) else {
            return self.refuse(
                client,
                id,
                wire::INVALID_PARAMS,
                "expected a sessionId and an update",
            );
        };
        let kind = text(&request.update, "/sessionUpdate").unwrap_or_default();
        let own = self.extensions.iter().any(|(namespace, serving)| {
            *serving == client
                && kind
                    .strip_prefix("x.")
                    .and_then(|rest| rest.strip_prefix(namespace.as_str()))
                    .is_some_and(|rest| rest.starts_with('.'))
        });
        if !own {
            return self.refuse(
                client,
                id,
                wire::INVALID_PARAMS,
                "the update's kind must start with `x.<your namespace>.`",
            );
        }
        let Some(root) = self.open(client, id, &request.session_id, message) else {
            return;
        };
        let mut update = request.update;
        let mut mark = json!({"origin": "extension"});
        if let Some(state) = request.state {
            mark["state"] = json!(state);
        }
        if !update["_meta"].is_object() {
            update["_meta"] = json!({});
        }
        update["_meta"][ext::META_KEY] = mark;
        match self.log(&root, &request.session_id, Record::Update { update }) {
            Some(entry) => self.reply(client, id, json!(ext::AppendResponse { seq: entry.seq })),
            None => self.refuse(
                client,
                id,
                wire::INTERNAL_ERROR,
                "the update could not be stored",
            ),
        }
    }

    /// An app calls a method of an extension. The daemon only carries it there and back.
    fn call_extension(&mut self, client: u64, method: &str, id: Value, params: Value) {
        let namespace = method.split('/').nth(1).unwrap_or_default();
        let Some(&extension) = self.extensions.get(namespace) else {
            return self.refuse(
                client,
                &id,
                wire::METHOD_NOT_FOUND,
                "no extension serves this method",
            );
        };
        self.next_extension_call += 1;
        let call = format!("ext-{}", self.next_extension_call);
        self.extension_calls
            .insert(call.clone(), (extension, client, id));
        self.send(extension, wire::request(call.as_str(), method, params));
    }

    /// An app answers an approval or a question. Only the first answer counts.
    fn client_answer(&mut self, client: u64, id: &Value, message: &Value) {
        let Some(request_id) = id.as_str() else {
            return;
        };
        // An extension answering a call that was passed to it.
        if self
            .extension_calls
            .get(request_id)
            .is_some_and(|(extension, _, _)| *extension == client)
        {
            if let Some((_, asker, their_id)) = self.extension_calls.remove(request_id) {
                self.send(asker, wire::relay(&their_id, message));
            }
            return;
        }
        let Some(root) = self.requests.get(request_id).cloned() else {
            return; // Unknown, or somebody else answered first.
        };
        let Some(session) = self.sessions.get_mut(&root) else {
            return;
        };
        if !session.subscribers.contains(&client) {
            return;
        }
        let Some(pending) = session.pending.shift_remove(request_id) else {
            return;
        };
        let agent_id = session.record.agent_id.clone();
        self.requests.remove(request_id);
        let by = self
            .clients
            .get(&client)
            .and_then(|entry| entry.name.clone());
        let answered = Answered {
            ask: pending.ask,
            outcome: addon::Outcome::of(message),
            source: Source::App(by.clone()),
            by,
            entry: Some(pending.entry),
        };
        self.answered(&agent_id, 0, answered);
    }

    fn resolve(&mut self, root: &str, request: &LogEntry, outcome: Value, by: Option<String>) {
        if let Record::Request {
            request_id, method, ..
        } = &request.record
        {
            let record = Record::Resolved {
                request_id: request_id.clone(),
                method: method.clone(),
                outcome,
                resolved_by: by,
            };
            self.log(root, &request.session_id, record);
        }
    }

    // ---- sessions and the log --------------------------------------------------------------

    /// Finds the root session of `session_id`, reading it from the store if it is not in memory.
    fn load(&mut self, session_id: &str) -> Option<String> {
        if let Some(root) = self.roots.get(session_id) {
            return Some(root.clone());
        }
        let record = self.store.session(session_id).ok()??;
        let entries = self.store.after(session_id, 0).unwrap_or_default();
        self.roots
            .insert(session_id.to_owned(), session_id.to_owned());
        let mut unanswered: IndexMap<String, LogEntry> = IndexMap::new();
        for entry in entries {
            match &entry.record {
                Record::Update { update } => {
                    if let Some(child) = subagent_session(update) {
                        self.roots.insert(child.to_owned(), session_id.to_owned());
                    }
                }
                Record::Request { request_id, .. } => {
                    unanswered.insert(request_id.clone(), entry);
                }
                Record::Resolved { request_id, .. } => {
                    unanswered.shift_remove(request_id);
                }
            }
        }
        self.sessions.insert(
            session_id.to_owned(),
            Session {
                record,
                live: false,
                resuming: false,
                adopting: false,
                queued: Vec::new(),
                subscribers: BTreeSet::new(),
                pending: IndexMap::new(),
            },
        );
        // Whatever was in flight when the previous daemon stopped is over.
        for request in unanswered.values() {
            if let Record::Request { method, .. } = &request.record {
                let outcome = wire::cancelled(method);
                self.resolve(session_id, request, outcome, Some("daemon".to_owned()));
            }
        }
        self.end_turn(session_id, "the daemon was restarted");
        Some(session_id.to_owned())
    }

    /// Numbers a record, stores it, and sends it to every attached app.
    fn log(&mut self, root: &str, session_id: &str, record: Record) -> Option<LogEntry> {
        let at = now();
        let seq = match self.store.append(root, session_id, &record, at) {
            Ok(seq) => seq,
            Err(error) => {
                eprintln!("bridge: dropped an event of session {root}: {error}");
                return None;
            }
        };
        let entry = LogEntry {
            seq,
            session_id: session_id.to_owned(),
            record,
        };
        let session = self.sessions.get_mut(root)?;
        session.record.head_seq = seq;
        session.record.updated_at = at;
        let mut summary_changed = false;
        if session_id == root
            && let Record::Update { update } = &entry.record
        {
            // Keep the title and turn state for the session list. The rules say where they are.
            let mut scratch = SessionState::default();
            if scratch.apply_update(&self.rules, update).is_ok() {
                let title = scratch
                    .values
                    .get("sessionInfo")
                    .and_then(|info| text(info, "/title"));
                let state = scratch
                    .values
                    .get("state")
                    .and_then(|state| text(state, "/state"));
                let mut changed = false;
                for (new, current) in [
                    (title, &mut session.record.title),
                    (state, &mut session.record.state),
                ] {
                    if new.is_some() && new != current.as_deref() {
                        *current = new.map(str::to_owned);
                        changed = true;
                    }
                }
                if changed {
                    if let Err(error) = self.store.put_session(&session.record) {
                        eprintln!("bridge: {error}");
                    }
                    summary_changed = true;
                }
            }
        }
        let message = wire::entry(root, &entry);
        let subscribers: Vec<u64> = session.subscribers.iter().copied().collect();
        for client in subscribers {
            self.send(client, message.clone());
        }
        if summary_changed {
            self.announce(root);
        }
        Some(entry)
    }

    /// Writes a session's setup values into its log, so the log alone can rebuild them.
    /// They arrive in the response to `session/new` or `session/resume`, not as updates.
    fn log_setup(&mut self, root: &str, result: &Value) {
        for (field, kind) in [
            ("configOptions", "config_option_update"),
            ("availableCommands", "available_commands_update"),
        ] {
            if let Some(value) = result.get(field) {
                let update = wire::own_update(json!({"sessionUpdate": kind, field: value}));
                self.log(root, root, Record::Update { update });
            }
        }
    }

    /// Closes what was in flight when a session lost its adapter: pending requests are
    /// cancelled, and a turn that was running is reported as stopped.
    fn end_turn(&mut self, root: &str, why: &str) {
        let Some(session) = self.sessions.get_mut(root) else {
            return;
        };
        let pending: Vec<Pending> = session
            .pending
            .drain(..)
            .map(|(_, pending)| pending)
            .collect();
        let was_busy = session
            .record
            .state
            .as_deref()
            .is_some_and(|state| state != "idle");
        let agent_id = session.record.agent_id.clone();
        for pending in pending {
            if let Record::Request {
                request_id, method, ..
            } = &pending.entry.record
            {
                self.requests.remove(request_id);
                let outcome = wire::cancelled(method);
                self.resolve(root, &pending.entry, outcome, Some("daemon".to_owned()));
            }
            // An addon that asked is told; an adapter that asked is gone.
            let outcome = cancelled(&pending.ask.method);
            self.deliver(&agent_id, &pending.ask.asker, &outcome);
        }
        if was_busy {
            self.cancel_open_tool_calls(root);
            for update in [
                json!({"sessionUpdate": "notice", "severity": "error", "title": "The turn was interrupted", "description": why}),
                json!({"sessionUpdate": "state_update", "state": "idle", "stopReason": "cancelled", "background": []}),
            ] {
                let update = wire::own_update(update);
                self.log(root, root, Record::Update { update });
            }
        }
    }

    /// Marks tool calls that were still waiting or running as cancelled. Their turn is over,
    /// and the agent that would have finished them is gone.
    fn cancel_open_tool_calls(&mut self, root: &str) {
        let entries = self.store.after(root, 0).unwrap_or_default();
        let mut sessions: IndexMap<String, SessionState> = IndexMap::new();
        for entry in &entries {
            let state = sessions.entry(entry.session_id.clone()).or_default();
            let _ = match &entry.record {
                Record::Update { update } => state.apply_update(&self.rules, update),
                Record::Request {
                    request_id,
                    method,
                    params,
                    ..
                } => state.apply_request(&self.rules, request_id, method, params),
                Record::Resolved { .. } => continue,
            };
        }
        let mut open = Vec::new();
        for (session_id, state) in &sessions {
            for (id, call) in state.collections.get("toolCall").into_iter().flatten() {
                if matches!(
                    text(call, "/status"),
                    Some("pending" | "in_progress" | "paused")
                ) {
                    open.push((session_id.clone(), id.clone()));
                }
            }
        }
        for (session_id, id) in open {
            let update = wire::own_update(
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": "cancelled"}),
            );
            self.log(root, &session_id, Record::Update { update });
        }
    }

    // ---- adapters --------------------------------------------------------------------------

    /// Makes sure an agent's adapter is started and has answered `initialize`. If it has not
    /// yet, the app's request is put aside and handled again once it has.
    fn ensure_ready(&mut self, agent_id: &str, client: u64, message: &Value) -> Ready {
        if !self.agents.contains_key(agent_id) {
            return Ready::No(format!("agent `{agent_id}` is not configured"));
        }
        if self.agents[agent_id].running.is_none()
            && let Err(why) = self.launch(agent_id)
        {
            return Ready::No(why);
        }
        let Some(adapter) = self.agents[agent_id].running.as_mut() else {
            return Ready::No("the agent did not start".to_owned());
        };
        if adapter.initialized.is_some() {
            Ready::Yes
        } else {
            adapter.waiting.push((client, message.clone()));
            Ready::Later
        }
    }

    fn launch(&mut self, agent_id: &str) -> Result<(), String> {
        let agent = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| "unknown agent".to_owned())?;
        let running = agent.spec.launcher.launch().map_err(|error| {
            let why = format!("could not start the agent: {error}");
            agent.last_error = Some(why.clone());
            why
        })?;
        self.generations += 1;
        let generation = self.generations;
        let Link { tx, mut rx } = running.link;
        let (events, name) = (self.events.clone(), agent_id.to_owned());
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let event = Event::FromAgent {
                    agent: name.clone(),
                    generation,
                    message,
                };
                if events.send(event).await.is_err() {
                    return;
                }
            }
            let _ = events
                .send(Event::AgentClosed {
                    agent: name,
                    generation,
                })
                .await;
        });
        agent.last_error = None;
        agent.running = Some(Adapter {
            tx,
            _guard: running.guard,
            generation,
            initialized: None,
            next_id: 0,
            calls: HashMap::new(),
            waiting: Vec::new(),
            early: Vec::new(),
            auth: None,
        });
        // Like every request, `initialize` passes the addons.
        self.call(
            agent_id,
            Call::Initialize,
            "initialize",
            initialize_params(),
        );
        Ok(())
    }

    /// The agent as an addon sees it.
    fn agent_view<'a>(agents: &'a IndexMap<String, Agent>, agent_id: &'a str) -> addon::Agent<'a> {
        let agent = agents.get(agent_id);
        let info = agent
            .and_then(|agent| agent.running.as_ref())
            .and_then(|adapter| adapter.initialized.as_ref());
        addon::Agent {
            id: agent_id,
            harness: agent.and_then(|agent| agent.spec.harness.as_deref()),
            adapter: info.and_then(|result| text(result, "/info/name")),
            version: info.and_then(|result| text(result, "/info/version")),
            capabilities: info.and_then(|result| result.get("capabilities")),
            os: std::env::consts::OS,
        }
    }

    fn addon_id(&self, index: usize) -> &'static str {
        self.addons
            .get(index)
            .and_then(Option::as_ref)
            .map_or("", |addon| addon.info().id)
    }

    /// Runs one hook of one addon, then carries out what the addon asked for. `None` when
    /// the addon was not asked: it does not apply to the agent (if `check`), or it is the
    /// one whose hook is running.
    fn hook<T>(
        &mut self,
        index: usize,
        agent_id: &str,
        check: bool,
        run: impl FnOnce(&mut dyn addon::Addon, &mut addon::Cx<'_>, addon::Agent<'_>) -> T,
    ) -> Option<T> {
        let mut addon = self.addons.get_mut(index)?.take()?;
        let agent = Self::agent_view(&self.agents, agent_id);
        let ran = (!check || addon.applies(&agent)).then(|| {
            let logs = Logs {
                store: self.store.as_ref(),
                roots: &self.roots,
            };
            let mut cx = addon::Cx::new(&logs, self.ids + 1);
            let out = run(addon.as_mut(), &mut cx, agent);
            let (effects, next) = cx.finish();
            (out, effects, next - 1)
        });
        self.addons[index] = Some(addon);
        let (out, effects, last) = ran?;
        self.ids = last;
        for effect in effects {
            self.effect(index, agent_id, effect);
        }
        Some(out)
    }

    /// Who a kept message is from, as an addon is told.
    fn origin<'a>(&self, source: &'a Source) -> addon::Origin<'a> {
        match source {
            Source::App(name) => addon::Origin::App {
                name: name.as_deref(),
            },
            Source::Daemon => addon::Origin::Daemon,
            Source::Adapter => addon::Origin::Adapter,
            Source::Addon(index) => addon::Origin::Addon(self.addon_id(*index)),
        }
    }

    /// Who made a request to an adapter.
    fn source(&self, call: &Call) -> Source {
        match call {
            Call::Initialize | Call::List { .. } | Call::Resume { .. } => Source::Daemon,
            Call::New { client, .. }
            | Call::Fork { client, .. }
            | Call::Delete { client, .. }
            | Call::Forward { client, .. } => self.app(*client),
            Call::Addon { addon, .. } => Source::Addon(*addon),
        }
    }

    /// Marks something an addon sends towards the apps as that addon's.
    fn mark(&self, index: usize, value: &mut Value) {
        if !value.is_object() {
            return;
        }
        if !value["_meta"].is_object() {
            value["_meta"] = json!({});
        }
        value["_meta"][ext::META_KEY] = json!({"origin": "addon", "addon": self.addon_id(index)});
    }

    /// Carries out one thing the addon `index` asked for while looking at a message of
    /// `agent_id`. What it sends starts beyond its own layer, in the direction it travels.
    fn effect(&mut self, index: usize, agent_id: &str, effect: addon::Effect) {
        self.ids += 1;
        let number = self.ids;
        match effect {
            addon::Effect::Call { id, method, params } => {
                let sent = Sent {
                    call: Call::Addon { addon: index, id },
                    method,
                    params,
                    request: number,
                };
                self.dispatch(agent_id, index + 1, sent);
            }
            addon::Effect::Notify { method, params } => {
                let notification = OneWay {
                    id: number,
                    source: Source::Addon(index),
                    what: What::Notification(method),
                    body: params,
                };
                self.pass(agent_id, index + 1, notification);
            }
            addon::Effect::Append {
                session_id,
                mut update,
            } => {
                if !self.roots.contains_key(&session_id) {
                    return;
                }
                self.mark(index, &mut update);
                let update = OneWay {
                    id: number,
                    source: Source::Addon(index),
                    what: What::Update(session_id),
                    body: update,
                };
                self.pass(agent_id, index, update);
            }
            addon::Effect::Announce { method, params } => {
                let notification = OneWay {
                    id: number,
                    source: Source::Addon(index),
                    what: What::AgentNotification(method),
                    body: params,
                };
                self.pass(agent_id, index, notification);
            }
            addon::Effect::Ask {
                id,
                method,
                mut params,
            } => {
                self.mark(index, &mut params);
                let ask = Ask {
                    asker: Asker::Addon { addon: index, id },
                    method,
                    params,
                    request: number,
                };
                self.ask(agent_id, index, ask);
            }
            addon::Effect::AppCall { id, method, params } => {
                let client = ADDON_CLIENTS + index as u64;
                self.app_calls.insert((client, id), agent_id.to_owned());
                self.handle(client, wire::request(id, &method, params));
            }
            addon::Effect::Tell {
                app,
                method,
                params,
            } => self.send(app, wire::notification(&method, params)),
            addon::Effect::Wake { id, after } => {
                let (events, agent) = (self.events.clone(), agent_id.to_owned());
                tokio::spawn(async move {
                    tokio::time::sleep(after).await;
                    let woken = Event::Timer {
                        addon: index,
                        agent,
                        id,
                    };
                    let _ = events.send(woken).await;
                });
            }
            addon::Effect::Release { id, verdict } => {
                let how = match verdict {
                    addon::Verdict::Hold => return,
                    addon::Verdict::Pass => Release::Pass(None),
                    addon::Verdict::Answer(result) => {
                        Release::Outcome(addon::Outcome::Result(result))
                    }
                    addon::Verdict::Refuse { code, message } => {
                        Release::Outcome(addon::Outcome::Error {
                            code,
                            message,
                            data: Some(json!({"addon": self.addon_id(index)})),
                        })
                    }
                };
                self.release(index, id, how);
            }
            addon::Effect::ReleaseWith { id, content } => {
                self.release(index, id, Release::Pass(Some(content)));
            }
            addon::Effect::Discard { id } => self.release(index, id, Release::Discard),
        }
    }

    /// Keeps a message for the addon at layer `index` until it releases it.
    fn hold(&mut self, index: usize, agent_id: &str, id: u64, what: HeldWhat) {
        let held = Held {
            addon: index,
            agent: agent_id.to_owned(),
            what,
        };
        self.held.insert(id, held);
    }

    /// Ends a hold: the message goes on from the layer that held it. Only the addon that
    /// holds a message can release it.
    fn release(&mut self, index: usize, id: u64, how: Release) {
        if self.held.get(&id).is_none_or(|held| held.addon != index) {
            return;
        }
        let Some(Held { addon, agent, what }) = self.held.remove(&id) else {
            return;
        };
        // Somebody waits for a request, a response or an answer, so it cannot just vanish.
        let dropped = addon::Outcome::Error {
            code: wire::INTERNAL_ERROR,
            message: "an addon dropped the message".to_owned(),
            data: Some(json!({"addon": self.addon_id(index)})),
        };
        match what {
            HeldWhat::Request(mut sent) => match how {
                Release::Pass(params) => {
                    if let Some(params) = params {
                        sent.params = params;
                    }
                    self.dispatch(&agent, addon + 1, *sent);
                }
                Release::Outcome(outcome) => self.settle(&agent, addon, *sent, outcome),
                Release::Discard => self.settle(&agent, addon, *sent, dropped),
            },
            HeldWhat::Response(sent, outcome) => {
                let outcome = how.over(outcome, dropped);
                self.settle(&agent, addon, *sent, outcome);
            }
            HeldWhat::Method(mut call) => match how {
                Release::Pass(params) => {
                    if let Some(params) = params {
                        call.params = params;
                    }
                    self.serve(&agent, addon + 1, *call);
                }
                Release::Outcome(outcome) => {
                    self.send(call.client, wire::relay(&call.id, &outcome.to_json()));
                }
                Release::Discard => {
                    self.send(call.client, wire::relay(&call.id, &dropped.to_json()));
                }
            },
            HeldWhat::OneWay(mut message) => {
                let Release::Pass(body) = how else {
                    return;
                };
                if let Some(body) = body {
                    message.body = body;
                }
                let next = match message.what {
                    What::Notification(_) => addon + 1,
                    What::Update(_) | What::AgentNotification(_) => addon,
                };
                self.pass(&agent, next, *message);
            }
            HeldWhat::Edge(mut edge) => {
                let next = if edge.kind.inward() { addon + 1 } else { addon };
                let waited_for = matches!(edge.kind, EdgeKind::Request | EdgeKind::Response(..));
                let outcome = match how {
                    Release::Pass(None) => return self.edge(next, *edge),
                    Release::Pass(Some(content)) if edge.message.get("method").is_some() => {
                        edge.message["params"] = content;
                        return self.edge(next, *edge);
                    }
                    Release::Pass(Some(result)) => addon::Outcome::Result(result),
                    Release::Outcome(outcome) => outcome,
                    Release::Discard if waited_for => dropped,
                    Release::Discard => return,
                };
                match edge.kind {
                    EdgeKind::Request => self.edge_answer(addon, *edge, &outcome),
                    EdgeKind::Response(..) | EdgeKind::Answer => {
                        let id = edge.message["id"].clone();
                        edge.message = wire::relay(&id, &outcome.to_json());
                        self.edge(next, *edge);
                    }
                    // An outcome for a message nobody answers: it is dropped.
                    EdgeKind::Notification | EdgeKind::Push => {}
                }
            }
            HeldWhat::Ask(mut ask) => match how {
                Release::Pass(params) => {
                    if let Some(params) = params {
                        ask.params = params;
                    }
                    self.ask(&agent, addon, *ask);
                }
                Release::Outcome(outcome) => self.answered_by(&agent, addon, *ask, outcome),
                Release::Discard => self.answered_by(&agent, addon, *ask, dropped),
            },
            HeldWhat::Answer(answered) => {
                let mut answered = *answered;
                answered.outcome = how.over(answered.outcome, dropped);
                self.answered(&agent, addon + 1, answered);
            }
        }
    }

    /// Sends a request to an adapter and remembers what to do with the answer.
    fn call(&mut self, agent_id: &str, call: Call, method: &str, params: Value) {
        self.ids += 1;
        let sent = Sent {
            call,
            method: method.to_owned(),
            params,
            request: self.ids,
        };
        self.dispatch(agent_id, 0, sent);
    }

    /// Passes a request through the addons from layer `from` on, then to the adapter.
    fn dispatch(&mut self, agent_id: &str, from: usize, mut sent: Sent) {
        let source = self.source(&sent.call);
        for index in from..self.addons.len() {
            let origin = self.origin(&source);
            let verdict = self.hook(index, agent_id, true, |addon, cx, agent| {
                addon.request(
                    cx,
                    &mut addon::Request {
                        id: sent.request,
                        agent,
                        from: origin,
                        method: &sent.method,
                        params: &mut sent.params,
                    },
                )
            });
            match verdict {
                None | Some(addon::Verdict::Pass) => {}
                Some(addon::Verdict::Hold) => {
                    let what = HeldWhat::Request(sent.into());
                    return self.hold(index, agent_id, what_id(&what), what);
                }
                Some(addon::Verdict::Answer(result)) => {
                    return self.settle(agent_id, index, sent, addon::Outcome::Result(result));
                }
                Some(addon::Verdict::Refuse { code, message }) => {
                    let outcome = addon::Outcome::Error {
                        code,
                        message,
                        data: Some(json!({"addon": self.addon_id(index)})),
                    };
                    return self.settle(agent_id, index, sent, outcome);
                }
            }
        }
        let Some(adapter) = self
            .agents
            .get_mut(agent_id)
            .and_then(|agent| agent.running.as_mut())
        else {
            return self.fail(agent_id, sent.call, "the agent is not running");
        };
        let id = adapter.next_id;
        adapter.next_id += 1;
        let message = wire::request(id, &sent.method, sent.params.clone());
        adapter.calls.insert(id, sent);
        if adapter.tx.try_send(message).is_err() {
            let generation = adapter.generation;
            self.agent_stopped(agent_id, generation, "the agent stopped responding");
        }
    }

    /// A request has its outcome, from the adapter or from the addon at layer `upto`. The
    /// layers nearer the app see it, then the daemon acts on it.
    fn settle(&mut self, agent_id: &str, upto: usize, sent: Sent, mut outcome: addon::Outcome) {
        // An addon's own request started at its layer, so its answer ends there.
        let lowest = match sent.call {
            Call::Addon { addon, .. } => addon + 1,
            _ => 0,
        };
        let source = self.source(&sent.call);
        for index in (lowest..upto.min(self.addons.len())).rev() {
            let origin = self.origin(&source);
            let flow = self.hook(index, agent_id, true, |addon, cx, agent| {
                addon.response(
                    cx,
                    &mut addon::Response {
                        id: sent.request,
                        agent,
                        from: origin,
                        method: &sent.method,
                        params: &sent.params,
                        outcome: &mut outcome,
                    },
                )
            });
            if flow == Some(addon::Flow::Hold) {
                let what = HeldWhat::Response(sent.into(), outcome);
                return self.hold(index, agent_id, what_id(&what), what);
            }
        }
        let generation = self
            .agents
            .get(agent_id)
            .and_then(|agent| agent.running.as_ref())
            .map_or(0, |adapter| adapter.generation);
        self.agent_answered(agent_id, generation, sent.call, &outcome.to_json());
    }

    /// A request cannot be answered by its adapter: tell whoever waits.
    fn fail(&mut self, agent_id: &str, call: Call, why: &str) {
        match call {
            Call::New { client, id, .. }
            | Call::Fork { client, id, .. }
            | Call::Delete { client, id, .. }
            | Call::Forward { client, id } => {
                self.refuse(client, &id, wire::INTERNAL_ERROR, why);
            }
            Call::Resume { root } => {
                if let Some(session) = self.sessions.get_mut(&root) {
                    session.resuming = false;
                    let adopting = std::mem::take(&mut session.adopting);
                    let queued = std::mem::take(&mut session.queued);
                    if adopting {
                        self.forget(&root);
                    }
                    for (client, message) in queued {
                        if let Some(id) = message.get("id") {
                            self.refuse(client, id, wire::INTERNAL_ERROR, why);
                        }
                    }
                }
            }
            Call::List { job, agent } => self.sessions_listed(job, &agent, &Value::Null),
            Call::Initialize => {}
            Call::Addon { addon, id } => {
                let outcome = addon::Outcome::Error {
                    code: wire::INTERNAL_ERROR,
                    message: why.to_owned(),
                    data: None,
                };
                self.hook(addon, agent_id, false, |addon, cx, _| {
                    addon.reply(cx, id, &outcome);
                });
            }
        }
    }

    /// Passes a message nobody answers through the addons, then acts on it. A notification
    /// travels towards the adapter and starts at layer `next`; an update or another
    /// notification of the adapter travels towards the apps and starts below layer `next`.
    fn pass(&mut self, agent_id: &str, next: usize, mut message: OneWay) {
        let layers = self.addons.len();
        let order: Vec<usize> = match message.what {
            What::Notification(_) => (next..layers).collect(),
            What::Update(_) | What::AgentNotification(_) => (0..next.min(layers)).rev().collect(),
        };
        for index in order {
            let (id, from) = (message.id, self.origin(&message.source));
            let kept = self.hook(index, agent_id, true, |addon, cx, agent| {
                match &message.what {
                    What::Notification(method) => addon.notification(
                        cx,
                        &mut addon::Notification {
                            id,
                            agent,
                            from,
                            method,
                            params: &mut message.body,
                        },
                    ),
                    What::Update(session_id) => addon.update(
                        cx,
                        &mut addon::Update {
                            id,
                            agent,
                            from,
                            session_id,
                            update: &mut message.body,
                        },
                    ),
                    What::AgentNotification(method) => addon.agent_notification(
                        cx,
                        &mut addon::AgentNotification {
                            id,
                            agent,
                            from,
                            method,
                            params: &mut message.body,
                        },
                    ),
                }
            });
            match kept {
                None | Some(addon::Keep::Yes) => {}
                Some(addon::Keep::No) => return,
                Some(addon::Keep::Hold) => {
                    return self.hold(index, agent_id, id, HeldWhat::OneWay(message.into()));
                }
            }
        }
        match (message.what, message.source) {
            // One under `_x/` was for the addons.
            (What::Notification(method), _) => {
                if !method.starts_with("_x/") {
                    self.to_adapter(agent_id, wire::notification(&method, message.body));
                }
            }
            (What::Update(session_id), _) => {
                if let Some(root) = self.roots.get(&session_id).cloned() {
                    let update = message.body;
                    self.log(&root, &session_id, Record::Update { update });
                }
            }
            // An addon's own goes to the apps of the session it names, or to all of them.
            (What::AgentNotification(method), Source::Addon(_)) => {
                let attached = text(&message.body, "/sessionId")
                    .and_then(|id| self.roots.get(id))
                    .and_then(|root| self.sessions.get(root))
                    .map(|session| session.subscribers.iter().copied().collect());
                let clients: Vec<u64> =
                    attached.unwrap_or_else(|| self.clients.keys().copied().collect());
                let notification = wire::notification(&method, message.body);
                for client in clients {
                    self.send(client, notification.clone());
                }
            }
            // An adapter's is neither stored nor passed on. Claude's adapter sends
            // `auth/status_update` with the account's email address; only its state is kept.
            (What::AgentNotification(method), _) => {
                if method == "auth/status_update"
                    && let Some(adapter) = self
                        .agents
                        .get_mut(agent_id)
                        .and_then(|agent| agent.running.as_mut())
                {
                    adapter.auth = text(&message.body, "/state").map(str::to_owned);
                }
            }
        }
    }

    /// Somebody asks the user something: an adapter (`upto` is then the number of layers),
    /// or the addon at layer `upto`. The layers nearer the app see it first and may answer
    /// in the user's place. Otherwise it is logged and stays pending until an app answers,
    /// however long that takes and whether or not an app is attached right now.
    fn ask(&mut self, agent_id: &str, upto: usize, mut ask: Ask) {
        let source = match ask.asker {
            Asker::Adapter(_) => Source::Adapter,
            Asker::Addon { addon, .. } => Source::Addon(addon),
        };
        for index in (0..upto.min(self.addons.len())).rev() {
            let origin = self.origin(&source);
            let verdict = self.hook(index, agent_id, true, |addon, cx, agent| {
                addon.agent_request(
                    cx,
                    &mut addon::AgentRequest {
                        id: ask.request,
                        agent,
                        from: origin,
                        method: &ask.method,
                        params: &mut ask.params,
                    },
                )
            });
            let outcome = match verdict {
                None | Some(addon::Verdict::Pass) => continue,
                Some(addon::Verdict::Hold) => {
                    return self.hold(index, agent_id, ask.request, HeldWhat::Ask(ask.into()));
                }
                Some(addon::Verdict::Answer(result)) => addon::Outcome::Result(result),
                Some(addon::Verdict::Refuse { code, message }) => addon::Outcome::Error {
                    code,
                    message,
                    data: Some(json!({"addon": self.addon_id(index)})),
                },
            };
            return self.answered_by(agent_id, index, ask, outcome);
        }
        let Some(entry) = self.log_ask(&ask) else {
            let outcome = addon::Outcome::Error {
                code: wire::METHOD_NOT_FOUND,
                message: "the request names no session the daemon knows".to_owned(),
                data: None,
            };
            return self.deliver(agent_id, &ask.asker, &outcome);
        };
        let root = self.roots.get(&entry.session_id).cloned();
        if let (Record::Request { request_id, .. }, Some(root)) = (&entry.record, root)
            && let Some(session) = self.sessions.get_mut(&root)
        {
            let request_id = request_id.clone();
            self.requests.insert(request_id.clone(), root);
            session.pending.insert(request_id, Pending { ask, entry });
        }
    }

    /// Writes a request made of the user into its session's log, which shows it to the apps.
    fn log_ask(&mut self, ask: &Ask) -> Option<LogEntry> {
        let session_id = text(&ask.params, "/sessionId")?.to_owned();
        let root = self.roots.get(&session_id)?.clone();
        let head = self.sessions.get(&root)?.record.head_seq;
        let record = Record::Request {
            request_id: format!("{root}#{}", head + 1),
            method: ask.method.clone(),
            params: ask.params.clone(),
            outcome: None,
        };
        self.log(&root, &session_id, record)
    }

    /// The addon at layer `index` answers a request in the user's place. The log holds the
    /// request and who answered, and no app is asked.
    fn answered_by(&mut self, agent_id: &str, index: usize, ask: Ask, outcome: addon::Outcome) {
        let answered = Answered {
            entry: self.log_ask(&ask),
            ask,
            outcome,
            source: Source::Addon(index),
            by: Some(format!("addon {}", self.addon_id(index))),
        };
        // The layers nearer the asker see the answer as they would the user's.
        self.answered(agent_id, index + 1, answered);
    }

    /// A request made of the user has its answer. The layers from `from` on see it, up to
    /// whoever asked; then it is handed over and logged.
    fn answered(&mut self, agent_id: &str, from: usize, mut answered: Answered) {
        let end = match answered.ask.asker {
            Asker::Adapter(_) => self.addons.len(),
            Asker::Addon { addon, .. } => addon,
        };
        for index in from..end.min(self.addons.len()) {
            let origin = self.origin(&answered.source);
            let flow = self.hook(index, agent_id, true, |addon, cx, agent| {
                addon.answer(
                    cx,
                    &mut addon::Answer {
                        id: answered.ask.request,
                        agent,
                        from: origin,
                        method: &answered.ask.method,
                        params: &answered.ask.params,
                        outcome: &mut answered.outcome,
                    },
                )
            });
            if flow == Some(addon::Flow::Hold) {
                let id = answered.ask.request;
                return self.hold(index, agent_id, id, HeldWhat::Answer(answered.into()));
            }
        }
        let answer = answered.outcome.to_json();
        if let Asker::Adapter(id) = &answered.ask.asker {
            self.to_adapter(agent_id, wire::relay(id, &answer));
        }
        if let Some(entry) = &answered.entry
            && let Some(root) = self.roots.get(&entry.session_id).cloned()
        {
            let outcome = answer
                .get("result")
                .or_else(|| answer.get("error"))
                .cloned()
                .unwrap_or(Value::Null);
            self.resolve(&root, entry, outcome, answered.by);
        }
        if let Asker::Addon { .. } = answered.ask.asker {
            self.deliver(agent_id, &answered.ask.asker, &answered.outcome);
        }
    }

    /// Hands an outcome to whoever asked the user.
    fn deliver(&mut self, agent_id: &str, asker: &Asker, outcome: &addon::Outcome) {
        match *asker {
            Asker::Adapter(ref id) => {
                self.to_adapter(agent_id, wire::relay(id, &outcome.to_json()));
            }
            Asker::Addon { addon, id } => {
                self.hook(addon, agent_id, false, |addon, cx, _| {
                    addon.reply(cx, id, outcome);
                });
            }
        }
    }

    fn agent_message(&mut self, agent_id: &str, generation: u64, message: Value) {
        let Some(adapter) = self
            .agents
            .get_mut(agent_id)
            .and_then(|agent| agent.running.as_mut())
            .filter(|adapter| adapter.generation == generation)
        else {
            return;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (text(&message, "/method"), message.get("id")) {
            (None, Some(id)) => {
                if let Some(sent) = id.as_u64().and_then(|id| adapter.calls.remove(&id)) {
                    let layers = self.addons.len();
                    self.settle(agent_id, layers, sent, addon::Outcome::of(&message));
                }
            }
            (Some("session/update"), None) => self.agent_update(agent_id, params),
            (Some(method), Some(id)) => {
                self.ids += 1;
                let ask = Ask {
                    asker: Asker::Adapter(id.clone()),
                    method: method.to_owned(),
                    params,
                    request: self.ids,
                };
                let layers = self.addons.len();
                self.ask(agent_id, layers, ask);
            }
            (Some(method), None) => {
                self.ids += 1;
                let notification = OneWay {
                    id: self.ids,
                    source: Source::Adapter,
                    what: What::AgentNotification(method.to_owned()),
                    body: params,
                };
                let layers = self.addons.len();
                self.pass(agent_id, layers, notification);
            }
            _ => {}
        }
    }

    fn agent_answered(&mut self, agent_id: &str, generation: u64, call: Call, response: &Value) {
        match call {
            Call::Initialize => {
                let Some(result) = response.get("result") else {
                    return self.agent_stopped(
                        agent_id,
                        generation,
                        "the agent refused `initialize`",
                    );
                };
                let Some(adapter) = self.agents[agent_id].running.as_mut() else {
                    return;
                };
                adapter.initialized = Some(result.clone());
                for (client, message) in std::mem::take(&mut adapter.waiting) {
                    self.handle(client, message);
                }
            }
            Call::List { job, agent } => self.sessions_listed(job, &agent, response),
            Call::Fork {
                client,
                id,
                agent,
                cwd,
                setup,
            } => {
                let Some(session_id) = text(response, "/result/sessionId").map(str::to_owned)
                else {
                    return self.send(client, wire::relay(&id, response));
                };
                let at = now();
                let record = SessionRecord {
                    session_id: session_id.clone(),
                    agent_id: agent.clone(),
                    cwd,
                    title: None,
                    state: None,
                    setup,
                    head_seq: 0,
                    created_at: at,
                    updated_at: at,
                };
                if let Err(error) = self.store.put_session(&record) {
                    return self.refuse(client, &id, wire::INTERNAL_ERROR, &error.to_string());
                }
                // The copy comes with its history, which the adapter has not sent. Ask for it
                // the way an adopted session is loaded; until then apps that open it wait.
                let params = resume_params(&record, true);
                self.roots.insert(session_id.clone(), session_id.clone());
                self.sessions.insert(
                    session_id.clone(),
                    Session {
                        record,
                        live: false,
                        resuming: true,
                        adopting: true,
                        queued: Vec::new(),
                        subscribers: BTreeSet::from([client]),
                        pending: IndexMap::new(),
                    },
                );
                self.send(client, wire::relay(&id, response));
                let call = Call::Resume { root: session_id };
                self.call(&agent, call, "session/resume", params);
            }
            Call::New {
                client,
                id,
                agent,
                cwd,
                setup,
            } => {
                let session_id = text(response, "/result/sessionId").map(str::to_owned);
                let Some(session_id) = session_id else {
                    return self.send(client, wire::relay(&id, response));
                };
                let at = now();
                let record = SessionRecord {
                    session_id: session_id.clone(),
                    agent_id: agent,
                    cwd,
                    title: None,
                    state: None,
                    setup,
                    head_seq: 0,
                    created_at: at,
                    updated_at: at,
                };
                if let Err(error) = self.store.put_session(&record) {
                    return self.refuse(client, &id, wire::INTERNAL_ERROR, &error.to_string());
                }
                self.harness.remove(&session_id);
                self.roots.insert(session_id.clone(), session_id.clone());
                self.sessions.insert(
                    session_id.clone(),
                    Session {
                        record,
                        live: true,
                        resuming: false,
                        adopting: false,
                        queued: Vec::new(),
                        subscribers: BTreeSet::from([client]),
                        pending: IndexMap::new(),
                    },
                );
                self.send(client, wire::relay(&id, response));
                self.announce(&session_id);
                self.log_setup(&session_id, &response["result"]);
                let early = self.agents[agent_id]
                    .running
                    .as_mut()
                    .map(|adapter| std::mem::take(&mut adapter.early))
                    .unwrap_or_default();
                for params in early {
                    self.agent_update(agent_id, params);
                }
            }
            Call::Resume { root } => {
                let Some(session) = self.sessions.get_mut(&root) else {
                    return;
                };
                session.resuming = false;
                let adopting = std::mem::take(&mut session.adopting);
                let queued = std::mem::take(&mut session.queued);
                if let Some(result) = response.get("result") {
                    session.live = true;
                    self.log_setup(&root, result);
                    self.announce(&root);
                    for (client, message) in queued {
                        self.handle(client, message);
                    }
                } else {
                    if adopting {
                        // Nothing usable was loaded: do not keep a half-empty log.
                        self.forget(&root);
                    }
                    for (client, message) in queued {
                        if let Some(id) = message.get("id") {
                            self.send(client, wire::relay(id, response));
                        }
                    }
                }
            }
            Call::Delete {
                client,
                id,
                session,
            } => {
                if response.get("error").is_none() {
                    self.removed(&session);
                }
                self.send(client, wire::relay(&id, response));
            }
            Call::Forward { client, id } => self.send(client, wire::relay(&id, response)),
            Call::Addon { addon, id } => {
                let outcome = addon::Outcome::of(response);
                self.hook(addon, agent_id, false, |addon, cx, _| {
                    addon.reply(cx, id, &outcome);
                });
            }
        }
    }

    fn agent_update(&mut self, agent_id: &str, params: Value) {
        let (Some(session_id), Some(update)) = (text(&params, "/sessionId"), params.get("update"))
        else {
            return;
        };
        let Some(root) = self.roots.get(session_id).cloned() else {
            // The session's `session/new` has not been answered yet.
            if let Some(adapter) = self.agents[agent_id].running.as_mut()
                && adapter.early.len() < MAX_EARLY_UPDATES
            {
                adapter.early.push(params);
            }
            return;
        };
        if let Some(child) = subagent_session(update) {
            self.roots.insert(child.to_owned(), root.clone());
        }
        if text(update, "/sessionUpdate") == Some("replay_complete") {
            return; // The end of the harness's own replay, while adopting. Not content.
        }
        self.ids += 1;
        let update = OneWay {
            id: self.ids,
            source: Source::Adapter,
            what: What::Update(session_id.to_owned()),
            body: update.clone(),
        };
        let layers = self.addons.len();
        self.pass(agent_id, layers, update);
    }

    /// An adapter is gone (it exited, or the daemon gives up on it). Everything waiting on it
    /// fails, and its sessions stop being live. The next request for one of them starts it again.
    fn agent_stopped(&mut self, agent_id: &str, generation: u64, why: &str) {
        let Some(agent) = self.agents.get_mut(agent_id) else {
            return;
        };
        if agent.running.as_ref().map(|adapter| adapter.generation) != Some(generation) {
            return;
        }
        let Some(adapter) = agent.running.take() else {
            return;
        };
        agent.last_error = Some(why.to_owned());
        // What addons held for this adapter ends with it. A call of an addon's own method
        // belongs to no adapter and stays.
        let held: Vec<u64> = self
            .held
            .iter()
            .filter(|(_, held)| {
                held.agent == agent_id
                    && !matches!(held.what, HeldWhat::Method(_) | HeldWhat::Edge(_))
            })
            .map(|(id, _)| *id)
            .collect();
        let mut unanswered: Vec<Sent> = adapter.calls.into_values().collect();
        for id in held {
            match self.held.remove(&id).map(|held| held.what) {
                Some(HeldWhat::Request(sent) | HeldWhat::Response(sent, _)) => {
                    unanswered.push(*sent);
                }
                Some(HeldWhat::Ask(ask)) => {
                    self.deliver(agent_id, &ask.asker, &cancelled(&ask.method));
                }
                Some(HeldWhat::Answer(answered)) => {
                    let method = &answered.ask.method;
                    if let Some(entry) = &answered.entry
                        && let Some(root) = self.roots.get(&entry.session_id).cloned()
                    {
                        let by = Some("daemon".to_owned());
                        self.resolve(&root, entry, wire::cancelled(method), by);
                    }
                    self.deliver(agent_id, &answered.ask.asker, &cancelled(method));
                }
                _ => {}
            }
        }
        for sent in unanswered {
            self.fail(agent_id, sent.call, why);
        }
        for (client, message) in adapter.waiting {
            let Some(id) = message.get("id") else {
                continue;
            };
            if text(&message, "/method") == Some("initialize") {
                let result = self.initialize_result(client);
                self.reply(client, id, result);
            } else {
                self.refuse(client, id, wire::INTERNAL_ERROR, why);
            }
        }
        let stopped: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.live && session.record.agent_id == agent_id)
            .map(|(root, _)| root.clone())
            .collect();
        for root in stopped {
            if let Some(session) = self.sessions.get_mut(&root) {
                session.live = false;
            }
            self.end_turn(&root, why);
            self.announce(&root);
        }
        for index in (0..self.addons.len()).rev() {
            self.hook(index, agent_id, true, |addon, cx, agent| {
                addon.stopped(cx, &agent, why);
            });
        }
    }
}

impl Release {
    /// The outcome a held response or answer goes on with.
    fn over(self, was: addon::Outcome, dropped: addon::Outcome) -> addon::Outcome {
        match self {
            Self::Pass(None) => was,
            Self::Pass(Some(result)) => addon::Outcome::Result(result),
            Self::Outcome(outcome) => outcome,
            Self::Discard => dropped,
        }
    }
}

/// The number addons know a held request by.
fn what_id(what: &HeldWhat) -> u64 {
    match what {
        HeldWhat::Request(sent) | HeldWhat::Response(sent, _) => sent.request,
        HeldWhat::Method(call) => call.request,
        HeldWhat::OneWay(message) => message.id,
        HeldWhat::Edge(edge) => edge.id,
        HeldWhat::Ask(ask) => ask.request,
        HeldWhat::Answer(answered) => answered.ask.request,
    }
}

/// What whoever asked the user is told when the request can no longer be answered.
fn cancelled(method: &str) -> addon::Outcome {
    let outcome = wire::cancelled(method);
    match method {
        "session/request_permission" | "elicitation/create" => addon::Outcome::Result(outcome),
        _ => addon::Outcome::of(&json!({ "error": outcome })),
    }
}

/// The params to resume a session at its adapter, with or without a replay of its history.
fn resume_params(record: &SessionRecord, replay: bool) -> Value {
    let mut params = json!({
        "sessionId": record.session_id,
        "cwd": record.cwd,
        "mcpServers": record.setup.get("mcpServers").cloned().unwrap_or_else(|| json!([])),
    });
    if let Some(directories) = record.setup.get("additionalDirectories") {
        params["additionalDirectories"] = directories.clone();
    }
    if replay {
        params["replayFrom"] = json!({"type": "start"});
    }
    params
}

/// The child session id that a `subagent_update` announces.
fn subagent_session(update: &Value) -> Option<&str> {
    (text(update, "/sessionUpdate") == Some("subagent_update"))
        .then(|| text(update, "/sessionId"))
        .flatten()
}

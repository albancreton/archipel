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
//! | app | `session/new` | pass to the adapter, record the session, attach the app |
//! | app | any other request for a session | pass to the adapter, waking the session first if needed |
//!
//! A session is *live* while an adapter process holds it. A session that is not live can still be
//! attached to and replayed from the log; the first request for it restarts the adapter and
//! resumes the session there.

use crate::wire;
use bridge_agents::{AgentSpec, initialize_params};
use bridge_core::{
    log::{LogEntry, Record, condense},
    state::SessionState,
    store::{SessionRecord, Store, rfc3339},
};
use bridge_schema::{ext, rules::RuleSet};
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
}

struct Client {
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
    calls: HashMap<u64, Call>,
    /// App requests held back until the adapter has answered `initialize`.
    waiting: Vec<(u64, Value)>,
    /// Updates for a session id not known yet. See [`MAX_EARLY_UPDATES`].
    early: Vec<Value>,
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
    },
    /// The daemon waking a session that is not live.
    Resume {
        root: String,
    },
    /// Any other app request: the answer goes back to the app unchanged.
    Forward {
        client: u64,
        id: Value,
    },
}

struct Session {
    record: SessionRecord,
    /// An adapter process holds the session.
    live: bool,
    /// The daemon has asked the adapter to resume the session and waits for the answer.
    resuming: bool,
    /// App requests held back until the session is live again.
    queued: Vec<(u64, Value)>,
    /// Attached apps.
    subscribers: BTreeSet<u64>,
    /// Requests from the agent that nobody has answered, by request id.
    pending: IndexMap<String, Pending>,
}

struct Pending {
    /// The JSON-RPC id the adapter used, for the answer.
    agent_request_id: Value,
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
        }
    }

    pub(crate) async fn run(mut self, mut inbox: mpsc::Receiver<Event>) {
        while let Some(event) = inbox.recv().await {
            match event {
                Event::ClientOpened { client, tx } => {
                    self.clients.insert(client, Client { tx, name: None });
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

    /// Queues a message for an app. An app whose queue is full is disconnected: it can attach
    /// again and catch up from the log, which is cheaper than holding everything for it.
    fn send(&mut self, client: u64, message: Value) {
        if let Some(entry) = self.clients.get(&client)
            && entry.tx.try_send(message).is_err()
        {
            self.drop_client(client);
        }
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
    }

    // ---- messages from apps ----------------------------------------------------------------

    fn client_message(&mut self, client: u64, message: Value) {
        if !self.clients.contains_key(&client) {
            return;
        }
        let method = text(&message, "/method").map(str::to_owned);
        match (method, message.get("id").cloned()) {
            (Some(method), Some(id)) => self.client_request(client, &method, id, message),
            (Some(_), None) => self.client_notification(&message),
            (None, Some(id)) => self.client_answer(client, &id, &message),
            (None, None) => {}
        }
    }

    fn client_request(&mut self, client: u64, method: &str, id: Value, message: Value) {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
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
                    let result = self.initialize_result();
                    self.reply(client, &id, result);
                }
            }
            ext::METHOD_AGENTS_LIST => {
                let agents = self.agent_infos();
                self.reply(client, &id, json!({ "agents": agents }));
            }
            ext::METHOD_SESSIONS_LIST => self.list_sessions(client, &id, &params),
            ext::METHOD_SESSION_ATTACH => self.attach(client, &id, params),
            ext::METHOD_SESSION_DETACH => {
                if let Some(root) = text(&params, "/sessionId").and_then(|id| self.roots.get(id))
                    && let Some(session) = self.sessions.get_mut(root)
                {
                    session.subscribers.remove(&client);
                }
                self.reply(client, &id, json!({}));
            }
            "session/resume" => self.resume_for_client(client, &id, &params),
            "session/new" => self.new_session(client, id, &message),
            _ => match text(&params, "/sessionId").map(str::to_owned) {
                Some(session_id) => self.forward(client, id, &session_id, message),
                None => self.forward_to_default_agent(client, id, &message),
            },
        }
    }

    /// What the daemon answers to an app's `initialize`: the default agent's capabilities under
    /// the daemon's own name. Without a working agent the capabilities are empty, and the app
    /// can still list and read sessions.
    fn initialize_result(&self) -> Value {
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
                ],
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
                    } else {
                        ext::AgentReadiness::Ready
                    },
                    detail: agent.last_error.clone().filter(|_| failed),
                }
            })
            .collect()
    }

    fn list_sessions(&mut self, client: u64, id: &Value, params: &Value) {
        let only = text(params, "/agentId");
        match self.store.sessions() {
            Ok(records) => {
                let sessions: Vec<ext::SessionSummary> = records
                    .into_iter()
                    .filter(|record| only.is_none_or(|agent| agent == record.agent_id))
                    .map(|record| ext::SessionSummary {
                        live: self
                            .sessions
                            .get(&record.session_id)
                            .is_some_and(|session| session.live),
                        updated_at: Some(rfc3339(record.updated_at)),
                        session_id: record.session_id,
                        agent_id: record.agent_id,
                        title: record.title,
                        cwd: record.cwd,
                        state: record.state,
                        head_seq: record.head_seq,
                    })
                    .collect();
                self.reply(client, id, json!({ "sessions": sessions }));
            }
            Err(error) => self.refuse(client, id, wire::INTERNAL_ERROR, &error.to_string()),
        }
    }

    fn attach(&mut self, client: u64, id: &Value, params: Value) {
        let Ok(request) = serde_json::from_value::<ext::AttachSessionRequest>(params) else {
            return self.refuse(client, id, wire::INVALID_PARAMS, "expected a sessionId");
        };
        let Some(root) = self.load(&request.session_id) else {
            return self.refuse(client, id, wire::INVALID_PARAMS, "unknown session");
        };
        let head = self.sessions[&root].record.head_seq;
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
                head_seq: head
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
    fn resume_for_client(&mut self, client: u64, id: &Value, params: &Value) {
        let Some(root) = text(params, "/sessionId").and_then(|session| self.load(session)) else {
            return self.refuse(client, id, wire::INVALID_PARAMS, "unknown session");
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
            let head = self.sessions[&root].record.head_seq;
            self.catch_up(client, &root, Some(head));
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
                let call = Call::New {
                    client,
                    id,
                    agent: agent.clone(),
                    cwd,
                };
                self.call(&agent, call, "session/new", params);
            }
            Ready::Later => {}
            Ready::No(why) => self.refuse(client, &id, wire::INTERNAL_ERROR, &why),
        }
    }

    /// Passes an app's request about a session to the adapter that holds it.
    fn forward(&mut self, client: u64, id: Value, session_id: &str, message: Value) {
        let Some(root) = self.load(session_id) else {
            return self.refuse(client, &id, wire::INVALID_PARAMS, "unknown session");
        };
        let session = &self.sessions[&root];
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
                    let params = json!({
                        "sessionId": root,
                        "cwd": session.record.cwd,
                        "mcpServers": [],
                    });
                    self.call(&agent, Call::Resume { root }, "session/resume", params);
                }
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

    /// A notification from an app, such as `session/cancel`: passed on if the session is live.
    fn client_notification(&mut self, message: &Value) {
        let Some(session) = text(message, "/params/sessionId")
            .and_then(|id| self.roots.get(id))
            .and_then(|root| self.sessions.get(root))
            .filter(|session| session.live)
        else {
            return;
        };
        if let Some(adapter) = self
            .agents
            .get(&session.record.agent_id)
            .and_then(|agent| agent.running.as_ref())
        {
            let _ = adapter.tx.try_send(message.clone());
        }
    }

    /// An app answers an approval or a question. Only the first answer counts.
    fn client_answer(&mut self, client: u64, id: &Value, message: &Value) {
        let Some(request_id) = id.as_str() else {
            return;
        };
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
        self.requests.remove(request_id);
        if let Some(adapter) = self
            .agents
            .get(&session.record.agent_id)
            .and_then(|agent| agent.running.as_ref())
        {
            let _ = adapter
                .tx
                .try_send(wire::relay(&pending.agent_request_id, message));
        }
        let outcome = message
            .get("result")
            .or_else(|| message.get("error"))
            .cloned()
            .unwrap_or(Value::Null);
        let resolved_by = self
            .clients
            .get(&client)
            .and_then(|entry| entry.name.clone());
        self.resolve(&root, &pending.entry, outcome, resolved_by);
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
                if changed && let Err(error) = self.store.put_session(&session.record) {
                    eprintln!("bridge: {error}");
                }
            }
        }
        let message = wire::entry(root, &entry);
        let subscribers: Vec<u64> = session.subscribers.iter().copied().collect();
        for client in subscribers {
            self.send(client, message.clone());
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
        for pending in pending {
            if let Record::Request {
                request_id, method, ..
            } = &pending.entry.record
            {
                self.requests.remove(request_id);
                let outcome = wire::cancelled(method);
                self.resolve(root, &pending.entry, outcome, Some("daemon".to_owned()));
            }
        }
        if was_busy {
            for update in [
                json!({"sessionUpdate": "notice", "severity": "error", "title": "The turn was interrupted", "description": why}),
                json!({"sessionUpdate": "state_update", "state": "idle", "stopReason": "cancelled", "background": []}),
            ] {
                let update = wire::own_update(update);
                self.log(root, root, Record::Update { update });
            }
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
        let _ = tx.try_send(wire::request(0, "initialize", initialize_params()));
        agent.last_error = None;
        agent.running = Some(Adapter {
            tx,
            _guard: running.guard,
            generation,
            initialized: None,
            next_id: 1,
            calls: HashMap::from([(0, Call::Initialize)]),
            waiting: Vec::new(),
            early: Vec::new(),
        });
        Ok(())
    }

    /// Sends a request to an adapter and remembers what to do with the answer.
    fn call(&mut self, agent_id: &str, call: Call, method: &str, params: Value) {
        let Some(adapter) = self
            .agents
            .get_mut(agent_id)
            .and_then(|agent| agent.running.as_mut())
        else {
            return;
        };
        let id = adapter.next_id;
        adapter.next_id += 1;
        adapter.calls.insert(id, call);
        if adapter
            .tx
            .try_send(wire::request(id, method, params))
            .is_err()
        {
            let generation = adapter.generation;
            self.agent_stopped(agent_id, generation, "the agent stopped responding");
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
                if let Some(call) = id.as_u64().and_then(|id| adapter.calls.remove(&id)) {
                    self.agent_answered(agent_id, generation, call, &message);
                }
            }
            (Some("session/update"), None) => self.agent_update(agent_id, params),
            (Some(method), Some(id)) => self.agent_request(agent_id, method, id, params),
            // Other notifications are neither stored nor passed on. Claude's adapter sends
            // `auth/status_update` with the account's email address.
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
                    self.client_message(client, message);
                }
            }
            Call::New {
                client,
                id,
                agent,
                cwd,
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
                    head_seq: 0,
                    created_at: at,
                    updated_at: at,
                };
                if let Err(error) = self.store.put_session(&record) {
                    return self.refuse(client, &id, wire::INTERNAL_ERROR, &error.to_string());
                }
                self.roots.insert(session_id.clone(), session_id.clone());
                self.sessions.insert(
                    session_id.clone(),
                    Session {
                        record,
                        live: true,
                        resuming: false,
                        queued: Vec::new(),
                        subscribers: BTreeSet::from([client]),
                        pending: IndexMap::new(),
                    },
                );
                self.send(client, wire::relay(&id, response));
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
                let queued = std::mem::take(&mut session.queued);
                if let Some(result) = response.get("result") {
                    session.live = true;
                    self.log_setup(&root, result);
                    for (client, message) in queued {
                        self.client_message(client, message);
                    }
                } else {
                    for (client, message) in queued {
                        if let Some(id) = message.get("id") {
                            self.send(client, wire::relay(id, response));
                        }
                    }
                }
            }
            Call::Forward { client, id } => self.send(client, wire::relay(&id, response)),
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
        let record = Record::Update {
            update: update.clone(),
        };
        self.log(&root, session_id, record);
    }

    /// The agent asks the user something. It stays pending until an app answers, however long
    /// that takes and whether or not an app is attached right now.
    fn agent_request(&mut self, agent_id: &str, method: &str, id: &Value, params: Value) {
        let session_id = text(&params, "/sessionId").map(str::to_owned);
        let root = session_id
            .as_ref()
            .and_then(|session| self.roots.get(session))
            .filter(|root| self.sessions.contains_key(*root))
            .cloned();
        let logged = match (&root, &session_id) {
            (Some(root), Some(session_id)) => {
                let request_id = format!("{root}#{}", self.sessions[root].record.head_seq + 1);
                let record = Record::Request {
                    request_id,
                    method: method.to_owned(),
                    params,
                    outcome: None,
                };
                self.log(root, session_id, record)
            }
            _ => None,
        };
        let (Some(root), Some(entry)) = (root, logged) else {
            if let Some(adapter) = self.agents[agent_id].running.as_ref() {
                let refusal =
                    wire::error(id, wire::METHOD_NOT_FOUND, "not supported by the daemon");
                let _ = adapter.tx.try_send(refusal);
            }
            return;
        };
        if let (Record::Request { request_id, .. }, Some(session)) =
            (&entry.record, self.sessions.get_mut(&root))
        {
            self.requests.insert(request_id.clone(), root.clone());
            session.pending.insert(
                request_id.clone(),
                Pending {
                    agent_request_id: id.clone(),
                    entry,
                },
            );
        }
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
        for (_, call) in adapter.calls {
            match call {
                Call::New { client, id, .. } | Call::Forward { client, id } => {
                    self.refuse(client, &id, wire::INTERNAL_ERROR, why);
                }
                Call::Resume { root } => {
                    if let Some(session) = self.sessions.get_mut(&root) {
                        session.resuming = false;
                        for (client, message) in std::mem::take(&mut session.queued) {
                            if let Some(id) = message.get("id") {
                                self.refuse(client, id, wire::INTERNAL_ERROR, why);
                            }
                        }
                    }
                }
                Call::Initialize => {}
            }
        }
        for (client, message) in adapter.waiting {
            let Some(id) = message.get("id") else {
                continue;
            };
            if text(&message, "/method") == Some("initialize") {
                let result = self.initialize_result();
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
        }
    }
}

/// The child session id that a `subagent_update` announces.
fn subagent_session(update: &Value) -> Option<&str> {
    (text(update, "/sessionUpdate") == Some("subagent_update"))
        .then(|| text(update, "/sessionId"))
        .flatten()
}

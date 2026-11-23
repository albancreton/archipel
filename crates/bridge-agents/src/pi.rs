//! The Pi profile: Pi's adapter (`pi-acp`, ACP v1) presented to the daemon as an ACP v2 adapter.
//!
//! Two things differ from Claude's adapter, and both are hidden here so the daemon has one way
//! of working:
//!
//! **The protocol.** `pi-acp` speaks ACP v1. This module translates:
//!
//! | `pi-acp` (v1) | What the daemon sees (v2) |
//! |---|---|
//! | `session/prompt` answers when the turn ends | answers at once with a `messageId`; `state_update` `running`, then `idle` with the stop reason |
//! | no echo of the user's message | `user_message` for each prompt |
//! | message and thought chunks without ids | a `messageId` per run of chunks |
//! | `tool_call`, `tool_call_update` | `tool_call_update` |
//! | command output under `_meta.terminal_*` | `terminal_update`, `terminal_output_chunk` |
//! | diff as old and new text | diff as a patch |
//! | config options keyed by `id` | keyed by `configId` |
//! | startup banner and "Queued message" as assistant text | dropped |
//! | `session/load`, which always replays | `session/resume`, replaying only when asked |
//!
//! **Processes.** One `pi-acp` (and its `pi`) serves one session. This module starts one per
//! session, stops those that have been idle for a while, and starts them again with
//! `session/load` when the session is used. The daemon sees one adapter with many sessions that
//! are always there.

use crate::{AgentSpec, Launcher, Running};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bridge_transport::{Link, QUEUE};
use serde_json::{Map, Value, json};
use std::{
    any::Any,
    collections::HashMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

/// Makes ids for messages. The default is random; tests use a counter.
pub type Ids = Arc<dyn Fn() -> String + Send + Sync>;

/// The Pi profile. As a [`Launcher`] it starts the translator, which starts `pi-acp` as needed.
#[derive(Clone)]
pub struct Pi {
    /// Starts one `pi-acp` process.
    pub adapter: Arc<dyn Launcher>,
    /// The model to select in every new session. Without it Pi uses its own default.
    pub model: Option<String>,
    /// A session's process is stopped after this long without a turn.
    pub idle: Duration,
    pub ids: Ids,
}

impl Pi {
    #[must_use]
    pub fn new(adapter: Arc<dyn Launcher>) -> Self {
        Self {
            adapter,
            model: None,
            idle: Duration::from_secs(600),
            ids: Arc::new(random_id),
        }
    }

    /// The agent `pi` built on this profile.
    #[must_use]
    pub fn spec(self) -> AgentSpec {
        AgentSpec {
            id: "pi".to_owned(),
            name: "Pi".to_owned(),
            harness: Some("pi".to_owned()),
            launcher: Arc::new(self),
        }
    }
}

impl Launcher for Pi {
    fn launch(&self) -> io::Result<Running> {
        let (ours, theirs) = Link::pair();
        tokio::spawn(run(self.clone(), theirs));
        Ok(Running {
            link: ours,
            guard: None,
        })
    }
}

fn random_id() -> String {
    let mut bytes = [0u8; 12];
    let _ = getrandom::fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn run(config: Pi, daemon: Link) {
    let (events, mut inbox) = mpsc::channel(QUEUE);
    let Link {
        tx: to_daemon,
        rx: mut from_daemon,
    } = daemon;
    let mut tick = tokio::time::interval((config.idle / 2).max(Duration::from_millis(10)));
    let mut translator = Translator {
        config,
        daemon: to_daemon,
        events,
        processes: HashMap::new(),
        sessions: HashMap::new(),
        spare: None,
        welcome: None,
        greeting: Vec::new(),
        asks: HashMap::new(),
        next: 0,
    };
    loop {
        tokio::select! {
            message = from_daemon.recv() => match message {
                Some(message) => translator.daemon_message(message),
                None => return,
            },
            Some((process, message)) = inbox.recv() => match message {
                Some(message) => translator.adapter_message(process, message),
                None => translator.process_gone(process, "the Pi process stopped"),
            },
            _ = tick.tick() => translator.stop_idle(),
        }
    }
}

struct Translator {
    config: Pi,
    daemon: mpsc::Sender<Value>,
    /// For the tasks that read from `pi-acp` processes.
    events: mpsc::Sender<(u64, Option<Value>)>,
    processes: HashMap<u64, Process>,
    sessions: HashMap<String, Session>,
    /// A started process that has no session yet.
    spare: Option<u64>,
    /// `pi-acp`'s `initialize` result, once one process has answered.
    welcome: Option<Value>,
    /// Ids of the daemon's `initialize` requests that wait for `welcome`.
    greeting: Vec<Value>,
    /// Requests passed from a process to the daemon: our id to the process and its id.
    asks: HashMap<String, (u64, Value)>,
    next: u64,
}

struct Process {
    tx: mpsc::Sender<Value>,
    _guard: Option<Box<dyn Any + Send>>,
    /// False until the process has answered `initialize`.
    ready: bool,
    /// Calls held back until it is ready.
    held: Vec<(Call, &'static str, Value)>,
    next_id: u64,
    calls: HashMap<u64, Call>,
    session: Option<String>,
    used: Instant,
}

/// Why a request was sent to a process, and what to do with the answer.
enum Call {
    Initialize,
    New {
        id: Value,
        cwd: String,
    },
    /// Selecting the configured model right after `session/new`.
    NewModel {
        id: Value,
        session: String,
    },
    Prompt {
        session: String,
    },
    Config {
        id: Value,
    },
    Load {
        session: String,
        id: Option<Value>,
    },
    /// The answer goes back to the daemon unchanged.
    Relay {
        id: Value,
    },
}

struct Session {
    cwd: String,
    process: Option<u64>,
    /// Prompts sent whose turn has not ended.
    outstanding: usize,
    /// `session/load` is under way.
    loading: bool,
    /// The daemon asked for the history, so the reload's replay is passed on.
    replaying: bool,
    /// The run of chunks being streamed: its kind and the `messageId` given to it.
    stream: Option<(String, String)>,
    /// Daemon messages held back until the session's process is up again.
    waiting: Vec<Value>,
}

fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": message}})
}

/// A response with the same outcome under another id.
fn relay(id: &Value, response: &Value) -> Value {
    match response.get("error") {
        Some(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        None => result(id, response.get("result").cloned().unwrap_or(Value::Null)),
    }
}

impl Translator {
    fn send(&self, message: Value) {
        let _ = self.daemon.try_send(message);
    }

    fn emit(&self, session: &str, update: Value) {
        self.send(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": session, "update": update,
            }}),
        );
    }

    // ---- processes -------------------------------------------------------------------------

    fn start(&mut self) -> Result<u64, String> {
        let running = self
            .config
            .adapter
            .launch()
            .map_err(|error| format!("could not start pi-acp: {error}"))?;
        self.next += 1;
        let id = self.next;
        let Link { tx, mut rx } = running.link;
        let events = self.events.clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if events.send((id, Some(message))).await.is_err() {
                    return;
                }
            }
            let _ = events.send((id, None)).await;
        });
        let hello = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": 1,
            "clientInfo": {"name": "harness-bridge", "version": env!("CARGO_PKG_VERSION")},
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
        }});
        let _ = tx.try_send(hello);
        self.processes.insert(
            id,
            Process {
                tx,
                _guard: running.guard,
                ready: false,
                held: Vec::new(),
                next_id: 1,
                calls: HashMap::from([(0, Call::Initialize)]),
                session: None,
                used: Instant::now(),
            },
        );
        Ok(id)
    }

    /// Sends a request to a process, once it is ready.
    fn call(&mut self, process: u64, call: Call, method: &'static str, params: Value) {
        let Some(entry) = self.processes.get_mut(&process) else {
            return;
        };
        entry.used = Instant::now();
        if !entry.ready {
            entry.held.push((call, method, params));
            return;
        }
        let id = entry.next_id;
        entry.next_id += 1;
        entry.calls.insert(id, call);
        let _ = entry
            .tx
            .try_send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }

    /// A process without a session: the spare one, or a new one.
    fn free_process(&mut self) -> Result<u64, String> {
        match self.spare.take() {
            Some(spare) => Ok(spare),
            None => self.start(),
        }
    }

    // ---- from the daemon -------------------------------------------------------------------

    fn daemon_message(&mut self, message: Value) {
        let method = text(&message, "/method").map(str::to_owned);
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method.as_deref(), id) {
            (Some("initialize"), Some(id)) => {
                if self.welcome.is_some() {
                    return self.send(result(&id, self.welcome_v2()));
                }
                self.greeting.push(id);
                if self.processes.is_empty() {
                    match self.start() {
                        Ok(process) => self.spare = Some(process),
                        Err(why) => self.greet_failed(&why),
                    }
                }
            }
            (Some("session/new"), Some(id)) => match self.free_process() {
                Ok(process) => {
                    let servers = params
                        .get("mcpServers")
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                    let cwd = text(&params, "/cwd").unwrap_or_default().to_owned();
                    let params = json!({"cwd": cwd, "mcpServers": servers});
                    self.call(process, Call::New { id, cwd }, "session/new", params);
                }
                Err(why) => self.send(failure(&id, &why)),
            },
            (Some("session/list"), Some(id)) => {
                let process = match self.spare.or_else(|| self.processes.keys().next().copied()) {
                    Some(process) => Ok(process),
                    None => self.start().inspect(|process| self.spare = Some(*process)),
                };
                match process {
                    Ok(process) => self.call(process, Call::Relay { id }, "session/list", params),
                    Err(why) => self.send(failure(&id, &why)),
                }
            }
            (Some("session/resume"), Some(id)) => {
                let Some(session) = text(&params, "/sessionId").map(str::to_owned) else {
                    return self.send(failure(&id, "expected a sessionId"));
                };
                if self
                    .sessions
                    .get(&session)
                    .is_some_and(|known| known.process.is_some())
                {
                    return self.send(result(&id, json!({}))); // it is already up
                }
                let cwd = text(&params, "/cwd").unwrap_or_default().to_owned();
                let replay = params.get("replayFrom").is_some();
                self.load(&session, cwd, Some(id), replay);
            }
            (Some(_), _) => match text(&params, "/sessionId").map(str::to_owned) {
                Some(session) => self.for_session(&session, message),
                None => {
                    if let Some(id) = message.get("id") {
                        self.send(failure(id, "not supported by the Pi profile"));
                    }
                }
            },
            // The daemon answers a request that a process made.
            (None, Some(id)) => {
                let asked = id.as_str().and_then(|id| self.asks.remove(id));
                if let Some((process, their_id)) = asked
                    && let Some(entry) = self.processes.get(&process)
                {
                    let _ = entry.tx.try_send(relay(&their_id, &message));
                }
            }
            (None, None) => {}
        }
    }

    /// Handles a daemon message about a session, starting the session's process first if it
    /// was stopped.
    fn for_session(&mut self, session: &str, message: Value) {
        let Some(entry) = self.sessions.get_mut(session) else {
            if let Some(id) = message.get("id") {
                self.send(failure(id, "unknown session"));
            }
            return;
        };
        if entry.process.is_some() && !entry.loading {
            return self.act(session, &message);
        }
        entry.waiting.push(message);
        if entry.process.is_none() {
            let cwd = entry.cwd.clone();
            self.load(session, cwd, None, false);
        }
    }

    /// Starts a process for a session and reloads the session in it.
    fn load(&mut self, session: &str, cwd: String, id: Option<Value>, replay: bool) {
        let process = match self.free_process() {
            Ok(process) => process,
            Err(why) => {
                if let Some(id) = id {
                    self.send(failure(&id, &why));
                }
                return self.fail_waiting(session, &why);
            }
        };
        let entry = self.sessions.entry(session.to_owned()).or_insert(Session {
            cwd: cwd.clone(),
            process: None,
            outstanding: 0,
            loading: false,
            replaying: false,
            stream: None,
            waiting: Vec::new(),
        });
        entry.process = Some(process);
        entry.loading = true;
        entry.replaying = replay;
        entry.stream = None;
        if let Some(started) = self.processes.get_mut(&process) {
            started.session = Some(session.to_owned());
        }
        let params = json!({"sessionId": session, "cwd": cwd, "mcpServers": []});
        let call = Call::Load {
            session: session.to_owned(),
            id,
        };
        self.call(process, call, "session/load", params);
    }

    fn fail_waiting(&mut self, session: &str, why: &str) {
        let waiting = self
            .sessions
            .get_mut(session)
            .map(|entry| std::mem::take(&mut entry.waiting))
            .unwrap_or_default();
        for message in waiting {
            if let Some(id) = message.get("id") {
                self.send(failure(id, why));
            }
        }
    }

    /// Carries out a daemon message for a session whose process is up.
    fn act(&mut self, session: &str, message: &Value) {
        let Some(entry) = self.sessions.get_mut(session) else {
            return;
        };
        let Some(process) = entry.process else {
            return;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (text(message, "/method"), message.get("id").cloned()) {
            (Some("session/prompt"), Some(id)) => {
                let message_id = (self.config.ids)();
                let first = entry.outstanding == 0;
                entry.outstanding += 1;
                entry.stream = None;
                self.send(result(&id, json!({"messageId": message_id})));
                self.emit(
                    session,
                    json!({"sessionUpdate": "user_message", "messageId": message_id, "content": params["prompt"]}),
                );
                if first {
                    self.emit(
                        session,
                        json!({"sessionUpdate": "state_update", "state": "running", "background": []}),
                    );
                }
                let call = Call::Prompt {
                    session: session.to_owned(),
                };
                let params = json!({"sessionId": session, "prompt": params["prompt"]});
                self.call(process, call, "session/prompt", params);
            }
            (Some("session/set_config_option"), Some(id)) => {
                let params = json!({
                    "sessionId": session,
                    "configId": params["configId"],
                    "value": params["value"],
                });
                self.call(
                    process,
                    Call::Config { id },
                    "session/set_config_option",
                    params,
                );
            }
            (Some(method), Some(id)) => {
                // Not a method this profile knows: pass it on and let `pi-acp` decide.
                let Some(started) = self.processes.get_mut(&process) else {
                    return;
                };
                let call_id = started.next_id;
                started.next_id += 1;
                started.calls.insert(call_id, Call::Relay { id });
                let _ = started.tx.try_send(
                    json!({"jsonrpc": "2.0", "id": call_id, "method": method, "params": params}),
                );
            }
            (Some(_), None) => {
                // A notification, such as `session/cancel`: the same in v1.
                if let Some(started) = self.processes.get(&process) {
                    let _ = started.tx.try_send(message.clone());
                }
            }
            (None, _) => {}
        }
    }

    // ---- from pi-acp -----------------------------------------------------------------------

    fn adapter_message(&mut self, process: u64, message: Value) {
        let Some(entry) = self.processes.get_mut(&process) else {
            return; // from a process that was already stopped
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (text(&message, "/method"), message.get("id")) {
            (None, Some(id)) => {
                if let Some(call) = id.as_u64().and_then(|id| entry.calls.remove(&id)) {
                    self.answered(process, call, &message);
                }
            }
            (Some("session/update"), None) => {
                let (Some(session), Some(update)) =
                    (text(&params, "/sessionId"), params.get("update"))
                else {
                    return;
                };
                let Some(known) = self.sessions.get_mut(session) else {
                    return; // before `session/new` is answered: the startup banner and the like
                };
                if known.loading && !known.replaying {
                    return; // the replay of a reload nobody asked to see
                }
                for update in translate(known, update, &self.config.ids) {
                    self.emit(session, update);
                }
            }
            (Some(method), Some(id)) => {
                self.next += 1;
                let ask = format!("pi-ask-{}", self.next);
                self.asks.insert(ask.clone(), (process, id.clone()));
                let params = match method {
                    "session/request_permission" => permission_v2(params),
                    _ => params,
                };
                self.send(json!({"jsonrpc": "2.0", "id": ask, "method": method, "params": params}));
            }
            _ => {}
        }
    }

    fn answered(&mut self, process: u64, call: Call, response: &Value) {
        let answer = response.get("result");
        match call {
            Call::Initialize => {
                let Some(answer) = answer else {
                    return self.process_gone(process, "pi-acp refused `initialize`");
                };
                if self.welcome.is_none() {
                    self.welcome = Some(answer.clone());
                }
                for id in std::mem::take(&mut self.greeting) {
                    self.send(result(&id, self.welcome_v2()));
                }
                let held = match self.processes.get_mut(&process) {
                    Some(entry) => {
                        entry.ready = true;
                        std::mem::take(&mut entry.held)
                    }
                    None => Vec::new(),
                };
                for (call, method, params) in held {
                    self.call(process, call, method, params);
                }
            }
            Call::New { id, cwd } => {
                let Some(session) = answer.and_then(|answer| text(answer, "/sessionId")) else {
                    self.send(relay(&id, response));
                    self.processes.remove(&process);
                    return;
                };
                let session = session.to_owned();
                if let Some(entry) = self.processes.get_mut(&process) {
                    entry.session = Some(session.clone());
                }
                self.sessions.insert(
                    session.clone(),
                    Session {
                        cwd,
                        process: Some(process),
                        outstanding: 0,
                        loading: false,
                        replaying: false,
                        stream: None,
                        waiting: Vec::new(),
                    },
                );
                match self.config.model.clone() {
                    Some(model) => {
                        let params =
                            json!({"sessionId": session, "configId": "model", "value": model});
                        let call = Call::NewModel { id, session };
                        self.call(process, call, "session/set_config_option", params);
                    }
                    None => {
                        let options =
                            config_options_v2(answer.and_then(|a| a.get("configOptions")));
                        self.send(result(
                            &id,
                            json!({"sessionId": session, "configOptions": options}),
                        ));
                    }
                }
            }
            Call::NewModel { id, session } => {
                let options = config_options_v2(answer.and_then(|a| a.get("configOptions")));
                self.send(result(
                    &id,
                    json!({"sessionId": session, "configOptions": options}),
                ));
                if answer.is_none() {
                    self.emit(
                        &session,
                        notice(
                            "warning",
                            "The configured model could not be selected",
                            response,
                        ),
                    );
                }
            }
            Call::Prompt { session } => {
                let Some(entry) = self.sessions.get_mut(&session) else {
                    return;
                };
                entry.outstanding = entry.outstanding.saturating_sub(1);
                let over = entry.outstanding == 0;
                if over {
                    entry.stream = None;
                }
                if answer.is_none() {
                    self.emit(&session, notice("error", "The prompt failed", response));
                }
                if over {
                    let mut state =
                        json!({"sessionUpdate": "state_update", "state": "idle", "background": []});
                    if let Some(reason) = answer.and_then(|answer| answer.get("stopReason")) {
                        state["stopReason"] = reason.clone();
                    }
                    self.emit(&session, state);
                }
            }
            Call::Config { id } => match answer {
                Some(answer) => {
                    let options = config_options_v2(answer.get("configOptions"));
                    self.send(result(&id, json!({ "configOptions": options })));
                }
                None => self.send(relay(&id, response)),
            },
            Call::Relay { id } => self.send(relay(&id, response)),
            Call::Load { session, id } => {
                let Some(entry) = self.sessions.get_mut(&session) else {
                    return;
                };
                entry.loading = false;
                let replayed = std::mem::take(&mut entry.replaying);
                entry.stream = None;
                let Some(answer) = answer else {
                    entry.process = None;
                    self.processes.remove(&process);
                    if let Some(id) = &id {
                        self.send(relay(id, response));
                    }
                    return self.fail_waiting(&session, "the session could not be reloaded");
                };
                let waiting = std::mem::take(&mut entry.waiting);
                if let Some(id) = id {
                    // What ACP asks of a resume: the replay if any, the current state, the answer.
                    if replayed {
                        self.emit(&session, json!({"sessionUpdate": "replay_complete"}));
                    }
                    self.emit(
                        &session,
                        json!({"sessionUpdate": "state_update", "state": "idle", "background": []}),
                    );
                    let options = config_options_v2(answer.get("configOptions"));
                    self.send(result(&id, json!({ "configOptions": options })));
                }
                for message in waiting {
                    self.act(&session, &message);
                }
            }
        }
    }

    /// A process ended, or is given up on. Whatever waited on it fails.
    fn process_gone(&mut self, process: u64, why: &str) {
        let Some(entry) = self.processes.remove(&process) else {
            return;
        };
        if self.spare == Some(process) {
            self.spare = None;
        }
        if self.welcome.is_none() && self.processes.is_empty() {
            self.greet_failed(why);
        }
        for (_, call) in entry
            .calls
            .into_iter()
            .chain(entry.held.into_iter().map(|(call, _, _)| (0, call)))
        {
            match call {
                Call::New { id, .. }
                | Call::NewModel { id, .. }
                | Call::Config { id }
                | Call::Relay { id } => {
                    self.send(failure(&id, why));
                }
                Call::Load { id: Some(id), .. } => self.send(failure(&id, why)),
                Call::Load { id: None, .. } | Call::Prompt { .. } | Call::Initialize => {}
            }
        }
        self.asks.retain(|_, (asker, _)| *asker != process);
        let Some(session) = entry.session else {
            return;
        };
        let Some(known) = self.sessions.get_mut(&session) else {
            return;
        };
        known.process = None;
        known.loading = false;
        known.stream = None;
        let was_running = std::mem::take(&mut known.outstanding) > 0;
        if was_running {
            self.emit(&session, json!({"sessionUpdate": "notice", "severity": "error", "title": "The turn was interrupted", "description": why}));
            self.emit(&session, json!({"sessionUpdate": "state_update", "state": "idle", "stopReason": "cancelled", "background": []}));
        }
        self.fail_waiting(&session, why);
    }

    fn greet_failed(&mut self, why: &str) {
        for id in std::mem::take(&mut self.greeting) {
            self.send(failure(&id, why));
        }
    }

    /// Stops the processes of sessions that have done nothing for a while. The session stays
    /// known; its next use starts a process again.
    fn stop_idle(&mut self) {
        let idle = self.config.idle;
        let quiet: Vec<u64> = self
            .processes
            .iter()
            .filter(|(id, process)| {
                let busy = match &process.session {
                    Some(session) => self.sessions.get(session).is_none_or(|session| {
                        session.outstanding > 0 || session.loading || !session.waiting.is_empty()
                    }),
                    None => false,
                };
                !busy
                    && process.ready
                    && process.calls.is_empty()
                    && process.used.elapsed() >= idle
                    && !self.asks.values().any(|(asker, _)| asker == *id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in quiet {
            if let Some(process) = self.processes.remove(&id)
                && let Some(session) = process
                    .session
                    .as_ref()
                    .and_then(|s| self.sessions.get_mut(s))
            {
                session.process = None;
            }
            if self.spare == Some(id) {
                self.spare = None;
            }
        }
    }

    /// `pi-acp`'s `initialize` result in the v2 shape.
    fn welcome_v2(&self) -> Value {
        let v1 = self.welcome.as_ref().unwrap_or(&Value::Null);
        let mut prompt = Map::new();
        if v1.pointer("/agentCapabilities/promptCapabilities/image") == Some(&json!(true)) {
            prompt.insert("image".to_owned(), json!({}));
        }
        json!({
            "protocolVersion": 2,
            "info": v1.get("agentInfo").cloned().unwrap_or_else(|| json!({"name": "pi-acp", "version": "0"})),
            "capabilities": {"session": {"prompt": prompt}},
            "authMethods": [],
        })
    }
}

fn notice(severity: &str, title: &str, response: &Value) -> Value {
    json!({
        "sessionUpdate": "notice",
        "severity": severity,
        "title": title,
        "description": text(response, "/error/message"),
    })
}

/// v1 config options name their key `id`; v2 names it `configId`.
fn config_options_v2(options: Option<&Value>) -> Value {
    let Some(options) = options.and_then(Value::as_array) else {
        return json!([]);
    };
    options
        .iter()
        .map(|option| {
            let mut option = option.clone();
            if let Some(object) = option.as_object_mut()
                && let Some(id) = object.remove("id")
            {
                object.insert("configId".to_owned(), id);
            }
            option
        })
        .collect()
}

/// A v1 permission request in the v2 shape: the tool call moves under `subject`.
fn permission_v2(mut params: Value) -> Value {
    if let Some(object) = params.as_object_mut()
        && let Some(tool_call) = object.remove("toolCall")
    {
        object.insert(
            "title".to_owned(),
            tool_call.get("title").cloned().unwrap_or(Value::Null),
        );
        object.insert(
            "subject".to_owned(),
            json!({"type": "tool_call", "toolCall": tool_call}),
        );
    }
    params
}

/// Turns one v1 update into the v2 updates that say the same.
fn translate(session: &mut Session, update: &Value, ids: &Ids) -> Vec<Value> {
    let kind = text(update, "/sessionUpdate").unwrap_or_default();
    match kind {
        "agent_message_chunk" | "agent_thought_chunk" | "user_message_chunk" => {
            let piece = text(update, "/content/text").unwrap_or_default();
            // Outside a turn Pi only prints its startup banner. Inside one, "Queued message"
            // is its own announcement, not something the model said.
            let outside_a_turn = session.outstanding == 0 && !session.loading;
            if kind != "user_message_chunk" && (outside_a_turn || is_queue_notice(piece)) {
                // What follows a notice is a new message, not more of the one before it.
                session.stream = None;
                return Vec::new();
            }
            let message_id = match &session.stream {
                Some((current, id)) if current == kind => id.clone(),
                _ => {
                    let id = ids();
                    session.stream = Some((kind.to_owned(), id.clone()));
                    id
                }
            };
            let mut chunk = update.clone();
            chunk["messageId"] = message_id.into();
            vec![chunk]
        }
        "tool_call" | "tool_call_update" => {
            session.stream = None;
            tool_call_v2(session, update)
        }
        "session_info_update" => {
            // `_meta.piAcp` carries the queue depth and a running flag. The turn state is
            // reported from the prompts themselves, so only a title is worth passing on.
            let mut info = update.clone();
            if let Some(object) = info.as_object_mut() {
                object.remove("_meta");
            }
            if info.get("title").is_some() {
                vec![info]
            } else {
                Vec::new()
            }
        }
        "config_option_update" => {
            let mut changed = update.clone();
            changed["configOptions"] = config_options_v2(update.get("configOptions"));
            vec![changed]
        }
        // v2 has no modes; Pi's "thinking level" stays reachable as a config option.
        "current_mode_update" => Vec::new(),
        _ => vec![update.clone()],
    }
}

/// Pi announces its prompt queue as assistant text: "Queued message (position 1)." and
/// "Starting queued message. (0 remaining)".
fn is_queue_notice(text: &str) -> bool {
    text.starts_with("Queued message (position") || text.starts_with("Starting queued message")
}

fn tool_call_v2(session: &Session, update: &Value) -> Vec<Value> {
    let mut updates = Vec::new();
    let mut call = update.clone();
    call["sessionUpdate"] = "tool_call_update".into();
    let title = text(update, "/title").map(str::to_owned);
    let Some(object) = call.as_object_mut() else {
        return updates;
    };
    // Command output travels under `_meta`. In v2 it is a terminal of its own.
    if let Some(Value::Object(mut meta)) = object.remove("_meta") {
        if let Some(info) = meta.remove("terminal_info") {
            updates.push(json!({
                "sessionUpdate": "terminal_update",
                "terminalId": info["terminal_id"],
                "command": title,
                "cwd": info["cwd"],
            }));
        }
        if let Some(output) = meta.remove("terminal_output") {
            updates.push(json!({
                "sessionUpdate": "terminal_output_chunk",
                "terminalId": output["terminal_id"],
                "data": BASE64.encode(output["data"].as_str().unwrap_or_default()),
            }));
        }
        if let Some(exit) = meta.remove("terminal_exit") {
            updates.push(json!({
                "sessionUpdate": "terminal_update",
                "terminalId": exit["terminal_id"],
                "exitStatus": {"exitCode": exit["exit_code"], "signal": exit["signal"]},
            }));
        }
        if !meta.is_empty() {
            object.insert("_meta".to_owned(), Value::Object(meta));
        }
    }
    // Pi puts the tool's name in the title, except for commands, where the title is the command.
    if text(update, "/sessionUpdate") == Some("tool_call") {
        let name = match text(update, "/kind") {
            Some("execute") => Some("bash".to_owned()),
            _ => title,
        };
        if let Some(name) = name {
            object.insert("name".to_owned(), name.into());
        }
    }
    if let Some(Value::Array(content)) = object.get_mut("content") {
        for block in content.iter_mut() {
            if block["type"] == "diff" && block.get("changes").is_none() {
                *block = diff_v2(block, &session.cwd);
            }
        }
    }
    // The terminal exists before the tool call that points at it.
    updates.insert(
        updates
            .iter()
            .position(|update| {
                update["sessionUpdate"] != "terminal_update" || update.get("command").is_none()
            })
            .unwrap_or(updates.len()),
        call,
    );
    updates
}

/// A v1 diff (old and new text) as a v2 diff (which files changed, and a patch).
fn diff_v2(block: &Value, cwd: &str) -> Value {
    let path = text(block, "/path").unwrap_or_default();
    let path = if path.starts_with('/') || cwd.is_empty() {
        path.to_owned()
    } else {
        format!("{}/{path}", cwd.trim_end_matches('/'))
    };
    let old = block.get("oldText").and_then(Value::as_str);
    let new = text(block, "/newText").unwrap_or_default();
    let patch = similar::TextDiff::from_lines(old.unwrap_or_default(), new)
        .unified_diff()
        .header(&path, &path)
        .to_string();
    json!({
        "type": "diff",
        "changes": [{
            "operation": if old.is_some() { "modify" } else { "add" },
            "path": path,
            "fileType": "text",
        }],
        "patch": {"format": "git_patch", "text": format!("diff --git {path} {path}\n{patch}")},
    })
}

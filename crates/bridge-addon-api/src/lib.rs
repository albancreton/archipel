//! Addons: small pieces of code that sit between the daemon and an adapter.
//!
//! The daemon's core passes ACP messages along and keeps the log. Whatever goes beyond that
//! lives in an addon, so that it can be switched off, replaced or deleted without touching
//! the core:
//!
//! - a **fix** works around something a harness, an adapter or the protocol gets wrong or
//!   leaves out. It is expected to be removed once upstream has caught up.
//! - a **feature** adds something no upstream offers. It may stay for good.
//!
//! An addon is a Rust type that implements [`Addon`]. It is compiled into the daemon: the
//! ones the library ships are in the `bridge-addons` crate, and anybody who builds their own
//! daemon binary can add their own from any crate (`examples/own-daemon`). This crate holds
//! only what an addon needs, so an addon crate does not depend on the daemon.
//!
//! # What an addon sees
//!
//! Every message between an app and an adapter, in both directions:
//!
//! ```text
//!            towards the adapter ──────────────────────────────▶
//! app ── request ──────▶ [request]            ──▶ adapter
//! app ◀─ its response ── [response]           ◀── adapter
//! app ── notification ─▶ [notification]       ──▶ adapter      such as session/cancel
//! app ◀─ update ──────── [update]             ◀── adapter      session/update
//! app                    [agent_notification] ◀── adapter      any other notification
//! app ◀─ request ─────── [agent_request]      ◀── adapter      an approval, a question
//! app ── its answer ───▶ [answer]             ──▶ adapter
//!            ◀────────────────────────────── towards the app
//! ```
//!
//! The daemon's own requests to an adapter (`initialize`, the `session/resume` that wakes a
//! session, `session/list`) pass the same hooks as an app's. Every message says who it is
//! from ([`Origin`]).
//!
//! Each of these can be let through, changed, stopped, or held and released later.
//!
//! # The app side
//!
//! The hooks above sit between the daemon and an adapter. A second set sits between an app
//! and the daemon, and sees what the daemon handles or says itself: attaching to a session
//! and its replay, the session list, the daemon's answer to an app's `initialize`, and every
//! message on its way to one app.
//!
//! ```text
//! app ── request ──────▶ [app_request]      ──▶ daemon ──▶ [request] ──▶ adapter
//! app ◀─ its response ── [app_response]     ◀── daemon
//! app ── notification ─▶ [app_notification] ──▶ daemon
//! app ── an answer ────▶ [app_answer]       ──▶ daemon
//! app ◀─ anything else ─ [to_app]           ◀── daemon     updates, replay, requests
//! ```
//!
//! So an app's `session/prompt` passes `app_request` of every addon, then the daemon, then
//! `request` of every addon. These hooks are called for every app and every agent:
//! [`Addon::applies`] is not asked. On its way to an app everything passes `to_app`, also
//! what an addon added itself.
//!
//! # What an addon sends
//!
//! An addon can stand in for either side. Like the app, it sends the adapter requests
//! ([`Cx::call`]) and notifications ([`Cx::notify`]). Like the adapter, it sends apps
//! session updates ([`Cx::append`]), other notifications ([`Cx::announce`]) and requests for
//! the user to answer ([`Cx::ask`]). The answers to its requests arrive in [`Addon::reply`].
//!
//! # Several addons
//!
//! Addons are layers, in the order they were added: the first is nearest the app, the last
//! nearest the adapter. A message towards the adapter passes them first to last, a message
//! towards the app last to first. An addon that answers a request itself is the adapter as
//! far as the layers nearer the app can tell: they see the response, the layers nearer the
//! adapter see nothing. What an addon sends itself starts at its own layer and passes only
//! the layers beyond it, in the direction it travels. An addon is never shown its own
//! messages.
//!
//! # Rules
//!
//! The daemon handles one message at a time on one task and calls the hooks from there. A
//! hook must return quickly and must not block. An addon that has to wait for something
//! holds the message and releases it from a later hook; to wait for time to pass it asks to
//! be woken ([`Cx::wake`]).
//!
//! What an addon asks for through [`Cx`] happens when its hook returns, before the message
//! it was looking at moves on.
//!
//! See `docs/design/addons.md`.

use serde_json::{Value, json};
use std::time::Duration;

/// Whether an addon repairs something or adds something. See the crate documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fix,
    Feature,
}

/// What an addon says about itself. This is what `bridged addons` prints, and what somebody
/// reads to decide whether the addon can go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// `<scope>.<name>` in lower case, such as `claude.something` for one harness or
    /// `acp.something` for any. Used to switch the addon on and off.
    pub id: &'static str,
    pub kind: Kind,
    /// One sentence: what it does.
    pub summary: &'static str,
    /// The fault or gap it stands in for, with the versions where it was seen.
    pub because: &'static str,
    /// What has to happen upstream for it to be removed. Empty for one meant to stay.
    pub until: &'static str,
    /// The conformance scenario that shows whether it is still needed (`conformance/`).
    pub scenario: Option<&'static str>,
}

/// The agent a message belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Agent<'a> {
    /// The id apps use, such as `claude`. A daemon may have several agents of one harness.
    pub id: &'a str,
    /// Which harness it is, such as `claude` or `pi`. `None` for a plain ACP adapter.
    pub harness: Option<&'a str>,
    /// The adapter's name and version, as it reported them in `initialize`. `None` until it
    /// has answered, so also while `initialize` itself passes the hooks.
    pub adapter: Option<&'a str>,
    pub version: Option<&'a str>,
    /// The capabilities the adapter advertised in `initialize`.
    pub capabilities: Option<&'a Value>,
    /// The operating system the daemon runs on: `macos`, `linux`, `windows`.
    pub os: &'static str,
}

/// Who a message is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin<'a> {
    /// An app, with the name it gave in its `initialize`.
    App {
        name: Option<&'a str>,
    },
    /// The daemon itself: `initialize`, waking a session, listing a harness's sessions.
    Daemon,
    Adapter,
    /// Another addon, by its id.
    Addon(&'a str),
}

/// How a request ended: the two things a JSON-RPC response can carry.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Result(Value),
    Error {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

impl Outcome {
    /// Reads a JSON-RPC response (or any object with `result` or `error`).
    #[must_use]
    pub fn of(response: &Value) -> Self {
        match response.get("error") {
            Some(error) => Self::Error {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(-32603),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                data: error.get("data").cloned(),
            },
            None => Self::Result(response.get("result").cloned().unwrap_or(Value::Null)),
        }
    }

    /// `{"result": …}` or `{"error": …}`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Result(result) => json!({ "result": result }),
            Self::Error {
                code,
                message,
                data,
            } => {
                let mut error = json!({"code": code, "message": message});
                if let Some(data) = data {
                    error["data"] = data.clone();
                }
                json!({ "error": error })
            }
        }
    }

    /// The result, unless it is an error.
    #[must_use]
    pub fn result(&self) -> Option<&Value> {
        match self {
            Self::Result(result) => Some(result),
            Self::Error { .. } => None,
        }
    }
}

/// A request on its way to an adapter.
pub struct Request<'a> {
    /// A number the daemon gives every message, to tell them apart: a request's comes back
    /// in [`Response::id`], and it is what [`Cx::release`] takes. Not the JSON-RPC id.
    pub id: u64,
    pub agent: Agent<'a>,
    pub from: Origin<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// What to do with a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Let it go on, with whatever the addon changed.
    Pass,
    /// Do not send it. Whoever asked gets this JSON-RPC error, marked with the addon's id.
    Refuse { code: i64, message: String },
    /// Do not send it. Whoever asked gets this result, as if the other side had answered.
    Answer(Value),
    /// Keep it here until the addon releases it by its id, from any later hook
    /// ([`Cx::release`]). Nothing else waits: other messages go on meanwhile. A held request
    /// that is never released is never answered, unless its adapter stops.
    Hold,
}

/// What to do with a response or an answer. Its content is changed in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Let it go on, with whatever the addon changed.
    Pass,
    /// Keep it here until the addon releases it. See [`Verdict::Hold`].
    Hold,
}

/// An adapter's answer to a request, on its way back to whoever asked.
pub struct Response<'a> {
    /// The [`Request::id`] of the request this answers.
    pub id: u64,
    pub agent: Agent<'a>,
    /// Who made the request.
    pub from: Origin<'a>,
    pub method: &'a str,
    /// The request's params, as they were sent.
    pub params: &'a Value,
    /// May be changed, also from an error to a result or back.
    pub outcome: &'a mut Outcome,
}

/// A notification on its way to the adapter, such as an app's `session/cancel`.
///
/// One whose method starts with `_x/` is for the addons alone: it is shown to them and
/// never reaches an adapter.
pub struct Notification<'a> {
    pub id: u64,
    pub agent: Agent<'a>,
    pub from: Origin<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// A `session/update` on its way from the adapter, before the daemon logs it and passes it
/// to apps.
pub struct Update<'a> {
    pub id: u64,
    pub agent: Agent<'a>,
    pub from: Origin<'a>,
    /// The session the update is for, which may be a subagent's.
    pub session_id: &'a str,
    /// The update body. May be changed.
    pub update: &'a mut Value,
}

/// Any other notification from the adapter, such as `auth/status_update`.
///
/// The daemon passes none of these to apps: it only keeps the login state from
/// `auth/status_update`. An addon that wants apps to have one drops it and sends its own
/// ([`Cx::announce`]), which does go to apps.
pub struct AgentNotification<'a> {
    pub id: u64,
    pub agent: Agent<'a>,
    pub from: Origin<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// What to do with a notification or an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Let it go on, with whatever the addon changed.
    Yes,
    /// Drop it. A dropped update is neither logged nor passed on.
    No,
    /// Keep it here until the addon releases or discards it. What comes after it goes on
    /// meanwhile, so a held update ends up later in the log than it arrived.
    Hold,
}

/// A request made of the user, such as `session/request_permission` or `elicitation/create`,
/// before the daemon logs it and shows it to apps.
pub struct AgentRequest<'a> {
    /// Comes back in [`Answer::id`].
    pub id: u64,
    pub agent: Agent<'a>,
    /// The adapter, or an addon nearer the adapter ([`Cx::ask`]).
    pub from: Origin<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// The answer to a request made of the user, on its way back to whoever asked.
pub struct Answer<'a> {
    /// The [`AgentRequest::id`] of the request this answers.
    pub id: u64,
    pub agent: Agent<'a>,
    /// Who answered: an app, or an addon nearer the app.
    pub from: Origin<'a>,
    /// The method and params of the request this answers.
    pub method: &'a str,
    pub params: &'a Value,
    /// May be changed.
    pub outcome: &'a mut Outcome,
}

/// An app's call of a method under `_x/`, which no adapter knows.
pub struct MethodCall<'a> {
    pub id: u64,
    pub from: Origin<'a>,
    pub method: &'a str,
    pub params: &'a Value,
}

/// A connected app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct App<'a> {
    /// The number of its connection. Stays the same until it disconnects.
    pub id: u64,
    /// The name it gave in its `initialize`.
    pub name: Option<&'a str>,
    /// What it may do: `observe`, `interact`, `extend` or `admin`.
    pub scope: &'static str,
}

/// A request from an app, before the daemon handles it. That is every request an app
/// makes: the daemon's own methods (`initialize`, `_bridge/…`) and the ones it will pass to
/// an adapter.
pub struct AppRequest<'a> {
    /// Comes back in [`AppResponse::id`].
    pub id: u64,
    pub app: App<'a>,
    /// The agent of the session the params name, if they name one the daemon knows.
    pub agent: Option<Agent<'a>>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// The answer to an app's request, on its way to that app.
pub struct AppResponse<'a> {
    /// The [`AppRequest::id`] of the request this answers.
    pub id: u64,
    pub app: App<'a>,
    pub agent: Option<Agent<'a>>,
    pub method: &'a str,
    /// The request's params, as the daemon got them.
    pub params: &'a Value,
    /// May be changed.
    pub outcome: &'a mut Outcome,
}

/// A notification from an app, before the daemon handles it.
pub struct AppNotification<'a> {
    pub id: u64,
    pub app: App<'a>,
    pub agent: Option<Agent<'a>>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// An app's answer to a request it was sent, before the daemon handles it.
pub struct AppAnswer<'a> {
    pub id: u64,
    pub app: App<'a>,
    /// The JSON-RPC id of the request it answers, as [`ToApp::request`] showed it.
    pub request: &'a Value,
    /// May be changed.
    pub outcome: &'a mut Outcome,
}

/// A notification or a request on its way to one app: a session update (live or replayed),
/// a pending approval, the daemon's own notifications, an addon's.
pub struct ToApp<'a> {
    pub id: u64,
    pub app: App<'a>,
    pub agent: Option<Agent<'a>>,
    /// For a request, its JSON-RPC id. `None` for a notification.
    pub request: Option<&'a Value>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// A session, as the daemon's log knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// The session itself, or for a subagent's session the one it belongs to.
    pub root: String,
    /// The id of the agent that runs it.
    pub agent: String,
    pub cwd: String,
    pub title: Option<String>,
    /// The turn state last reported, such as `idle`.
    pub state: Option<String>,
}

/// What the daemon's log holds.
pub trait Sessions {
    /// The most recent update of this kind (its `sessionUpdate`) in the session itself, not
    /// in its subagents.
    fn last_update(&self, session_id: &str, kind: &str) -> Option<Value>;

    /// Every update of the session itself, oldest first.
    fn updates(&self, _session_id: &str) -> Vec<Value> {
        Vec::new()
    }

    fn session(&self, _session_id: &str) -> Option<SessionInfo> {
        None
    }
}

/// Something an addon asked for through [`Cx`]. The daemon carries these out; an addon's
/// tests can read them with [`Cx::effects`].
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Call {
        id: u64,
        method: String,
        params: Value,
    },
    Notify {
        method: String,
        params: Value,
    },
    Append {
        session_id: String,
        update: Value,
    },
    Announce {
        method: String,
        params: Value,
    },
    Ask {
        id: u64,
        method: String,
        params: Value,
    },
    AppCall {
        id: u64,
        method: String,
        params: Value,
    },
    Tell {
        app: u64,
        method: String,
        params: Value,
    },
    Wake {
        id: u64,
        after: Duration,
    },
    Release {
        id: u64,
        verdict: Verdict,
    },
    ReleaseWith {
        id: u64,
        content: Value,
    },
    Discard {
        id: u64,
    },
}

/// What a hook can read and do besides the message it was given.
///
/// What it sends goes to the adapter of the message at hand, or to the apps.
pub struct Cx<'a> {
    sessions: &'a dyn Sessions,
    next_call: u64,
    effects: Vec<Effect>,
}

impl<'a> Cx<'a> {
    /// For the daemon, and for tests of an addon. `next_call` is the first id handed out.
    #[must_use]
    pub fn new(sessions: &'a dyn Sessions, next_call: u64) -> Self {
        Self {
            sessions,
            next_call,
            effects: Vec::new(),
        }
    }

    fn next(&mut self) -> u64 {
        let id = self.next_call;
        self.next_call += 1;
        id
    }

    /// The most recent update of a kind in a session's log. See [`Sessions::last_update`].
    #[must_use]
    pub fn last_update(&self, session_id: &str, kind: &str) -> Option<Value> {
        self.sessions.last_update(session_id, kind)
    }

    /// Every update in a session's log, oldest first. See [`Sessions::updates`].
    #[must_use]
    pub fn updates(&self, session_id: &str) -> Vec<Value> {
        self.sessions.updates(session_id)
    }

    /// What the log knows about a session.
    #[must_use]
    pub fn session(&self, session_id: &str) -> Option<SessionInfo> {
        self.sessions.session(session_id)
    }

    /// Sends a request of the addon's own to the adapter. The answer arrives in
    /// [`Addon::reply`] with the id returned here. If the adapter stops first, the reply is
    /// an error.
    pub fn call(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next();
        self.effects.push(Effect::Call {
            id,
            method: method.to_owned(),
            params,
        });
        id
    }

    /// Sends a notification of the addon's own to the adapter, such as `session/cancel`.
    pub fn notify(&mut self, method: &str, params: Value) {
        self.effects.push(Effect::Notify {
            method: method.to_owned(),
            params,
        });
    }

    /// Adds an update to a session: it is logged, replayed and sent to every attached app
    /// like one from the adapter, marked as this addon's. For an unknown session it is
    /// dropped.
    pub fn append(&mut self, session_id: &str, update: Value) {
        self.effects.push(Effect::Append {
            session_id: session_id.to_owned(),
            update,
        });
    }

    /// Sends apps a notification that is not a session update. It is not logged. With a
    /// `sessionId` in its params it goes to the apps attached to that session, otherwise to
    /// every app.
    pub fn announce(&mut self, method: &str, params: Value) {
        self.effects.push(Effect::Announce {
            method: method.to_owned(),
            params,
        });
    }

    /// Asks the user something, the way an adapter asks for an approval: the request is
    /// logged in the session its params name (`sessionId`), marked as this addon's, shown to
    /// every attached app, and the first answer counts. The answer arrives in
    /// [`Addon::reply`] with the id returned here. If the session is unknown the reply is an
    /// error; if its adapter stops first, the reply is a cancellation.
    pub fn ask(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next();
        self.effects.push(Effect::Ask {
            id,
            method: method.to_owned(),
            params,
        });
        id
    }

    /// Makes a request of the daemon the way an app would: `session/new`, `session/prompt`,
    /// `_bridge/sessions/list` and so on. Unlike [`Cx::call`], which goes straight to the
    /// adapter, this has the daemon do its part: record a new session, wake one that is not
    /// live. The answer arrives in [`Addon::reply`] with the id returned here. The request
    /// does not pass the app-side hooks; what the daemon sends an adapter for it passes
    /// every addon's [`Addon::request`], marked as from this addon.
    pub fn app_call(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next();
        self.effects.push(Effect::AppCall {
            id,
            method: method.to_owned(),
            params,
        });
        id
    }

    /// Sends one app a notification. Not logged. Dropped if the app is gone.
    pub fn tell(&mut self, app: u64, method: &str, params: Value) {
        self.effects.push(Effect::Tell {
            app,
            method: method.to_owned(),
            params,
        });
    }

    /// Asks to be called back: [`Addon::timer`] runs with the id returned here once `after`
    /// has passed. This is how an addon puts a time limit on something it holds.
    pub fn wake(&mut self, after: Duration) -> u64 {
        let id = self.next();
        self.effects.push(Effect::Wake { id, after });
        id
    }

    /// Ends the hold on a message this addon held, by the message's id.
    ///
    /// - `Pass` lets it go on as it is.
    /// - `Answer` and `Refuse` give a held request its outcome here, and replace the outcome
    ///   of a held response or answer, which then goes on.
    ///
    /// For a held notification, update or message to an app, anything but `Pass` discards
    /// it.
    pub fn release(&mut self, id: u64, verdict: Verdict) {
        self.effects.push(Effect::Release { id, verdict });
    }

    /// Ends the hold on a message and lets it go on with other content: the params of a
    /// request or notification, the body of an update, the result of a response or answer.
    pub fn release_with(&mut self, id: u64, content: Value) {
        self.effects.push(Effect::ReleaseWith { id, content });
    }

    /// Ends the hold on a notification or an update by dropping it. A held request, response
    /// or answer cannot be dropped, since somebody waits for it: it ends as an error.
    pub fn discard(&mut self, id: u64) {
        self.effects.push(Effect::Discard { id });
    }

    /// What was asked for so far.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// For the daemon: what was asked for, and the next free id.
    #[must_use]
    pub fn finish(self) -> (Vec<Effect>, u64) {
        (self.effects, self.next_call)
    }
}

/// See the crate documentation. Every hook has a default that does nothing, so an addon
/// implements only the ones it needs.
pub trait Addon: Send + 'static {
    fn info(&self) -> Info;

    /// Whether the addon wants the messages of this agent. This is where an addon limits
    /// itself to a harness, an adapter version or an operating system. Asked for every
    /// message to or from an adapter, so it has to be cheap. Not asked for the app-side
    /// hooks, [`Addon::reply`], [`Addon::method`] and [`Addon::timer`].
    fn applies(&self, _agent: &Agent<'_>) -> bool {
        true
    }

    /// A request on its way to the adapter: an app's, the daemon's own, or another addon's.
    fn request(&mut self, _cx: &mut Cx<'_>, _request: &mut Request<'_>) -> Verdict {
        Verdict::Pass
    }

    /// The answer to a request, on its way back. Also the answer an addon nearer the adapter
    /// gave in the adapter's place.
    fn response(&mut self, _cx: &mut Cx<'_>, _response: &mut Response<'_>) -> Flow {
        Flow::Pass
    }

    /// A notification on its way to the adapter.
    fn notification(&mut self, _cx: &mut Cx<'_>, _notification: &mut Notification<'_>) -> Keep {
        Keep::Yes
    }

    /// An update on its way from the adapter.
    fn update(&mut self, _cx: &mut Cx<'_>, _update: &mut Update<'_>) -> Keep {
        Keep::Yes
    }

    /// Any other notification on its way from the adapter.
    fn agent_notification(
        &mut self,
        _cx: &mut Cx<'_>,
        _notification: &mut AgentNotification<'_>,
    ) -> Keep {
        Keep::Yes
    }

    /// A request made of the user. Answering it here keeps it from the user: the log still
    /// shows the request and who answered. While it is held, it is neither logged nor shown.
    fn agent_request(&mut self, _cx: &mut Cx<'_>, _request: &mut AgentRequest<'_>) -> Verdict {
        Verdict::Pass
    }

    /// The answer to a request made of the user, on its way back. Also the answer an addon
    /// nearer the app gave in the user's place.
    fn answer(&mut self, _cx: &mut Cx<'_>, _answer: &mut Answer<'_>) -> Flow {
        Flow::Pass
    }

    /// A request from an app, before the daemon handles it.
    fn app_request(&mut self, _cx: &mut Cx<'_>, _request: &mut AppRequest<'_>) -> Verdict {
        Verdict::Pass
    }

    /// The answer to an app's request, on its way to that app: the daemon's own, an
    /// adapter's as the other hooks left it, or the one an addon nearer the daemon gave.
    fn app_response(&mut self, _cx: &mut Cx<'_>, _response: &mut AppResponse<'_>) -> Flow {
        Flow::Pass
    }

    /// A notification from an app, before the daemon handles it.
    fn app_notification(
        &mut self,
        _cx: &mut Cx<'_>,
        _notification: &mut AppNotification<'_>,
    ) -> Keep {
        Keep::Yes
    }

    /// An app's answer to a request it was sent, before the daemon handles it. Dropping it
    /// leaves the request open for another answer.
    fn app_answer(&mut self, _cx: &mut Cx<'_>, _answer: &mut AppAnswer<'_>) -> Keep {
        Keep::Yes
    }

    /// A notification or a request on its way to one app. Dropping it keeps it from that
    /// app only.
    fn to_app(&mut self, _cx: &mut Cx<'_>, _message: &mut ToApp<'_>) -> Keep {
        Keep::Yes
    }

    /// The answer to a request this addon made with [`Cx::call`], [`Cx::ask`] or
    /// [`Cx::app_call`].
    fn reply(&mut self, _cx: &mut Cx<'_>, _id: u64, _outcome: &Outcome) {}

    /// An app calls a method under `_x/`. Addons are asked in order; `Pass` means "not
    /// mine". If none takes it, it goes to the extension that registered the namespace.
    fn method(&mut self, _cx: &mut Cx<'_>, _call: &MethodCall<'_>) -> Verdict {
        Verdict::Pass
    }

    /// The time asked for with [`Cx::wake`] has passed.
    fn timer(&mut self, _cx: &mut Cx<'_>, _id: u64) {}

    /// An adapter stopped: its process ended, or the daemon gave up on it. By now every
    /// request that waited on it has failed, including this addon's own. The next request
    /// for the agent starts a new adapter, beginning with `initialize`.
    fn stopped(&mut self, _cx: &mut Cx<'_>, _agent: &Agent<'_>, _why: &str) {}
}

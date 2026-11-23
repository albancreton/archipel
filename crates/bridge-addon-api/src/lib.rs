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
//!            towards the adapter ──────────────────────────▶
//! app ── request ──────▶ [request]      ──▶ adapter
//! app ◀─ its response ── [response]     ◀── adapter
//! app ── notification ─▶ [notification] ──▶ adapter        such as session/cancel
//! app ◀─ update ──────── [update]       ◀── adapter        session/update
//! app ◀─ request ─────── [agent_request]◀── adapter        an approval, a question
//! app ── its answer ───▶ [answer]       ──▶ adapter
//!            ◀────────────────────────── towards the app
//! ```
//!
//! The daemon's own requests to an adapter (`initialize`, the `session/resume` that wakes a
//! session, `session/list`) pass the same hooks as an app's.
//!
//! # Several addons
//!
//! Addons are layers, in the order they were added: the first is nearest the app, the last
//! nearest the adapter. A message towards the adapter passes them first to last, a message
//! towards the app last to first. An addon that answers a request itself is the adapter as
//! far as the layers nearer the app can tell: they see the response, the layers nearer the
//! adapter see nothing. What an addon sends itself ([`Cx::call`], [`Cx::append`]) starts at
//! its own layer and passes only the layers beyond it.
//!
//! # Rules
//!
//! The daemon handles one message at a time on one task and calls the hooks from there. A
//! hook must return quickly and must not block. An addon that has to wait for something
//! holds the request ([`Verdict::Hold`]) and releases it from a later hook.
//!
//! What an addon asks for through [`Cx`] happens when its hook returns, before the message
//! it was looking at moves on.
//!
//! See `docs/design/addons.md`.

use serde_json::{Value, json};

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
    /// A number the daemon gives every request, to tell them apart: the same one comes back
    /// in [`Response::id`], and it is what [`Cx::release`] takes. Not the JSON-RPC id.
    pub id: u64,
    pub agent: Agent<'a>,
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
    /// Keep it here until the addon calls [`Cx::release`] with the request's id, from any
    /// later hook. Nothing else waits: other requests go on meanwhile. A held request that
    /// is never released is never answered, unless its adapter stops.
    ///
    /// Not available in [`Addon::agent_request`], where it means [`Verdict::Pass`].
    Hold,
}

/// An adapter's answer to a request, on its way back to whoever asked.
pub struct Response<'a> {
    /// The [`Request::id`] of the request this answers.
    pub id: u64,
    pub agent: Agent<'a>,
    pub method: &'a str,
    /// The request's params, as they were sent.
    pub params: &'a Value,
    /// May be changed, also from an error to a result or back.
    pub outcome: &'a mut Outcome,
}

/// A notification an app sent for the adapter, such as `session/cancel`.
pub struct Notification<'a> {
    pub agent: Agent<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// A `session/update` an adapter sent, before the daemon logs it and passes it to apps.
pub struct Update<'a> {
    pub agent: Agent<'a>,
    /// The session the update is for, which may be a subagent's.
    pub session_id: &'a str,
    /// The update body. May be changed.
    pub update: &'a mut Value,
}

/// What to do with a notification or an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Let it go on, with whatever the addon changed.
    Yes,
    /// Drop it. A dropped update is neither logged nor passed on.
    No,
}

/// A request the adapter makes of the user, such as `session/request_permission` or
/// `elicitation/create`, before the daemon logs it and shows it to apps.
pub struct AgentRequest<'a> {
    pub agent: Agent<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
}

/// An app's answer to a request of the adapter, on its way to the adapter.
pub struct Answer<'a> {
    pub agent: Agent<'a>,
    /// The method and params of the request this answers.
    pub method: &'a str,
    pub params: &'a Value,
    /// May be changed.
    pub outcome: &'a mut Outcome,
}

/// An app's call of a method under `_x/`, which no adapter knows.
pub struct MethodCall<'a> {
    /// For [`Cx::release`], if the addon holds the call.
    pub id: u64,
    pub method: &'a str,
    pub params: &'a Value,
}

/// What the daemon's log holds.
pub trait Sessions {
    /// The most recent update of this kind (its `sessionUpdate`) in the session itself, not
    /// in its subagents.
    fn last_update(&self, session_id: &str, kind: &str) -> Option<Value>;
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
    Append {
        session_id: String,
        update: Value,
    },
    Release {
        request: u64,
        verdict: Verdict,
    },
    ReleaseWith {
        request: u64,
        params: Value,
    },
}

/// What a hook can read and do besides the message it was given.
pub struct Cx<'a> {
    sessions: &'a dyn Sessions,
    next_call: u64,
    effects: Vec<Effect>,
}

impl<'a> Cx<'a> {
    /// For the daemon, and for tests of an addon. `next_call` is the id the first
    /// [`Cx::call`] returns.
    #[must_use]
    pub fn new(sessions: &'a dyn Sessions, next_call: u64) -> Self {
        Self {
            sessions,
            next_call,
            effects: Vec::new(),
        }
    }

    /// The most recent update of a kind in a session's log. See [`Sessions::last_update`].
    #[must_use]
    pub fn last_update(&self, session_id: &str, kind: &str) -> Option<Value> {
        self.sessions.last_update(session_id, kind)
    }

    /// Sends a request of the addon's own to the adapter of the message at hand. The answer
    /// arrives in [`Addon::reply`] with the id returned here. If the adapter stops first, the
    /// reply is an error.
    pub fn call(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_call;
        self.next_call += 1;
        self.effects.push(Effect::Call {
            id,
            method: method.to_owned(),
            params,
        });
        id
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

    /// Ends the hold on a request this addon held ([`Verdict::Hold`]): `Pass` lets it go on
    /// as it was, `Refuse` and `Answer` end it here.
    pub fn release(&mut self, request: u64, verdict: Verdict) {
        self.effects.push(Effect::Release { request, verdict });
    }

    /// Ends the hold on a request and lets it go on with other params.
    pub fn release_with(&mut self, request: u64, params: Value) {
        self.effects.push(Effect::ReleaseWith { request, params });
    }

    /// What was asked for so far.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// For the daemon: what was asked for, and the next free call id.
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
    /// message, so it has to be cheap. Not asked for [`Addon::reply`] and [`Addon::method`].
    fn applies(&self, _agent: &Agent<'_>) -> bool {
        true
    }

    /// A request on its way to the adapter: an app's or the daemon's own.
    fn request(&mut self, _cx: &mut Cx<'_>, _request: &mut Request<'_>) -> Verdict {
        Verdict::Pass
    }

    /// The answer to a request, on its way back. Also the answer an addon nearer the adapter
    /// gave in the adapter's place.
    fn response(&mut self, _cx: &mut Cx<'_>, _response: &mut Response<'_>) {}

    /// A notification from an app on its way to the adapter.
    fn notification(&mut self, _cx: &mut Cx<'_>, _notification: &mut Notification<'_>) -> Keep {
        Keep::Yes
    }

    /// An update on its way from the adapter.
    fn update(&mut self, _cx: &mut Cx<'_>, _update: &mut Update<'_>) -> Keep {
        Keep::Yes
    }

    /// A request the adapter makes of the user. Answering it here keeps it from the user:
    /// the log still shows the request and who answered.
    fn agent_request(&mut self, _cx: &mut Cx<'_>, _request: &mut AgentRequest<'_>) -> Verdict {
        Verdict::Pass
    }

    /// An app's answer to a request of the adapter, on its way to the adapter. Also the
    /// answer an addon nearer the app gave in the user's place.
    fn answer(&mut self, _cx: &mut Cx<'_>, _answer: &mut Answer<'_>) {}

    /// The answer to a request this addon made with [`Cx::call`].
    fn reply(&mut self, _cx: &mut Cx<'_>, _call: u64, _outcome: &Outcome) {}

    /// An app calls a method under `_x/`. Addons are asked in order; `Pass` means "not
    /// mine". If none takes it, it goes to the extension that registered the namespace.
    fn method(&mut self, _cx: &mut Cx<'_>, _call: &MethodCall<'_>) -> Verdict {
        Verdict::Pass
    }
}

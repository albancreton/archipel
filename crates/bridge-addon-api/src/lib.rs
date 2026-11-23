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
//! The daemon handles one message at a time on one task and calls the hooks from there. A
//! hook must return quickly and must not block.
//!
//! See `docs/design/addons.md`.

use serde_json::Value;

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
    /// The adapter's name and version, as it reported them in `initialize`.
    pub adapter: Option<&'a str>,
    pub version: Option<&'a str>,
    /// The capabilities the adapter advertised in `initialize`.
    pub capabilities: Option<&'a Value>,
    /// The operating system the daemon runs on: `macos`, `linux`, `windows`.
    pub os: &'static str,
}

/// What the daemon's log holds for one session.
pub trait History {
    /// The most recent update of this kind (its `sessionUpdate`) in the session itself, not
    /// in its subagents.
    fn last_update(&self, kind: &str) -> Option<Value>;
}

/// A request the daemon is about to send to an adapter: an app's, or one of its own such as
/// the `session/resume` that wakes a session.
pub struct Request<'a> {
    pub agent: Agent<'a>,
    pub method: &'a str,
    /// May be changed.
    pub params: &'a mut Value,
    /// The log of the session the request is about. `None` when it is about no session the
    /// daemon knows, as with `session/new`.
    pub history: Option<&'a dyn History>,
}

/// What to do with a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Send it, with whatever the addon changed.
    Pass,
    /// Do not send it. Whoever asked gets this JSON-RPC error, marked with the addon's id.
    Refuse { code: i64, message: String },
}

/// A `session/update` an adapter sent, before the daemon logs it and passes it to apps.
pub struct Update<'a> {
    pub agent: Agent<'a>,
    /// The session the update is for, which may be a subagent's.
    pub session_id: &'a str,
    /// The update body. May be changed.
    pub update: &'a mut Value,
}

/// What to do with an [`Update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Log it and pass it on, with whatever the addon changed.
    Yes,
    /// Drop it: it is neither logged nor passed on.
    No,
}

/// See the crate documentation. Every hook has a default that does nothing, so an addon
/// implements only the ones it needs.
pub trait Addon: Send + 'static {
    fn info(&self) -> Info;

    /// Whether the addon wants the messages of this agent. This is where an addon limits
    /// itself to a harness, an adapter version or an operating system. Asked for every
    /// message, so it has to be cheap.
    fn applies(&self, _agent: &Agent<'_>) -> bool {
        true
    }

    /// A request on its way to the adapter.
    fn request(&mut self, _request: &mut Request<'_>) -> Verdict {
        Verdict::Pass
    }

    /// An update on its way from the adapter.
    fn update(&mut self, _update: &mut Update<'_>) -> Keep {
        Keep::Yes
    }
}

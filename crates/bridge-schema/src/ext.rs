//! The daemon's extensions to ACP v2. Draft: names and fields may change before the first release.
//!
//! ACP reserves method names starting with `_` and the `_meta` field for extensions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The key under `_meta` that holds [`BridgeMeta`].
pub const META_KEY: &str = "bridge";

pub const METHOD_AGENTS_LIST: &str = "_bridge/agents/list";
pub const METHOD_SESSIONS_LIST: &str = "_bridge/sessions/list";
pub const METHOD_SESSION_ATTACH: &str = "_bridge/session/attach";
pub const METHOD_SESSION_DETACH: &str = "_bridge/session/detach";
pub const NOTIFICATION_REQUEST_RESOLVED: &str = "_bridge/request/resolved";
pub const NOTIFICATION_SESSIONS_UPDATED: &str = "_bridge/sessions/updated";
pub const NOTIFICATION_SESSIONS_REMOVED: &str = "_bridge/sessions/removed";
pub const METHOD_EXTENSION_REGISTER: &str = "_bridge/extension/register";

/// Error code: the session is open in another process of the harness, so the daemon will not
/// act on it. The error's `data` is an [`OpenElsewhere`]. The way to continue is `session/fork`.
pub const ERROR_OPEN_ELSEWHERE: i64 = -32004;

/// The `data` of an [`ERROR_OPEN_ELSEWHERE`] error: who has the session open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenElsewhere {
    pub session_id: String,
    pub pid: u32,
    /// What that process says it is doing, such as `busy` or `idle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// How that process was started, such as `cli`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}
pub const METHOD_SESSION_APPEND: &str = "_bridge/session/append";

/// `_bridge/extension/register` request: this connection is the extension `namespace`.
///
/// From then on it may add updates whose kind starts with `x.<namespace>.` to sessions, and
/// the daemon passes it every request whose method starts with `_x/<namespace>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterExtensionRequest {
    /// Lower-case letters, digits and `-`.
    pub namespace: String,
}

/// `_bridge/session/append` request: add an update of the extension's own kind to a session.
/// It is numbered, stored, replayed and sent to every attached app like any other update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppendRequest {
    pub session_id: String,
    /// The update body. Its `sessionUpdate` must start with `x.<namespace>.`.
    pub update: Value,
    /// How the update changes what clients hold. Without it the update is kept as an event
    /// that clients do not place among the items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ItemRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppendResponse {
    /// The sequence number the update got.
    pub seq: u64,
}

/// How an extension's update changes client state: an ACP state rule whose target is the
/// update's own kind. The daemon stores it in the update under `_meta.bridge.state`, so every
/// client can apply it without knowing the extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemRule {
    /// `append`, `upsert`, `replace` or `remove`.
    pub action: String,
    /// Dotted path of the item's id in the update, such as `checkpointId`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The field of the item that is changed. Absent means the whole item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Dotted path of the new data in the update. Absent means the whole update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// What a connected app may do. Each level includes the ones before it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// List and read sessions.
    Observe,
    /// Also create sessions, prompt, cancel, answer, change settings.
    Interact,
    /// Also act as an extension: register a namespace, add items to sessions, serve methods.
    Extend,
    /// Also manage the daemon. Nothing needs this yet.
    Admin,
}

/// Added by the daemon under `_meta.bridge` on every `session/update` it sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMeta {
    /// Position of this update in the session's log. Starts at 1 and has no gaps.
    pub seq: u64,
    /// The session whose log this is, when the message belongs to a subagent's child session.
    /// Absent when it equals the message's own `sessionId`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_session_id: Option<String>,
}

/// Under `_meta.bridge` inside an update body that the daemon wrote itself, not the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrigin {
    /// Always `daemon`.
    pub origin: String,
}

/// Under `_meta.bridge` in the params of `session/new`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionMeta {
    /// The agent to create the session with. Absent for the daemon's first agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

/// `_bridge/agents/list` request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentsRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentInfo>,
}

/// One agent the daemon can start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Stable id used in other calls, such as `claude` or `pi`.
    pub agent_id: String,
    pub name: String,
    /// Version of the adapter, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub readiness: AgentReadiness,
    /// Human-readable explanation when the agent is not ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentReadiness {
    Ready,
    /// Installed, but the user has to log in with the harness itself.
    NeedsLogin,
    NotInstalled,
    Error,
}

/// `_bridge/sessions/list` request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsRequest {
    /// Only sessions of this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Only sessions in this working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Leave out sessions that only the harness knows. This never starts an adapter.
    #[serde(default)]
    pub logged_only: bool,
    /// From a previous response, to get the next page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// `_bridge/sessions/updated`: a session appeared or its summary changed. Sent to every app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionsUpdatedNotification {
    pub session: SessionSummary,
}

/// `_bridge/sessions/removed`: a session was deleted at its harness through the daemon, which
/// dropped its log too. Sent to every app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionsRemovedNotification {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub session_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub cwd: String,
    /// True when an adapter process currently holds the session.
    pub live: bool,
    /// True when the session is in the daemon's log. False for a session only the harness
    /// knows, for example one started in the harness's own UI: attaching to it, or sending it
    /// anything, makes the daemon load its history from the harness first.
    pub logged: bool,
    /// True when another process of the harness has the session open right now, for example
    /// the harness's own terminal UI. The daemon then refuses to act on the session; it can
    /// still be read, and `session/fork` makes a copy to continue with.
    #[serde(default)]
    pub open_elsewhere: bool,
    /// The last ACP `state_update` state, such as `idle` or `running`, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Sequence number of the newest update in the log.
    pub head_seq: u64,
    /// RFC 3339 time of the newest update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// `_bridge/session/attach` request: start receiving a session's updates.
///
/// The response comes first, then the catch-up, then live messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachSessionRequest {
    pub session_id: String,
    /// The last sequence number the client has applied. Absent to get the whole session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachSessionResponse {
    /// How the client is brought up to date.
    pub catch_up: CatchUp,
    /// Sequence number of the newest update at the time of the response.
    pub head_seq: u64,
    /// Another process of the harness has the session open. It can be read, but the daemon
    /// refuses to act on it; `session/fork` makes a copy to continue with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_elsewhere: Option<OpenElsewhere>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatchUp {
    /// The updates after `afterSeq` follow. The client keeps its state.
    Delta,
    /// A condensed replay of the whole session follows, ending with `replay_complete`.
    /// The client discards its state for this session first.
    Replay,
}

/// `_bridge/session/detach` request: stop receiving updates. The session keeps running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetachSessionRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetachSessionResponse {}

/// `_bridge/request/resolved`: a pending approval or question was answered, possibly by another client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestResolvedNotification {
    pub session_id: String,
    /// The id of the request. It is the JSON-RPC id the daemon sent the request with.
    pub request_id: String,
    /// The method of the request, such as `session/request_permission`.
    pub method: String,
    /// The params of the request. Present in a replay, where the request itself is not sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// The answer that was given, in the shape of the method's response.
    pub outcome: Value,
    /// A label for the client that answered, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
}

/// Root type used only to generate `schema/bridge.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BridgeExtensions {
    pub meta: BridgeMeta,
    pub update_origin: UpdateOrigin,
    pub new_session_meta: NewSessionMeta,
    pub list_agents_request: ListAgentsRequest,
    pub list_agents_response: ListAgentsResponse,
    pub list_sessions_request: ListSessionsRequest,
    pub list_sessions_response: ListSessionsResponse,
    pub attach_session_request: AttachSessionRequest,
    pub attach_session_response: AttachSessionResponse,
    pub detach_session_request: DetachSessionRequest,
    pub detach_session_response: DetachSessionResponse,
    pub request_resolved_notification: RequestResolvedNotification,
    pub sessions_updated_notification: SessionsUpdatedNotification,
    pub sessions_removed_notification: SessionsRemovedNotification,
    pub scope: Scope,
    pub register_extension_request: RegisterExtensionRequest,
    pub append_request: AppendRequest,
    pub append_response: AppendResponse,
    pub open_elsewhere: OpenElsewhere,
}

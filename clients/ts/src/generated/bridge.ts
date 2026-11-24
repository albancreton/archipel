// Generated from schema/bridge.schema.json by `just gen`. Do not edit.

/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "AgentReadiness".
 */
export type AgentReadiness = ("ready" | "not_installed" | "error") | "needs_login";
/**
 * What a connected app may do. Each level includes the ones before it.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "Scope".
 */
export type Scope = "observe" | "interact" | "extend" | "admin";
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "CatchUp".
 */
export type CatchUp = "delta" | "replay";

/**
 * Root type used only to generate `schema/bridge.schema.json`.
 */
export interface BridgeExtensionsToACPV2Draft {
  meta: BridgeMeta;
  updateOrigin: UpdateOrigin;
  newSessionMeta: NewSessionMeta;
  listAgentsRequest: ListAgentsRequest;
  listAgentsResponse: ListAgentsResponse;
  listSessionsRequest: ListSessionsRequest;
  listSessionsResponse: ListSessionsResponse;
  attachSessionRequest: AttachSessionRequest;
  attachSessionResponse: AttachSessionResponse;
  detachSessionRequest: DetachSessionRequest;
  detachSessionResponse: DetachSessionResponse;
  requestResolvedNotification: RequestResolvedNotification;
  sessionsUpdatedNotification: SessionsUpdatedNotification;
  sessionsRemovedNotification: SessionsRemovedNotification;
  scope: Scope;
  registerExtensionRequest: RegisterExtensionRequest;
  appendRequest: AppendRequest;
  appendResponse: AppendResponse;
  openElsewhere: OpenElsewhere;
}
/**
 * Added by the daemon under `_meta.bridge` on every `session/update` it sends.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "BridgeMeta".
 */
export interface BridgeMeta {
  /**
   * Position of this update in the session's log. Starts at 1 and has no gaps.
   */
  seq: number;
  /**
   * The session whose log this is, when the message belongs to a subagent's child session.
   * Absent when it equals the message's own `sessionId`.
   */
  rootSessionId?: string | null;
}
/**
 * Under `_meta.bridge` inside an update body that did not come from the agent.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "UpdateOrigin".
 */
export interface UpdateOrigin {
  /**
   * Who wrote it: `daemon`, `extension` or `addon`.
   */
  origin: string;
  /**
   * The id of the addon, when `origin` is `addon`.
   */
  addon?: string | null;
}
/**
 * Under `_meta.bridge` in the params of `session/new`.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "NewSessionMeta".
 */
export interface NewSessionMeta {
  /**
   * The agent to create the session with. Absent for the daemon's first agent.
   */
  agentId?: string | null;
}
/**
 * `_bridge/agents/list` request.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "ListAgentsRequest".
 */
export interface ListAgentsRequest {}
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "ListAgentsResponse".
 */
export interface ListAgentsResponse {
  agents: AgentInfo[];
}
/**
 * One agent the daemon can start.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "AgentInfo".
 */
export interface AgentInfo {
  /**
   * Stable id used in other calls, such as `claude` or `pi`.
   */
  agentId: string;
  name: string;
  /**
   * Version of the adapter, when known.
   */
  version?: string | null;
  readiness: AgentReadiness;
  /**
   * Human-readable explanation when the agent is not ready.
   */
  detail?: string | null;
  /**
   * What the agent said it can do in its `initialize` answer, once it has been started.
   * `session.title`, `session.delete` and `session.options` say whether a session can be
   * renamed, deleted, and its settings read before it exists. Absent until the agent has
   * started: `_bridge/sessions/list` with its id starts it.
   */
  capabilities?: {
    [k: string]: unknown;
  };
}
/**
 * `_bridge/sessions/list` request.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "ListSessionsRequest".
 */
export interface ListSessionsRequest {
  /**
   * Only sessions of this agent.
   */
  agentId?: string | null;
  /**
   * Only sessions in this working directory.
   */
  cwd?: string | null;
  /**
   * Leave out sessions that only the harness knows. This never starts an adapter.
   */
  loggedOnly?: boolean;
  /**
   * From a previous response, to get the next page.
   */
  cursor?: string | null;
}
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "ListSessionsResponse".
 */
export interface ListSessionsResponse {
  sessions: SessionSummary[];
  nextCursor?: string | null;
}
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "SessionSummary".
 */
export interface SessionSummary {
  sessionId: string;
  agentId: string;
  title?: string | null;
  cwd: string;
  /**
   * True when an adapter process currently holds the session.
   */
  live: boolean;
  /**
   * True when the session is in the daemon's log. False for a session only the harness
   * knows, for example one started in the harness's own UI: attaching to it, or sending it
   * anything, makes the daemon load its history from the harness first.
   */
  logged: boolean;
  /**
   * True when another process of the harness has the session open right now, for example
   * the harness's own terminal UI. The daemon then refuses to act on the session; it can
   * still be read, and `session/fork` makes a copy to continue with.
   */
  openElsewhere?: boolean;
  /**
   * The last ACP `state_update` state, such as `idle` or `running`, when known.
   */
  state?: string | null;
  /**
   * Sequence number of the newest update in the log.
   */
  headSeq: number;
  /**
   * RFC 3339 time of the newest update.
   */
  updatedAt?: string | null;
}
/**
 * `_bridge/session/attach` request: start receiving a session's updates.
 *
 * The response comes first, then the catch-up, then live messages.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "AttachSessionRequest".
 */
export interface AttachSessionRequest {
  sessionId: string;
  /**
   * The last sequence number the client has applied. Absent to get the whole session.
   */
  afterSeq?: number | null;
}
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "AttachSessionResponse".
 */
export interface AttachSessionResponse {
  /**
   * How the client is brought up to date.
   */
  catchUp: "delta" | "replay";
  /**
   * Sequence number of the newest update at the time of the response.
   */
  headSeq: number;
  /**
   * Another process of the harness has the session open. It can be read, but the daemon
   * refuses to act on it; `session/fork` makes a copy to continue with.
   */
  openElsewhere?: OpenElsewhere | null;
}
/**
 * The `data` of an [`ERROR_OPEN_ELSEWHERE`] error: who has the session open.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "OpenElsewhere".
 */
export interface OpenElsewhere {
  sessionId: string;
  pid: number;
  /**
   * What that process says it is doing, such as `busy` or `idle`.
   */
  status?: string | null;
  /**
   * How that process was started, such as `cli`.
   */
  client?: string | null;
}
/**
 * `_bridge/session/detach` request: stop receiving updates. The session keeps running.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "DetachSessionRequest".
 */
export interface DetachSessionRequest {
  sessionId: string;
}
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "DetachSessionResponse".
 */
export interface DetachSessionResponse {}
/**
 * `_bridge/request/resolved`: a pending approval or question was answered, possibly by another client.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "RequestResolvedNotification".
 */
export interface RequestResolvedNotification {
  sessionId: string;
  /**
   * The id of the request. It is the JSON-RPC id the daemon sent the request with.
   */
  requestId: string;
  /**
   * The method of the request, such as `session/request_permission`.
   */
  method: string;
  /**
   * The params of the request. Present in a replay, where the request itself is not sent.
   */
  params?: {
    [k: string]: unknown;
  };
  /**
   * The answer that was given, in the shape of the method's response.
   */
  outcome: {
    [k: string]: unknown;
  };
  /**
   * A label for the client that answered, when known.
   */
  resolvedBy?: string | null;
}
/**
 * `_bridge/sessions/updated`: a session appeared or its summary changed. Sent to every app.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "SessionsUpdatedNotification".
 */
export interface SessionsUpdatedNotification {
  session: SessionSummary;
}
/**
 * `_bridge/sessions/removed`: a session was deleted at its harness through the daemon, which
 * dropped its log too. Sent to every app.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "SessionsRemovedNotification".
 */
export interface SessionsRemovedNotification {
  sessionId: string;
}
/**
 * `_bridge/extension/register` request: this connection is the extension `namespace`.
 *
 * From then on it may add updates whose kind starts with `x.<namespace>.` to sessions, and
 * the daemon passes it every request whose method starts with `_x/<namespace>/`.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "RegisterExtensionRequest".
 */
export interface RegisterExtensionRequest {
  /**
   * Lower-case letters, digits and `-`.
   */
  namespace: string;
}
/**
 * `_bridge/session/append` request: add an update of the extension's own kind to a session.
 * It is numbered, stored, replayed and sent to every attached app like any other update.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "AppendRequest".
 */
export interface AppendRequest {
  sessionId: string;
  /**
   * The update body. Its `sessionUpdate` must start with `x.<namespace>.`.
   */
  update: {
    [k: string]: unknown;
  };
  /**
   * How the update changes what clients hold. Without it the update is kept as an event
   * that clients do not place among the items.
   */
  state?: ItemRule | null;
}
/**
 * How an extension's update changes client state: an ACP state rule whose target is the
 * update's own kind. The daemon stores it in the update under `_meta.bridge.state`, so every
 * client can apply it without knowing the extension.
 *
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "ItemRule".
 */
export interface ItemRule {
  /**
   * `append`, `upsert`, `replace` or `remove`.
   */
  action: string;
  /**
   * Dotted path of the item's id in the update, such as `checkpointId`.
   */
  id?: string | null;
  /**
   * The field of the item that is changed. Absent means the whole item.
   */
  field?: string | null;
  /**
   * Dotted path of the new data in the update. Absent means the whole update.
   */
  value?: string | null;
}
/**
 * This interface was referenced by `BridgeExtensionsToACPV2Draft`'s JSON-Schema
 * via the `definition` "AppendResponse".
 */
export interface AppendResponse {
  /**
   * The sequence number the update got.
   */
  seq: number;
}

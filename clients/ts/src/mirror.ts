// What a client of the daemon holds for one session, and how the daemon's messages change it.
// A port of `crates/bridge-core/src/mirror.rs`.

import { get, idString, isObject, type Json } from "./json.ts";
import { RULES, type RuleSet } from "./rules.ts";
import { Conversation } from "./state.ts";

export const REQUEST_RESOLVED = "_bridge/request/resolved";
export const SESSIONS_UPDATED = "_bridge/sessions/updated";

/** A client's copy of one session, including its subagents' child sessions. */
export class Mirror {
  conversation = new Conversation();
  /** The newest sequence number applied. Sent as `afterSeq` when attaching again. */
  seq = 0;

  /** Forgets everything, before a full replay. */
  reset(): void {
    this.conversation = new Conversation();
    this.seq = 0;
  }

  /** Notes a request this client sent, so the response can be applied when it arrives. */
  sent(message: Json, rules: RuleSet = RULES): void {
    // An answer to the agent is not applied here: the daemon confirms it with
    // `_bridge/request/resolved`, because another client may have answered first.
    if (isObject(message) && message["method"] === undefined) return;
    this.conversation.applyMessage("out", message, rules);
  }

  /** Applies a message from the daemon. */
  received(message: Json, rules: RuleSet = RULES): void {
    if (!isObject(message)) return;
    const seq = seqOf(message);
    if (seq !== undefined) this.seq = Math.max(this.seq, seq);
    const params = message["params"] ?? null;
    // The marker that ends a replay says nothing about the session itself.
    if (get(params, "update.sessionUpdate") === "replay_complete") return;
    const sessionId = get(params, "sessionId");
    const method = message["method"];

    if (method === REQUEST_RESOLVED && typeof sessionId === "string") {
      const id = idString(get(params, "requestId"));
      if (id === undefined) return;
      const session = this.conversation.session(sessionId);
      const request = get(params, "params");
      const requestMethod = get(params, "method");
      if (!session.requests.has(id) && typeof requestMethod === "string" && request !== undefined) {
        session.applyRequest(id, requestMethod, request, rules);
      }
      session.resolveRequest(id, get(params, "outcome") ?? null);
    } else if (typeof method === "string" && message["id"] !== undefined && typeof sessionId === "string") {
      const id = idString(message["id"]);
      if (id === undefined) return;
      const session = this.conversation.session(sessionId);
      // Sent again after a reconnect so it can be answered on this connection.
      if (session.requests.has(id)) return;
      session.applyRequest(id, method, withoutPosition(params), rules);
    } else {
      this.conversation.applyMessage("in", message, rules);
    }
  }
}

/** The request as the agent sent it, without the daemon's position mark. */
function withoutPosition(params: Json): Json {
  if (!isObject(params) || !isObject(params["_meta"])) return params;
  const { bridge: _bridge, ...meta } = params["_meta"];
  const { _meta, ...rest } = params;
  return Object.keys(meta).length > 0 ? { ...rest, _meta: meta } : rest;
}

/** The log position the daemon put on a message, if any. */
export function seqOf(message: Json): number | undefined {
  const seq = get(message, "params._meta.bridge.seq");
  return typeof seq === "number" ? seq : undefined;
}

/** The session whose log a message belongs to: for a subagent's messages, its parent's. */
export function rootOf(message: Json): string | undefined {
  const root = get(message, "params._meta.bridge.rootSessionId") ?? get(message, "params.sessionId");
  return typeof root === "string" ? root : undefined;
}

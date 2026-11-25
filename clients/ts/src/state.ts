// Client state, built by applying ACP v2 messages according to the schema's state rules.
//
// This is a port of `crates/archipel-core/src/state.rs`. Both must give the same state for the
// same messages; `testdata/states` holds what that state is for every recorded session.

import { base64Decode, base64Encode, get, idString, isObject, set, type Json, type JsonObject } from "./json.ts";
import { RULES, type RuleSet, type Rules, type StateRule } from "./rules.ts";

/** Fields whose streamed text is base64, as `target.field`. Joining them needs a decode first. */
const BASE64_FIELDS = new Set(["terminal.output.data"]);
/** Target name used for approvals and questions in the timeline. */
export const REQUEST_TARGET = "request";

/** Points at one entry of a collection. */
export interface ItemRef {
  target: string;
  id: string;
}

/** A request from the agent that waits for the user: an approval or a question. */
export interface Request {
  method: string;
  params: Json;
  /** The answer, once given. */
  outcome: Json | null;
}

/** Everything a client holds for one session. */
export class SessionState {
  /** Entries by target, then by id. */
  collections = new Map<string, Map<string, Json>>();
  /** Single values by target: `state`, `usage`, `configOptions`, ... */
  values = new Map<string, Json>();
  /** Entries in the order they first appeared. */
  timeline: ItemRef[] = [];
  /** Approvals and questions, by request id. */
  requests = new Map<string, Request>();
  /** Messages without state, in order: notices and the like. */
  events: Json[] = [];
  /** Messages whose kind the rules do not know and that carry no rule of their own. */
  unknown: Json[] = [];

  /** Applies the body of one `session/update` notification. */
  applyUpdate(update: Json, rules: RuleSet = RULES): void {
    const kind = isObject(update) ? update["sessionUpdate"] : undefined;
    if (typeof kind !== "string") throw new Error("update has no `sessionUpdate` field");
    const known = rules.updates[kind];
    if (!known) {
      // Not an ACP kind. An extension's update can carry its own rule.
      const own = ownRule(kind, update);
      if (own) this.applyRule(kind, own, update);
      else this.unknown.push(update);
      return;
    }
    if (!known.session?.length) {
      this.events.push(update);
      return;
    }
    this.applyRules(kind, known, update);
  }

  /** Records a request from the agent and applies its state rules. */
  applyRequest(requestId: string, method: string, params: Json, rules: RuleSet = RULES): void {
    const known = rules.requests[method];
    if (known) this.applyRules(method, known, params);
    const isNew = !this.requests.has(requestId);
    this.requests.set(requestId, { method, params, outcome: null });
    if (isNew) this.timeline.push({ target: REQUEST_TARGET, id: requestId });
  }

  /** Records the answer to a request. Returns false when the request is not known. */
  resolveRequest(requestId: string, outcome: Json): boolean {
    const request = this.requests.get(requestId);
    if (!request) return false;
    request.outcome = outcome;
    return true;
  }

  /** Applies the state rules of a response, such as the config options in `session/new`. */
  applyResponse(method: string, result: Json, rules: RuleSet = RULES): void {
    const known = rules.responses[method];
    if (known) this.applyRules(method, known, result);
  }

  /** The entry a timeline reference points at. */
  item(ref: ItemRef): Json | undefined {
    return this.collections.get(ref.target)?.get(ref.id);
  }

  /** The requests that have no answer yet, oldest first. */
  pendingRequests(): [string, Request][] {
    return [...this.requests].filter(([, request]) => request.outcome === null);
  }

  private applyRules(kind: string, rules: Rules, body: Json): void {
    for (const rule of rules.session ?? []) this.applyRule(kind, rule, body);
  }

  private applyRule(kind: string, rule: StateRule, body: Json): void {
    let id: string | undefined;
    if (rule.id !== undefined) {
      id = idString(get(body, rule.id));
      if (id === undefined) throw new Error(`\`${kind}\` has no id at \`${rule.id}\``);
    }
    let value: Json;
    if (rule.value !== undefined) {
      const found = get(body, rule.value);
      if (found === undefined || found === null) {
        // Optional data that is absent or null leaves the state alone.
        if (rule.action !== "append") return;
        if (found === undefined) throw new Error(`\`${kind}\` has no value at \`${rule.value}\``);
      }
      value = found as Json;
    } else {
      value = withoutKind(body);
    }

    if (rule.action === "remove") {
      if (id !== undefined) {
        this.collections.get(rule.target)?.delete(id);
        this.timeline = this.timeline.filter((ref) => !(ref.target === rule.target && ref.id === id));
      } else {
        this.values.delete(rule.target);
      }
      return;
    }
    if (id !== undefined) {
      let entries = this.collections.get(rule.target);
      if (!entries) {
        entries = new Map();
        this.collections.set(rule.target, entries);
      }
      if (!entries.has(id)) {
        const idKey = rule.id!.split(".").pop()!;
        entries.set(id, { [idKey]: id });
        this.timeline.push({ target: rule.target, id });
      }
      entries.set(id, changeEntry(entries.get(id)!, rule, value));
    } else {
      this.values.set(rule.target, changeEntry(this.values.get(rule.target) ?? null, rule, value));
    }
  }

  /** Rebuilds a state from its JSON form, for example to draw a stored session. */
  static fromJSON(json: Json): SessionState {
    const state = new SessionState();
    if (!isObject(json)) return state;
    for (const [target, entries] of Object.entries(isObject(json["collections"]) ? json["collections"] : {})) {
      state.collections.set(target, new Map(Object.entries(isObject(entries) ? entries : {})));
    }
    state.values = new Map(Object.entries(isObject(json["values"]) ? json["values"] : {}));
    state.timeline = (Array.isArray(json["timeline"]) ? json["timeline"] : []) as unknown as ItemRef[];
    state.requests = new Map(Object.entries(isObject(json["requests"]) ? json["requests"] : {})) as unknown as Map<string, Request>;
    state.events = Array.isArray(json["events"]) ? json["events"] : [];
    state.unknown = Array.isArray(json["unknown"]) ? json["unknown"] : [];
    return state;
  }

  /** The state as plain JSON, in the shape of `testdata/states`. */
  toJSON(): Json {
    const collections: JsonObject = {};
    for (const [target, entries] of this.collections) collections[target] = Object.fromEntries(entries);
    return {
      collections,
      values: Object.fromEntries(this.values),
      timeline: this.timeline as unknown as Json,
      requests: Object.fromEntries(this.requests) as unknown as Json,
      events: this.events,
      unknown: this.unknown,
    };
  }
}

/** The state rule an update carries for itself under `_meta.archipel.state`, if any. */
function ownRule(kind: string, update: Json): StateRule | undefined {
  const rule = get(update, "_meta.archipel.state");
  if (!isObject(rule)) return undefined;
  const action = rule["action"];
  if (action !== "append" && action !== "upsert" && action !== "replace" && action !== "remove") {
    return undefined;
  }
  const text = (key: string) => (typeof rule[key] === "string" ? (rule[key] as string) : undefined);
  const own: StateRule = { action, target: kind };
  const [id, field, value] = [text("id"), text("field"), text("value")];
  if (id !== undefined) own.id = id;
  if (field !== undefined) own.field = field;
  if (value !== undefined) own.value = value;
  return own;
}

/** The message body without its kind tag. */
function withoutKind(body: Json): Json {
  if (!isObject(body)) return body;
  const { sessionUpdate: _kind, ...rest } = body;
  return rest;
}

function changeEntry(entry: Json, rule: StateRule, value: Json): Json {
  if (rule.action === "upsert") return merge(entry, value);
  if (rule.field === undefined || rule.action === "remove") return value;
  const object: JsonObject = isObject(entry) ? entry : {};
  if (rule.action === "append") {
    const isBase64 = BASE64_FIELDS.has(`${rule.target}.${rule.field}`);
    set(object, rule.field, append(get(object, rule.field) ?? null, value, isBase64));
  } else {
    set(object, rule.field, value);
  }
  return object;
}

/** Merges the fields of `incoming` into `entry`. A null field clears that field. */
function merge(entry: Json, incoming: Json): Json {
  if (!isObject(entry) || !isObject(incoming)) return incoming;
  for (const [key, value] of Object.entries(incoming)) {
    if (value === null) delete entry[key];
    else entry[key] = value;
  }
  return entry;
}

/**
 * Appends streamed data to a field.
 *
 * - Text is concatenated. Base64 text is decoded first, so that the result is valid base64.
 * - A content block is added to a list; two neighbouring text blocks become one.
 */
function append(slot: Json, value: Json, isBase64: boolean): Json {
  if (typeof value === "string") {
    if (slot === null) return value;
    if (typeof slot === "string") return concatText(slot, value, isBase64);
  }
  if (slot === null) return [value];
  if (Array.isArray(slot)) {
    const last = slot[slot.length - 1];
    if (isObject(last) && last["type"] === "text" && typeof last["text"] === "string" && isObject(value)
      && value["type"] === "text" && typeof value["text"] === "string") {
      last["text"] = last["text"] + value["text"];
    } else {
      slot.push(value);
    }
    return slot;
  }
  return [slot, value];
}

function concatText(existing: string, text: string, isBase64: boolean): string {
  if (isBase64) {
    const left = base64Decode(existing);
    const right = base64Decode(text);
    if (left && right) {
      const joined = new Uint8Array(left.length + right.length);
      joined.set(left);
      joined.set(right, left.length);
      return base64Encode(joined);
    }
  }
  return existing + text;
}

/** Which way a message travelled, seen from the client: `in` is from the agent. */
export type Direction = "in" | "out";

/** One client's view of a conversation: the user's session and its subagents' child sessions. */
export class Conversation {
  /** Sessions by id, in the order they first appeared. The first is the one the user talks to. */
  sessions = new Map<string, SessionState>();
  /** Requests this side sent and has no answer to yet. */
  private outgoing = new Map<string, { method: string; sessionId: string | undefined }>();
  /** Requests from the agent, to find their session when the answer goes out. */
  private incoming = new Map<string, string>();

  /** The state of one session, created empty if it is new. */
  session(sessionId: string): SessionState {
    let state = this.sessions.get(sessionId);
    if (!state) {
      state = new SessionState();
      this.sessions.set(sessionId, state);
    }
    return state;
  }

  /** The first session seen, which is the one the user talks to. */
  root(): [string, SessionState] | undefined {
    return this.sessions.entries().next().value;
  }

  /** Applies one JSON-RPC message. */
  applyMessage(direction: Direction, message: Json, rules: RuleSet = RULES): void {
    if (!isObject(message)) return;
    const method = typeof message["method"] === "string" ? message["method"] : undefined;
    const id = idString(message["id"]);
    const params = message["params"] ?? null;
    const sessionId = isObject(params) && typeof params["sessionId"] === "string" ? params["sessionId"] : undefined;

    if (direction === "in" && method === "session/update" && id === undefined) {
      const update = isObject(params) ? params["update"] : undefined;
      if (sessionId !== undefined && update !== undefined) this.session(sessionId).applyUpdate(update, rules);
    } else if (direction === "in" && method !== undefined && id !== undefined) {
      if (sessionId === undefined) return;
      this.incoming.set(id, sessionId);
      this.session(sessionId).applyRequest(id, method, params, rules);
    } else if (direction === "out" && method !== undefined && id !== undefined) {
      this.outgoing.set(id, { method, sessionId });
    } else if (direction === "out" && method === undefined && id !== undefined) {
      const session = this.incoming.get(id);
      if (session === undefined) return;
      this.incoming.delete(id);
      this.session(session).resolveRequest(id, message["result"] ?? message["error"] ?? null);
    } else if (direction === "in" && method === undefined && id !== undefined) {
      const sent = this.outgoing.get(id);
      if (!sent) return;
      this.outgoing.delete(id);
      const result = message["result"];
      if (result === undefined) return;
      const session = sent.sessionId ?? (isObject(result) && typeof result["sessionId"] === "string" ? result["sessionId"] : undefined);
      if (session !== undefined) this.session(session).applyResponse(sent.method, result, rules);
    }
  }

  /** Rebuilds a conversation from its JSON form. */
  static fromJSON(json: Json): Conversation {
    const conversation = new Conversation();
    const sessions = isObject(json) && isObject(json["sessions"]) ? json["sessions"] : {};
    for (const [id, state] of Object.entries(sessions)) conversation.sessions.set(id, SessionState.fromJSON(state));
    return conversation;
  }

  toJSON(): Json {
    const sessions: JsonObject = {};
    for (const [id, state] of this.sessions) sessions[id] = state.toJSON();
    return { sessions };
  }
}

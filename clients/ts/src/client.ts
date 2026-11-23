// The client an app uses: sessions that stay in step with the daemon, across reconnects.

import { ClosedError, Connection, RpcError, type Transport } from "./connection.ts";
import type { OpenElsewhere } from "./generated/bridge.ts";
import { get, isObject, type Json, type JsonObject } from "./json.ts";
import { Mirror, rootOf, SESSIONS_UPDATED } from "./mirror.ts";
import type { Conversation, ItemRef, Request, SessionState } from "./state.ts";

/** One line of the session list. */
export interface SessionSummary {
  sessionId: string;
  agentId: string;
  title?: string;
  cwd: string;
  /** An adapter process holds the session. */
  live: boolean;
  /** The session is in the daemon's log. If not, opening it loads its history first. */
  logged: boolean;
  /**
   * Another process of the harness has the session open, such as its own terminal UI. The
   * daemon then refuses to act on it (error code -32004); `forkSession` makes a copy to use.
   */
  openElsewhere?: boolean;
  /** `running`, `requires_action` or `idle`. */
  state?: string;
  headSeq: number;
  updatedAt?: string;
}

export interface ClientOptions {
  /** Opens a transport. Called again after a disconnect. */
  open: () => Promise<Transport>;
  /** The app's name, shown to other apps when it answers a request. */
  name: string;
  version?: string;
  /** Called when a session appears or its summary changes. */
  onSessionUpdated?: (session: SessionSummary) => void;
  /** Called when the connection is lost (`false`) and when it is back (`true`). */
  onConnection?: (connected: boolean) => void;
  /** Milliseconds between attempts to reconnect. 0 disables reconnecting. Default 1000. */
  retryMs?: number;
}

export class BridgeClient {
  private options: ClientOptions;
  private connection!: Connection;
  private sessions = new Map<string, Session>();
  private closed = false;
  /** What the daemon answered to `initialize`: its capabilities, and this connection's scope. */
  welcome: Json = null;

  private constructor(options: ClientOptions) {
    this.options = options;
  }

  /** Connects and introduces the app to the daemon. */
  static async connect(options: ClientOptions): Promise<BridgeClient> {
    const client = new BridgeClient(options);
    await client.open();
    return client;
  }

  private async open(): Promise<void> {
    const connection = new Connection(await this.options.open());
    connection.onMessage = (message) => this.receive(message);
    connection.onSend = (message) => {
      const root = rootOf(message);
      if (root !== undefined) this.sessions.get(root)?.sent(message);
    };
    connection.onClose = () => void this.lost();
    this.connection = connection;
    this.welcome = await connection.call("initialize", {
      protocolVersion: 2,
      info: { name: this.options.name, version: this.options.version ?? "0.0.0" },
      capabilities: { elicitation: { form: {}, url: {} } },
    });
  }

  private async lost(): Promise<void> {
    if (this.closed) return;
    this.options.onConnection?.(false);
    const retry = this.options.retryMs ?? 1000;
    if (retry <= 0) return;
    while (!this.closed) {
      await new Promise((done) => setTimeout(done, retry));
      try {
        await this.open();
        // Every session continues from the last sequence number it applied.
        await Promise.all([...this.sessions.values()].map((session) => session.attach()));
        this.options.onConnection?.(true);
        return;
      } catch {
        // Not back yet.
      }
    }
  }

  private receive(message: Json): void {
    if (get(message, "method") === SESSIONS_UPDATED) {
      const summary = get(message, "params.session");
      if (isObject(summary)) this.options.onSessionUpdated?.(summary as unknown as SessionSummary);
      return;
    }
    const root = rootOf(message);
    if (root !== undefined) {
      this.sessions.get(root)?.received(message);
      return;
    }
    // A response: it belongs to the session whose request it answers.
    for (const session of this.sessions.values()) session.received(message);
  }

  /** Sends any request, for example an extension's `_x/<namespace>/…` method. */
  call(method: string, params: Json = {}): Promise<Json> {
    return this.connection.call(method, params);
  }

  /** The agents the daemon can start. */
  async agents(): Promise<Json[]> {
    const result = await this.call("_bridge/agents/list");
    return (get(result, "agents") as Json[] | undefined) ?? [];
  }

  /**
   * All sessions, newest first: the daemon's own and those each harness reports.
   * `loggedOnly` leaves the latter out and starts no agent.
   */
  async listSessions(filter: { agentId?: string; cwd?: string; loggedOnly?: boolean } = {}): Promise<SessionSummary[]> {
    const result = await this.call("_bridge/sessions/list", filter as JsonObject);
    return (get(result, "sessions") as unknown as SessionSummary[] | undefined) ?? [];
  }

  /** Creates a session and attaches to it. */
  async newSession(options: { cwd: string; agentId?: string; mcpServers?: Json[] }): Promise<Session> {
    const params: JsonObject = { cwd: options.cwd, mcpServers: options.mcpServers ?? [] };
    if (options.agentId) params["_meta"] = { bridge: { agentId: options.agentId } };
    const created = await this.call("session/new", params);
    const sessionId = get(created, "sessionId");
    if (typeof sessionId !== "string") throw new Error("the agent did not return a session id");
    return this.session(sessionId);
  }

  /**
   * Makes a copy of a session to continue with and attaches to it. The original is left as it
   * is. This is the way on when a session is open somewhere else.
   */
  async forkSession(sessionId: string): Promise<Session> {
    const forked = await this.call("session/fork", { sessionId });
    const id = get(forked, "sessionId");
    if (typeof id !== "string") throw new Error("the agent did not return a session id");
    return this.session(id);
  }

  /** Attaches to a session and resolves once the app holds all of it. */
  async session(sessionId: string): Promise<Session> {
    let session = this.sessions.get(sessionId);
    if (!session) {
      session = new Session(sessionId, () => this.connection, () => this.forkSession(sessionId));
      this.sessions.set(sessionId, session);
      await session.attach();
    }
    return session;
  }

  /** Stops following a session. It keeps running in the daemon. */
  async detach(sessionId: string): Promise<void> {
    this.sessions.delete(sessionId);
    await this.call("_bridge/session/detach", { sessionId }).catch(() => {});
  }

  close(): void {
    this.closed = true;
    this.connection.close();
  }
}

/** A session the app follows. `state` is always what the daemon has, up to `seq`. */
export class Session {
  readonly id: string;
  private mirror = new Mirror();
  private connection: () => Connection;
  private listeners = new Set<() => void>();
  /** Resolves the wait for the end of a replay. */
  private replayed: (() => void) | null = null;
  private forked: () => Promise<Session>;
  /**
   * Who else has the session open, as far as the daemon last said: another process of the
   * harness, such as its own terminal UI. While it does, the daemon refuses prompts and setting
   * changes (error -32004); `fork()` makes a copy to continue with. Known when the session is
   * attached and after every prompt, so it can be out of date in between.
   */
  openElsewhere: OpenElsewhere | null = null;

  constructor(id: string, connection: () => Connection, fork: () => Promise<Session>) {
    this.id = id;
    this.connection = connection;
    this.forked = fork;
  }

  /** Makes a copy of this session to continue with, and attaches to it. This one is left as it is. */
  fork(): Promise<Session> {
    return this.forked();
  }

  /** Sends a request that acts on the session, and notes whether another process stood in the way. */
  private async act(method: string, params: JsonObject): Promise<void> {
    try {
      await this.connection().call(method, params);
    } catch (error) {
      if (error instanceof RpcError && error.openElsewhere) {
        this.held(isObject(error.data) ? (error.data as unknown as OpenElsewhere) : { sessionId: this.id, pid: 0 });
      }
      throw error;
    }
    this.held(null);
  }

  private held(by: OpenElsewhere | null): void {
    if (by === null && this.openElsewhere === null) return;
    this.openElsewhere = by;
    for (const listener of this.listeners) listener();
  }

  /** The newest sequence number applied. */
  get seq(): number {
    return this.mirror.seq;
  }

  /** The user's session. Its `timeline` lists the items in order. */
  get state(): SessionState {
    return this.mirror.conversation.session(this.id);
  }

  /** A subagent's child session, by the id in its `subagent` item. */
  child(sessionId: string): SessionState | undefined {
    return this.mirror.conversation.sessions.get(sessionId);
  }

  /** Every session of the conversation: the user's and its subagents'. */
  get conversation(): Conversation {
    return this.mirror.conversation;
  }

  /** The whole conversation as JSON, in the shape of `testdata/states`. */
  toJSON(): Json {
    return this.mirror.conversation.toJSON();
  }

  /** The items in order, each with what it currently holds. */
  items(): { ref: ItemRef; item: Json | Request | undefined }[] {
    const state = this.state;
    return state.timeline.map((ref) => ({
      ref,
      item: ref.target === "request" ? state.requests.get(ref.id) : state.item(ref),
    }));
  }

  /** `running`, `requires_action` or `idle`. */
  get turnState(): string | undefined {
    const state = get(this.state.values.get("state") ?? null, "state");
    return typeof state === "string" ? state : undefined;
  }

  /** Approvals and questions nobody has answered, across the session and its subagents. */
  pendingRequests(): { requestId: string; sessionId: string; request: Request }[] {
    const pending = [];
    for (const [sessionId, state] of this.mirror.conversation.sessions) {
      for (const [requestId, request] of state.pendingRequests()) pending.push({ requestId, sessionId, request });
    }
    return pending;
  }

  /** Calls `listener` after every change. Returns a function that stops it. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Resolves when `done()` is true, checking after every change. */
  until(done: () => boolean): Promise<void> {
    if (done()) return Promise.resolve();
    return new Promise((resolve) => {
      const stop = this.subscribe(() => {
        if (done()) {
          stop();
          resolve();
        }
      });
    });
  }

  /** Sends a message to the agent. Resolves when the agent has taken it, not when it is done. */
  async prompt(content: string | Json[]): Promise<void> {
    const prompt = typeof content === "string" ? [{ type: "text", text: content }] : content;
    await this.act("session/prompt", { sessionId: this.id, prompt });
  }

  /** Answers an approval with one of its options. */
  approve(requestId: string, optionId: string): void {
    this.answer(requestId, { outcome: { outcome: "selected", optionId } });
  }

  /** Answers a request with a raw result, such as the form content of a question. */
  answer(requestId: string, result: Json): void {
    this.connection().answer(requestId, result);
  }

  /** Interrupts the running turn. */
  cancel(): void {
    this.connection().notify("session/cancel", { sessionId: this.id });
  }

  /** Changes a setting, such as `mode` or `model`. */
  async setConfigOption(configId: string, value: string): Promise<void> {
    await this.act("session/set_config_option", { sessionId: this.id, configId, type: "id", value });
  }

  /** Internal: (re)attaches from the last sequence number applied. */
  async attach(): Promise<void> {
    const params: JsonObject = { sessionId: this.id };
    if (this.mirror.seq > 0) params["afterSeq"] = this.mirror.seq;
    // The replay starts right after the response, so be ready for its end before asking.
    let finished = false;
    const done = new Promise<void>((resolve) => {
      this.replayed = () => {
        finished = true;
        resolve();
      };
    });
    this.resetOnReplay = true;
    let attached: Json;
    try {
      attached = await this.connection().call("_bridge/session/attach", params);
    } catch (error) {
      this.resetOnReplay = false;
      this.replayed = null;
      throw error;
    }
    const held = get(attached, "openElsewhere");
    this.openElsewhere = isObject(held) ? (held as unknown as OpenElsewhere) : null;
    if (get(attached, "catchUp") === "replay") {
      if (!finished) await done;
    } else {
      this.resetOnReplay = false;
      this.replayed = null;
      const head = Number(get(attached, "headSeq") ?? 0);
      await this.until(() => this.mirror.seq >= head);
    }
  }

  private resetOnReplay = false;

  /** Internal: a message the app sent about this session. */
  sent(message: Json): void {
    this.mirror.sent(message);
  }

  /** Internal: a message from the daemon for this session. */
  received(message: Json): void {
    // The attach response says whether a replay follows; if so, start from nothing.
    if (this.resetOnReplay && isObject(message) && message["method"] === undefined) {
      const catchUp = get(message, "result.catchUp");
      if (catchUp !== undefined) {
        this.resetOnReplay = false;
        if (catchUp === "replay") this.mirror.reset();
        return;
      }
    }
    try {
      this.mirror.received(message);
    } catch (error) {
      if (!(error instanceof ClosedError)) console.error("bridge: could not apply a message:", error);
    }
    if (get(message, "params.update.sessionUpdate") === "replay_complete") {
      this.replayed?.();
      this.replayed = null;
    }
    for (const listener of this.listeners) listener();
  }
}

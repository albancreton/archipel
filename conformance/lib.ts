// What the scenarios stand on: a daemon of their own, an app connected to it, a direct line to
// the adapter, and a way to note what was observed.

import { deflateSync } from "node:zlib";
import { ArchipelClient, unixSocketTransport, type Json, type JsonObject, type Session } from "@archipel/client";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";

export const here = import.meta.dirname;
export const root = path.join(here, "..");

export interface AgentConfig {
  /** The agent's entry in the daemon's `config.json`. */
  daemon: { id: string; profile: string; command: string; args: string[]; env?: Record<string, string> } & JsonObject;
  /** The cheapest model, selected before any turn that does not test the model itself. */
  cheapModel?: string;
  /** A second cheap model, to tell "kept the choice" from "fell back to the default". */
  otherCheapModel?: string;
  /** A shell line to run before the daemon starts, for agents that need the user's environment. */
  shell?: string;
}

/** A daemon started for this run, in a home of its own. */
export class Daemon {
  home: string;
  private agent: AgentConfig;
  private child: ChildProcess | null = null;
  log = "";

  constructor(agent: AgentConfig) {
    this.agent = agent;
    // Short on purpose: a Unix socket path is limited to about 100 bytes.
    this.home = path.join(root, "target/conformance", agent.daemon.id);
    rmSync(this.home, { recursive: true, force: true });
    mkdirSync(this.home, { recursive: true });
    this.configure({});
  }

  /** Writes the config. `addons` switches addons on or off; it applies from the next start. */
  configure(addons: Record<string, boolean>): void {
    writeFileSync(path.join(this.home, "config.json"), JSON.stringify({ agents: [this.agent.daemon], addons }));
  }

  async start(): Promise<void> {
    const archipeld = path.join(root, "target/debug/archipeld");
    if (!existsSync(archipeld)) throw new Error("run `cargo build` first");
    const args = ["--home", this.home];
    const child = this.agent.shell
      ? spawn("zsh", ["-c", `${this.agent.shell} >/dev/null 2>&1; exec "$0" "$@"`, archipeld, ...args], { stdio: ["ignore", "ignore", "pipe"] })
      : spawn(archipeld, args, { stdio: ["ignore", "ignore", "pipe"] });
    this.child = child;
    await new Promise<void>((resolve, reject) => {
      child.stderr!.on("data", (chunk) => {
        this.log += chunk;
        if (this.log.includes("listening on")) resolve();
      });
      child.once("exit", (code) => reject(new Error(`archipeld exited with ${code}: ${this.log}`)));
    });
  }

  async stop(): Promise<void> {
    const child = this.child;
    if (!child || child.exitCode !== null) return;
    this.child = null;
    child.removeAllListeners("exit");
    const gone = new Promise((done) => child.once("exit", done));
    child.kill("SIGINT");
    await gone;
  }

  connect(): Promise<ArchipelClient> {
    return ArchipelClient.connect({
      open: () => unixSocketTransport(path.join(this.home, "daemon.sock")),
      name: "conformance",
      retryMs: 0,
    });
  }
}

export class RpcFailure extends Error {
  code: number;
  constructor(code: number, message: string) {
    super(message);
    this.code = code;
  }
}

/** The adapter itself, spoken to directly: for what the daemon does not pass on yet. */
export class Adapter {
  private child: ChildProcess;
  private nextId = 1;
  private waiting = new Map<number, { resolve: (result: Json) => void; reject: (error: Error) => void }>();
  welcome: Json = null;

  private constructor(child: ChildProcess) {
    this.child = child;
    createInterface({ input: child.stdout! }).on("line", (line) => {
      let message: JsonObject;
      try {
        message = JSON.parse(line);
      } catch {
        return;
      }
      if (message["method"] !== undefined) {
        // A request from the agent that nobody here answers would hang it: decline.
        if (message["id"] !== undefined) this.send({ jsonrpc: "2.0", id: message["id"]!, error: { code: -32601, message: "not available" } });
        return;
      }
      const waiting = this.waiting.get(Number(message["id"]));
      if (!waiting) return;
      this.waiting.delete(Number(message["id"]));
      const error = message["error"] as JsonObject | undefined;
      if (error) waiting.reject(new RpcFailure(Number(error["code"]), String(error["message"])));
      else waiting.resolve(message["result"] ?? null);
    });
  }

  static async start(agent: AgentConfig): Promise<Adapter> {
    const child = spawn(agent.daemon.command, agent.daemon.args, {
      env: { ...process.env, ...agent.daemon.env },
      stdio: ["pipe", "pipe", "ignore"],
    });
    const adapter = new Adapter(child);
    adapter.welcome = await adapter.call("initialize", {
      protocolVersion: 2,
      info: { name: "archipel-conformance", version: "0.0.0" },
      capabilities: { elicitation: { form: {}, url: {} } },
    });
    return adapter;
  }

  private send(message: JsonObject): void {
    this.child.stdin!.write(`${JSON.stringify(message)}\n`);
  }

  call(method: string, params: JsonObject = {}): Promise<Json> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.send({ jsonrpc: "2.0", id, method, params });
    });
  }

  async stop(): Promise<void> {
    if (this.child.exitCode !== null) return;
    const gone = new Promise((done) => this.child.once("exit", done));
    this.child.stdin!.end();
    this.child.kill("SIGTERM");
    await gone;
  }
}

export interface Outcome {
  /** What was observed and should stay the same from run to run. Compared with the baseline. */
  facts: Record<string, Json>;
  /** What was observed and may vary: durations, lists that change with every release. */
  info: Record<string, Json>;
  /** What must hold whatever the harness version. */
  failed: string[];
}

/** What a scenario is given. */
export class Run {
  agent: AgentConfig;
  daemon: Daemon;
  work: string;
  outcome: Outcome = { facts: {}, info: {}, failed: [] };
  /** Sessions made by this run, to delete at the end. */
  made: Set<string>;
  private clients: ArchipelClient[] = [];
  private adapters: Adapter[] = [];

  constructor(agent: AgentConfig, daemon: Daemon, work: string, made: Set<string>) {
    this.agent = agent;
    this.daemon = daemon;
    this.work = work;
    this.made = made;
  }

  get id(): string {
    return this.agent.daemon.id;
  }

  fact(name: string, value: Json): void {
    this.outcome.facts[name] = value;
  }

  info(name: string, value: Json): void {
    this.outcome.info[name] = value;
  }

  /** Something that must be true. The scenario goes on, to report everything it can. */
  must(what: string, holds: boolean): void {
    if (!holds) this.outcome.failed.push(what);
  }

  async app(): Promise<ArchipelClient> {
    const client = await this.daemon.connect();
    this.clients.push(client);
    return client;
  }

  async adapter(): Promise<Adapter> {
    const adapter = await Adapter.start(this.agent);
    this.adapters.push(adapter);
    return adapter;
  }

  /** A new session, on the cheap model. */
  async session(app: ArchipelClient): Promise<Session> {
    const session = await app.newSession({ cwd: this.work, agentId: this.id });
    this.made.add(session.id);
    if (this.agent.cheapModel && current(session, "model") !== this.agent.cheapModel) {
      await session.setConfigOption("model", this.agent.cheapModel);
    }
    return session;
  }

  /**
   * Stops the daemon and starts it again: every session loses its process. `addons` switches
   * addons on or off for the new daemon; without it they are as shipped.
   */
  async restartDaemon(addons: Record<string, boolean> = {}): Promise<void> {
    for (const client of this.clients.splice(0)) client.close();
    await this.daemon.stop();
    this.daemon.configure(addons);
    await this.daemon.start();
  }

  async close(): Promise<void> {
    for (const client of this.clients.splice(0)) client.close();
    for (const adapter of this.adapters.splice(0)) await adapter.stop();
  }
}

export interface Setting {
  id: string;
  category: string;
  type: string;
  current: Json;
  values: string[];
}

/** The settings the session reports, by id. */
export function settings(session: Session): Map<string, Setting> {
  return settingsOf(session.state.values.get("configOptions") ?? null);
}

export function settingsOf(options: Json): Map<string, Setting> {
  const all = new Map<string, Setting>();
  for (const option of (Array.isArray(options) ? options : []) as JsonObject[]) {
    const id = String(option["configId"] ?? option["id"]);
    const values = ((option["options"] as JsonObject[] | undefined) ?? []).map((choice) => String(choice["value"]));
    all.set(id, {
      id,
      category: String(option["category"] ?? ""),
      type: String(option["type"] ?? ""),
      current: option["currentValue"] ?? null,
      values,
    });
  }
  return all;
}

export const current = (session: Session, id: string): Json => settings(session).get(id)?.current ?? null;

/** The id of the first setting of a category, such as `thought_level`. */
export function settingOf(session: Session, category: string): Setting | undefined {
  return [...settings(session).values()].find((setting) => setting.category === category);
}

export function within<T>(ms: number, what: string, work: Promise<T>): Promise<T> {
  let timer: NodeJS.Timeout;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`timed out after ${ms / 1000}s: ${what}`)), ms);
  });
  return Promise.race([work, late]).finally(() => clearTimeout(timer));
}

export interface Turn {
  ms: number;
  stopReason: Json;
  /** The model the harness says it used, where it says so. */
  model: Json;
  approvals: number;
}

/**
 * Sends a message and waits for the turn to end. `during` is called after every change, for
 * scenarios that act or watch while the turn runs. Approvals are allowed once, or refused, or
 * left for the scenario to answer (`leave`).
 */
export async function turn(
  session: Session,
  content: string | Json[],
  during?: (session: Session) => void,
  approvals: "allow" | "reject" | "leave" = "allow",
): Promise<Turn> {
  const text = typeof content === "string" ? content : JSON.stringify(content);
  const started = Date.now();
  let ran = false;
  let asked = 0;
  const answered = new Set<string>();
  const stop = session.subscribe(() => {
    if (session.turnState !== undefined && session.turnState !== "idle") ran = true;
    for (const { requestId, request } of session.pendingRequests()) {
      if (answered.has(requestId) || request.method !== "session/request_permission") continue;
      answered.add(requestId);
      asked += 1;
      if (approvals === "leave") continue;
      const options = ((request.params as JsonObject)["options"] as JsonObject[] | undefined) ?? [];
      const chosen = options.find((option) => String(option["kind"]).startsWith(approvals)) ?? options[0];
      if (chosen) session.approve(requestId, String(chosen["optionId"]));
    }
    during?.(session);
  });
  try {
    await session.prompt(content);
    // The state is `idle` before the turn starts, and again while a sleeping session wakes up.
    await within(240_000, `the turn for "${text.slice(0, 40)}"`, session.until(() => ran && session.turnState === "idle"));
  } finally {
    stop();
  }
  const state = session.state.values.get("state") as JsonObject | undefined;
  const usage = session.state.values.get("usage") as JsonObject | undefined;
  const meta = usage?.["_meta"] as JsonObject | undefined;
  return { ms: Date.now() - started, stopReason: state?.["stopReason"] ?? null, model: meta?.["_claude/model"] ?? null, approvals: asked };
}

/** Everything the agent said in the session, as one string. */
export function said(session: Session): string {
  const parts: string[] = [];
  const collect = (value: Json): void => {
    if (Array.isArray(value)) value.forEach(collect);
    else if (value && typeof value === "object") {
      if (typeof value["text"] === "string") parts.push(value["text"]);
      else Object.values(value).forEach(collect);
    }
  };
  for (const item of session.state.collections.get("agentMessage")?.values() ?? []) collect(item);
  return parts.join("");
}

export const count = (session: Session, target: string): number => session.state.collections.get(target)?.size ?? 0;

export function readJson(file: string): JsonObject | null {
  return existsSync(file) ? (JSON.parse(readFileSync(file, "utf8")) as JsonObject) : null;
}

/** A PNG of one colour, base64: something a model can only describe by looking at it. */
export function solidPng(red: number, green: number, blue: number, size = 64): string {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (bytes: Buffer) => {
    let c = 0xffffffff;
    for (const byte of bytes) c = crcTable[(c ^ byte) & 0xff]! ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const sum = Buffer.alloc(4);
    sum.writeUInt32BE(crc(body));
    return Buffer.concat([length, body, sum]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header.set([8, 2, 0, 0, 0], 8); // 8 bits, RGB
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(size * 3).map((_, at) => [red, green, blue][at % 3]!)]);
  const pixels = deflateSync(Buffer.concat(Array.from({ length: size }, () => row)));
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", pixels), chunk("IEND", Buffer.alloc(0))]).toString("base64");
}

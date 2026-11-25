// Settings before a session exists, renaming and deleting: what the client sends and returns.
// The daemon here is a stand-in that records the calls; the real one is tested in Rust.

import assert from "node:assert/strict";
import test from "node:test";
import { ArchipelClient, type Json, type JsonObject, type Transport } from "../src/index.ts";

function standIn(sent: JsonObject[]): Transport {
  const transport: Transport = {
    onMessage: null,
    onClose: null,
    close: () => {},
    send(message) {
      const { id, method, params } = message as { id?: number; method: string; params: JsonObject };
      if (id === undefined) return;
      sent.push({ method, params });
      const say = (frame: JsonObject) => queueMicrotask(() => transport.onMessage?.({ jsonrpc: "2.0", ...frame }));
      if (method === "_archipel/session/attach") {
        say({ id, result: { catchUp: "replay", headSeq: 0 } });
        const update: Json = { sessionUpdate: "replay_complete" };
        say({ method: "session/update", params: { sessionId: params["sessionId"] as string, update } });
      } else if (method === "session/options") {
        say({ id, result: { configOptions: [{ id: "model" }] } });
      } else if (method === "session/set_title") {
        say({ id, result: { title: String(params["title"]).trim() } });
      } else if (method === "_archipel/agents/list") {
        say({ id, result: { agents: [{ agentId: "claude", name: "Claude Code", readiness: "ready", capabilities: { session: { title: {} } } }] } });
      } else {
        say({ id, result: {} });
      }
    },
  };
  return transport;
}

test("settings without a session, rename and delete", async () => {
  const sent: JsonObject[] = [];
  const app = await ArchipelClient.connect({ open: async () => standIn(sent), name: "test", retryMs: 0 });
  const last = (): JsonObject => sent[sent.length - 1]!;

  const fresh = await app.sessionOptions({ cwd: "/work", agentId: "pi" });
  assert.deepEqual(fresh["configOptions"], [{ id: "model" }]);
  assert.deepEqual(last(), { method: "session/options", params: { cwd: "/work", _meta: { archipel: { agentId: "pi" } } } });
  await app.sessionOptions({ cwd: "/work", sessionId: "s1" });
  assert.deepEqual(last().params, { cwd: "/work", sessionId: "s1" });

  assert.equal(await app.setTitle("s1", "  tidy  "), "tidy", "the title the harness stored");

  const [agent] = await app.agents();
  assert.ok((agent?.capabilities as JsonObject | undefined)?.["session"]);

  // A deleted session is no longer followed: asking for it again attaches anew.
  await app.session("s1");
  await app.deleteSession("s1");
  assert.deepEqual(last(), { method: "session/delete", params: { sessionId: "s1" } });
  const attaches = (): number => sent.filter((call) => call["method"] === "_archipel/session/attach").length;
  assert.equal(attaches(), 1);
  await app.session("s1");
  assert.equal(attaches(), 2);
  app.close();
});

/** A daemon that knows the sessions in `known`, can say one was deleted, and can drop the connection. */
function daemonWith(known: Set<string>) {
  let current: Transport | null = null;
  const open = async (): Promise<Transport> => {
    const transport: Transport = {
      onMessage: null,
      onClose: null,
      close: () => {},
      send(message) {
        const { id, method, params } = message as { id?: number; method: string; params: JsonObject };
        if (id === undefined) return;
        const say = (frame: JsonObject) => queueMicrotask(() => transport.onMessage?.({ jsonrpc: "2.0", ...frame }));
        const sessionId = params["sessionId"] as string;
        if (method !== "_archipel/session/attach") return say({ id, result: {} });
        if (!known.has(sessionId)) return say({ id, error: { code: -32602, message: "unknown session; list the sessions first" } });
        say({ id, result: { catchUp: "replay", headSeq: 0 } });
        const update: Json = { sessionUpdate: "replay_complete" };
        say({ method: "session/update", params: { sessionId, update } });
      },
    };
    current = transport;
    return transport;
  };
  return {
    open,
    say: (frame: JsonObject) => current?.onMessage?.({ jsonrpc: "2.0", ...frame }),
    drop: () => current?.onClose?.(),
  };
}

test("a session another app deleted is marked, stops waiting and is no longer followed", async () => {
  const daemon = daemonWith(new Set(["s1", "s2"]));
  const gone: string[] = [];
  const app = await ArchipelClient.connect({ open: daemon.open, name: "test", retryMs: 0, onSessionRemoved: (id) => gone.push(id) });
  const session = await app.session("s1");
  const other = await app.session("s2");
  let heard = 0;
  session.subscribe(() => (heard += 1));
  const waiting = session.until(() => session.turnState === "running");

  daemon.say({ method: "_archipel/sessions/removed", params: { sessionId: "s1" } });
  await waiting; // nothing will change any more, so the wait ends
  assert.equal(session.removed, true);
  assert.equal(heard, 1, "listeners are told once");
  assert.deepEqual(gone, ["s1"]);
  assert.equal(other.removed, false);
  // A session nobody here follows: the app is still told, for its list.
  daemon.say({ method: "_archipel/sessions/removed", params: { sessionId: "elsewhere" } });
  assert.deepEqual(gone, ["s1", "elsewhere"]);
  app.close();
});

test("a session deleted while the connection was down does not stop the client coming back", async () => {
  const known = new Set(["s1", "s2"]);
  const daemon = daemonWith(known);
  const gone: string[] = [];
  let back: () => void = () => {};
  const returned = new Promise<void>((resolve) => (back = resolve));
  const app = await ArchipelClient.connect({
    open: daemon.open,
    name: "test",
    retryMs: 1,
    onSessionRemoved: (id) => gone.push(id),
    onConnection: (connected) => connected && back(),
  });
  const deleted = await app.session("s1");
  const kept = await app.session("s2");

  known.delete("s1");
  daemon.drop();
  await returned;
  assert.equal(deleted.removed, true);
  assert.equal(kept.removed, false);
  assert.deepEqual(gone, ["s1"]);
  app.close();
});

test("an error outside any request is kept on its session until the next change arrives", async () => {
  const daemon = daemonWith(new Set(["s1", "s2"]));
  const app = await ArchipelClient.connect({ open: daemon.open, name: "test", retryMs: 0 });
  const session = await app.session("s1");
  const other = await app.session("s2");
  let heard = 0;
  session.subscribe(() => (heard += 1));

  daemon.say({ method: "_archipel/session/error", params: { sessionId: "s1", code: -32005, message: "the disk is full" } });
  assert.equal(session.error?.storage, true);
  assert.equal(session.error?.message, "the disk is full");
  assert.equal(heard, 1);
  assert.equal(other.error, null);
  // The log is written again: the first update that arrives clears it.
  const update: Json = { sessionUpdate: "state_update", state: "running" };
  daemon.say({ method: "session/update", params: { sessionId: "s1", update, _meta: { archipel: { seq: 1 } } } });
  assert.equal(session.turnState, "running");
  assert.equal(session.error, null);
  app.close();
});

test("the app is told once when a session starts to need the user, and when the connection goes and comes", async () => {
  const daemon = daemonWith(new Set(["s1"]));
  const heard: string[] = [];
  const app = await ArchipelClient.connect({
    open: daemon.open, name: "test", retryMs: 1,
    onAttention: (session, why) => heard.push(`${session.sessionId} ${why}`),
  });
  const say = (sessionId: string, state: string) =>
    daemon.say({ method: "_archipel/sessions/updated", params: { session: { sessionId, agentId: "claude", cwd: "/w", live: true, logged: true, headSeq: 1, state } } });
  say("s1", "idle"); // first sight of an idle session: nothing happened
  say("s1", "running");
  say("s1", "requires_action");
  say("s1", "requires_action"); // a repeat, such as a title change meanwhile
  say("s1", "running");
  say("s1", "idle");
  say("s2", "requires_action"); // first sight of one that already waits
  assert.deepEqual(heard, ["s1 needs_answer", "s1 finished", "s2 needs_answer"]);

  assert.equal(app.connected, true);
  const seen: boolean[] = [];
  const stop = app.onConnectionChange((connected) => seen.push(connected));
  daemon.drop();
  assert.equal(app.connected, false);
  await new Promise<void>((back) => { const off = app.onConnectionChange((connected) => { if (connected) { off(); back(); } }); });
  assert.deepEqual(seen, [false, true]);
  stop();
  app.close();
});


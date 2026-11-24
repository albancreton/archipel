// Settings before a session exists, renaming and deleting: what the client sends and returns.
// The daemon here is a stand-in that records the calls; the real one is tested in Rust.

import assert from "node:assert/strict";
import test from "node:test";
import { BridgeClient, type Json, type JsonObject, type Transport } from "../src/index.ts";

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
      if (method === "_bridge/session/attach") {
        say({ id, result: { catchUp: "replay", headSeq: 0 } });
        const update: Json = { sessionUpdate: "replay_complete" };
        say({ method: "session/update", params: { sessionId: params["sessionId"] as string, update } });
      } else if (method === "session/options") {
        say({ id, result: { configOptions: [{ id: "model" }] } });
      } else if (method === "session/set_title") {
        say({ id, result: { title: String(params["title"]).trim() } });
      } else if (method === "_bridge/agents/list") {
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
  const app = await BridgeClient.connect({ open: async () => standIn(sent), name: "test", retryMs: 0 });
  const last = (): JsonObject => sent[sent.length - 1]!;

  const fresh = await app.sessionOptions({ cwd: "/work", agentId: "pi" });
  assert.deepEqual(fresh["configOptions"], [{ id: "model" }]);
  assert.deepEqual(last(), { method: "session/options", params: { cwd: "/work", _meta: { bridge: { agentId: "pi" } } } });
  await app.sessionOptions({ cwd: "/work", sessionId: "s1" });
  assert.deepEqual(last().params, { cwd: "/work", sessionId: "s1" });

  assert.equal(await app.setTitle("s1", "  tidy  "), "tidy", "the title the harness stored");

  const [agent] = await app.agents();
  assert.ok((agent?.capabilities as JsonObject | undefined)?.["session"]);

  // A deleted session is no longer followed: asking for it again attaches anew.
  await app.session("s1");
  await app.deleteSession("s1");
  assert.deepEqual(last(), { method: "session/delete", params: { sessionId: "s1" } });
  const attaches = (): number => sent.filter((call) => call["method"] === "_bridge/session/attach").length;
  assert.equal(attaches(), 1);
  await app.session("s1");
  assert.equal(attaches(), 2);
  app.close();
});

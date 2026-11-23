// A session that another process of the harness has open: what the client knows, and the fork.
// The daemon here is a stand-in that answers four methods; the real one is tested in Rust.

import assert from "node:assert/strict";
import test from "node:test";
import { BridgeClient, RpcError, type Json, type JsonObject, type Transport } from "../src/index.ts";

/** A daemon with one session, `s1`, that `holder` has open while it is set. */
function standIn(daemon: { holder: JsonObject | null }): Transport {
  const transport: Transport = {
    onMessage: null,
    onClose: null,
    close: () => {},
    send(message) {
      const { id, method, params } = message as { id?: number; method: string; params: JsonObject };
      if (id === undefined) return;
      const sessionId = params["sessionId"] as string;
      const say = (frame: JsonObject) => queueMicrotask(() => transport.onMessage?.({ jsonrpc: "2.0", ...frame }));
      const held = sessionId === "s1" ? daemon.holder : null;
      if (method === "_bridge/session/attach") {
        say({ id, result: { catchUp: "replay", headSeq: 0, ...(held ? { openElsewhere: held } : {}) } });
        const update: Json = { sessionUpdate: "replay_complete" };
        say({ method: "session/update", params: { sessionId, update } });
      } else if (method === "session/prompt" && held) {
        say({ id, error: { code: -32004, message: "open in another process", data: held } });
      } else if (method === "session/fork") {
        say({ id, result: { sessionId: `${sessionId}-fork` } });
      } else {
        say({ id, result: {} });
      }
    },
  };
  return transport;
}

test("a session says who else has it open, and its fork is free to use", async () => {
  const daemon = { holder: { sessionId: "s1", pid: 4242, status: "busy" } as JsonObject | null };
  const app = await BridgeClient.connect({ open: async () => standIn(daemon), name: "test", retryMs: 0 });
  const session = await app.session("s1");
  assert.equal(session.openElsewhere?.pid, 4242, "known from the attach");

  let changes = 0;
  session.subscribe(() => (changes += 1));
  await assert.rejects(session.prompt("hello"), (error) => error instanceof RpcError && error.openElsewhere);
  assert.equal(session.openElsewhere?.status, "busy");

  // The other process closes it: the next prompt goes through, and the session says so.
  daemon.holder = null;
  await session.prompt("hello");
  assert.equal(session.openElsewhere, null);
  assert.ok(changes > 0, "listeners hear about it");

  daemon.holder = { sessionId: "s1", pid: 4242 };
  const copy = await session.fork();
  assert.equal(copy.id, "s1-fork");
  assert.equal(copy.openElsewhere, null);
  await copy.prompt("hello");
  app.close();
});

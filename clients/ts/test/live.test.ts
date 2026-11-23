// Runs the client against the real daemon program, with a fake agent that replays a recording.
// Needs `cargo build` first: it starts `target/debug/bridged`.

import assert from "node:assert/strict";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { BridgeClient, commandTransport, pair, RpcError, unixSocketTransport, webSocketTransport, type SessionSummary } from "../src/index.ts";

const root = path.join(import.meta.dirname, "../../..");
const bridged = path.join(root, "target/debug/bridged");

/** Starts a daemon in a fresh directory and resolves with its WebSocket address. */
async function daemon(home: string, scenario: string): Promise<{ process: ChildProcess; ws: string }> {
  const recording = path.join(root, `testdata/transcripts/${scenario}.ndjson`);
  const child = spawn(bridged, ["--home", home, "--ws", "127.0.0.1:0", "--fake", recording, "--fake-speed", "0"], {
    stdio: ["ignore", "ignore", "pipe"],
  });
  let log = "";
  return new Promise((resolve, reject) => {
    child.stderr!.on("data", (chunk) => {
      log += chunk;
      const ws = /listening on (ws:\/\/\S+) \(token/.exec(log);
      if (ws) resolve({ process: child, ws: ws[1]! });
    });
    child.once("exit", (code) => reject(new Error(`bridged exited with ${code}: ${log}`)));
  });
}

const plain = (value: unknown): unknown => JSON.parse(JSON.stringify(value));

test("a session driven through the real daemon ends in the reference state", { skip: !existsSync(bridged) }, async () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "bridge-ts-"));
  const { process: child, ws } = await daemon(home, "claude-v2/approval_denied");
  try {
    const updates: SessionSummary[] = [];
    const app = await BridgeClient.connect({
      open: () => unixSocketTransport(path.join(home, "daemon.sock")),
      name: "ts-test",
      onSessionUpdated: (session) => updates.push(session),
      retryMs: 0,
    });
    assert.equal((app.welcome as { _meta: { bridge: { scope: string } } })._meta.bridge.scope, "admin");

    // The steps of the recording: new session, default mode, a prompt, then "No" to the approval.
    const session = await app.newSession({ cwd: "/work/probe" });
    await session.setConfigOption("mode", "default");
    await session.prompt(
      "Run this exact shell command: printf 'denied fixture' > probe-denied.txt  Then reply with exactly one word saying whether it ran: yes or no.",
    );
    await session.until(() => session.pendingRequests().length > 0);
    assert.equal(session.turnState, "requires_action");
    const [pending] = session.pendingRequests();
    session.approve(pending!.requestId, "reject");
    await session.until(() => session.turnState === "idle");

    const expected = JSON.parse(readFileSync(path.join(root, "testdata/daemon/claude-v2/approval_denied.state.json"), "utf8"));
    assert.deepStrictEqual(plain(session), expected);
    assert.ok(updates.some((update) => update.state === "running"), "the session list was kept up to date");

    // A second app, over the WebSocket, gets the same from the log.
    const token = readFileSync(path.join(home, "ws-token"), "utf8").trim();
    const other = await BridgeClient.connect({ open: () => webSocketTransport(ws, { token }), name: "ts-test-2", retryMs: 0 });
    const listed = await other.listSessions({ loggedOnly: true });
    assert.equal(listed.length, 1);
    assert.equal(listed[0]!.sessionId, session.id);
    const replayed = await other.session(session.id);
    assert.deepStrictEqual(plain(replayed), expected);
    assert.equal(replayed.seq, session.seq);

    // A third app pairs with a one-time code, gets a token of its own, and may only observe.
    const code = execFileSync(bridged, ["--home", home, "pair", "--scope", "observe", "--name", "tablet"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
    const paired = await pair(ws, code);
    assert.deepStrictEqual([paired.scope, paired.name], ["observe", "tablet"]);
    const tablet = await BridgeClient.connect({ open: () => webSocketTransport(ws, { token: paired.token }), name: "tablet", retryMs: 0 });
    const watched = await tablet.session(session.id);
    assert.deepStrictEqual(plain(watched), expected);
    await assert.rejects(watched.prompt("may I?"), RpcError);
    await assert.rejects(pair(ws, code), "a code works once");
    tablet.close();

    // The same session through a command that carries the connection, as SSH would.
    const bridge = path.join(root, "target/debug/bridge");
    const carried = await BridgeClient.connect({
      open: () => commandTransport(bridge, ["--home", home, "connect", "--stdio"]),
      name: "ts-test-3",
      retryMs: 0,
    });
    assert.deepStrictEqual(plain(await carried.session(session.id)), expected);
    carried.close();

    // A wrong token is refused, and so is an unknown session.
    await assert.rejects(BridgeClient.connect({ open: () => webSocketTransport(ws, { token: "wrong" }), name: "x", retryMs: 0 }));
    await assert.rejects(other.session("no-such-session"), RpcError);
    app.close();
    other.close();
  } finally {
    child.kill("SIGINT");
    await new Promise((done) => child.once("exit", done));
    rmSync(home, { recursive: true, force: true });
  }
});

test("a client that loses the daemon reconnects and continues from where it was", { skip: !existsSync(bridged) }, async () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "bridge-ts-"));
  let running = await daemon(home, "claude-v2/simple");
  try {
    const connection: boolean[] = [];
    const app = await BridgeClient.connect({
      open: () => unixSocketTransport(path.join(home, "daemon.sock")),
      name: "ts-test",
      onConnection: (connected) => connection.push(connected),
      retryMs: 50,
    });
    const session = await app.newSession({ cwd: "/" });
    await session.prompt("Respond with the following text: fixture simple ok");
    await session.until(() => session.turnState === "idle");
    const before = plain(session);
    const seq = session.seq;

    // The daemon stops and starts again on the same directory.
    running.process.kill("SIGINT");
    await new Promise((done) => running.process.once("exit", done));
    running = await daemon(home, "claude-v2/simple");
    await new Promise<void>((resolve) => {
      const check = () => (connection.at(-1) === true ? resolve() : setTimeout(check, 20));
      check();
    });
    assert.deepStrictEqual(connection, [false, true]);
    // Nothing was missed, so nothing changed, and the session is still followed.
    assert.equal(session.seq, seq);
    assert.deepStrictEqual(plain(session), before);
    app.close();
  } finally {
    running.process.kill("SIGINT");
    await new Promise((done) => running.process.once("exit", done));
    rmSync(home, { recursive: true, force: true });
  }
});

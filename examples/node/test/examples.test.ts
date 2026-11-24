// Runs every example here against the real daemon program with a fake agent that replays
// a recorded session. Needs `cargo build` first.

import { BridgeClient, unixSocketTransport } from "@harness-bridge/client";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { chat } from "../chat.ts";
import { ensureDaemon } from "../daemon.ts";
import { startExtension } from "../extension.ts";

const root = path.join(import.meta.dirname, "../../..");
const bridged = path.join(root, "target/debug/bridged");
const fake = (scenario: string) => ["--fake", path.join(root, `testdata/transcripts/claude-v2/${scenario}.ndjson`), "--fake-speed", "0"];

test("the daemon is started once, and found the second time", { skip: !existsSync(bridged) }, async () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "bridge-ex-"));
  const first = await ensureDaemon({ home, command: bridged, args: fake("simple") });
  try {
    assert.ok(first.started, "nothing was running, so it was started");
    const second = await ensureDaemon({ home, command: "/no/such/program" });
    assert.equal(second.started, null, "one answers already, so nothing is started");
    assert.equal(second.socket, first.socket);
  } finally {
    first.stop();
    rmSync(home, { recursive: true, force: true });
  }
  await assert.rejects(ensureDaemon({ home: `${home}-none`, command: "/no/such/program", timeoutMs: 2000 }), /could not start the daemon/);
  rmSync(`${home}-none`, { recursive: true, force: true });
});

test("the chat prints a turn and answers its approval", { skip: !existsSync(bridged) }, async () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "bridge-ex-"));
  const daemon = await ensureDaemon({ home, command: bridged, args: fake("approval_command") });
  try {
    const lines: string[] = [];
    const session = await chat({
      socket: daemon.socket,
      cwd: root,
      message: "anything: the fake agent replays its recording",
      mode: "default",
      approve: "allow-once",
      print: (line) => lines.push(line),
    });
    assert.equal(session.turnState, "idle");
    assert.ok(lines.some((line) => line.startsWith("you: ")), lines.join("\n"));
    assert.ok(lines.includes("approval: printf 'approval fixture' > probe-write.txt -> allow-once"), lines.join("\n"));
    assert.ok(lines.some((line) => line.startsWith("tool ") && line.endsWith(": completed")), lines.join("\n"));
    assert.equal(lines.at(-1), "agent: done.");
  } finally {
    daemon.stop();
    rmSync(home, { recursive: true, force: true });
  }
});

test("the extension's item reaches an app, live and on replay", { skip: !existsSync(bridged) }, async () => {
  const home = mkdtempSync(path.join(os.tmpdir(), "bridge-ex-"));
  const daemon = await ensureDaemon({ home, command: bridged, args: fake("simple") });
  try {
    const extension = await startExtension(daemon.socket);
    const connect = (name: string) => BridgeClient.connect({ open: () => unixSocketTransport(daemon.socket), name, retryMs: 0 });
    const app = await connect("example-app");
    const session = await app.newSession({ cwd: root });

    const added = (await app.call("_x/notes/add", { sessionId: session.id, text: "remember this" })) as { noteId: string };
    const note = () => session.items().find(({ ref }) => ref.target === "x.notes.note")?.item as { text: string } | undefined;
    await session.until(() => note() !== undefined);
    assert.deepEqual([added.noteId, note()!.text], ["note-1", "remember this"]);

    const late = await (await connect("late-app")).session(session.id);
    assert.ok(late.items().some(({ ref }) => ref.target === "x.notes.note"), "it is in the log");

    await assert.rejects(app.call("_x/notes/remove", { sessionId: session.id }), /unknown method/);
    extension.close();
  } finally {
    daemon.stop();
    rmSync(home, { recursive: true, force: true });
  }
});

test("the launchd file is a well-formed property list", { skip: process.platform !== "darwin" }, () => {
  const plist = path.join(root, "examples/service/dev.harness-bridge.bridged.plist");
  assert.match(execFileSync("plutil", ["-lint", plist], { encoding: "utf8" }), /OK/);
});

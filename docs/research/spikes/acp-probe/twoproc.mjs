// Two Claude adapter processes on one session, taking turns. What does each know afterwards,
// what ends up in the transcript, and does Claude's registry of running sessions list them?
// node twoproc.mjs   (three short Haiku turns)
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const here = import.meta.dirname;
const cwd = path.join(here, "work");
function adapter(label) {
  const child = spawn("node", [path.join(here, "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js")], {
    cwd, env: { ...process.env, ANTHROPIC_MODEL: "haiku", CLAUDE_CODE_EXECUTABLE: "/Users/dev/.local/bin/claude" }, stdio: ["pipe", "pipe", "ignore"] });
  let buf = "", next = 1; const waiting = new Map(); const said = []; let idle = null;
  child.stdout.on("data", (d) => { buf += d; let i; while ((i = buf.indexOf("\n")) >= 0) { const line = buf.slice(0, i); buf = buf.slice(i + 1);
    let m; try { m = JSON.parse(line); } catch { continue; }
    if (m.id !== undefined && !m.method) waiting.get(m.id)?.(m);
    const u = m.params?.update;
    if (u?.sessionUpdate === "agent_message_chunk") said.push(u.content?.text ?? "");
    if (u?.sessionUpdate === "agent_message") said.push((u.content ?? []).map((c) => c.text ?? "").join(""));
    if (u?.sessionUpdate === "user_message") said.push(`\n[user] ${(u.content ?? []).map((c) => c.text ?? "").join("")}\n`);
    if (u?.sessionUpdate === "state_update" && u.state === "idle") idle?.();
  } });
  const call = (method, params) => new Promise((r) => { const id = next++; waiting.set(id, r); child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n"); });
  return { label, pid: child.pid, call, said, kill: () => child.kill(),
    async turn(sessionId, text) { said.length = 0; const done = new Promise((r) => (idle = r)); await call("session/prompt", { sessionId, prompt: [{ type: "text", text }] }); await done; return said.join("").trim(); } };
}
const init = { protocolVersion: 2, info: { name: "twoproc", version: "0" }, capabilities: {} };
const registry = () => fs.readdirSync(path.join(os.homedir(), ".claude/sessions")).filter((f) => f.endsWith(".json"))
  .map((f) => { const d = JSON.parse(fs.readFileSync(path.join(os.homedir(), ".claude/sessions", f), "utf8")); return `${d.pid} ${d.sessionId.slice(0, 8)} ${d.kind}/${d.entrypoint} ${d.status}`; });

const a = adapter("A"); await a.call("initialize", init);
const { result: created } = await a.call("session/new", { cwd, mcpServers: [] });
const sessionId = created.sessionId;
console.log("session", sessionId);
console.log("A turn 1:", await a.turn(sessionId, "Remember the codeword ALPHA. Reply with exactly: ok"));
console.log("registry after A's turn:", registry());

const b = adapter("B"); await b.call("initialize", init);
const resumed = await b.call("session/resume", { sessionId, cwd, mcpServers: [] });
console.log("B resumes while A holds it:", resumed.error ? `refused: ${JSON.stringify(resumed.error)}` : "accepted");
console.log("B turn:", await b.turn(sessionId, "Remember the codeword BRAVO as well. Reply with exactly: ok"));
console.log("A turn 2:", await a.turn(sessionId, "List every codeword I have told you so far, comma separated, nothing else."));
console.log("registry with both alive:", registry());
a.kill(); b.kill();

// The transcript: is it one line of messages, or did it fork?
const file = path.join(os.homedir(), ".claude/projects", cwd.replace(/[^a-zA-Z0-9]/g, "-"), `${sessionId}.jsonl`);
const nodes = fs.readFileSync(file, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)).filter((d) => d.uuid);
const children = new Map(); for (const n of nodes) children.set(n.parentUuid, (children.get(n.parentUuid) ?? 0) + 1);
console.log("transcript:", nodes.length, "messages;", [...children.values()].filter((n) => n > 1).length, "of them have more than one follow-up (forks)");
console.log("  kinds of writer:", [...new Set(nodes.map((n) => n.entrypoint))]);

// A third process reads the session back: which history does it get?
const c = adapter("C"); await c.call("initialize", init);
await c.call("session/resume", { sessionId, cwd, mcpServers: [], replayFrom: { type: "start" } });
await new Promise((r) => setTimeout(r, 500));
console.log("a fresh process replays:", c.said.join("").replace(/\n+/g, " | ").slice(0, 600));
c.kill();

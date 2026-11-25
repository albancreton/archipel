// What does session/fork do on the Claude adapter? No model call. node fork.mjs <sessionId>
import { spawn } from "node:child_process";
import path from "node:path";
const here = import.meta.dirname; const cwd = path.join(here, "work");
function adapter() {
  const child = spawn("node", [path.join(here, "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js")], { cwd, env: { ...process.env, ANTHROPIC_MODEL: "haiku", CLAUDE_CODE_EXECUTABLE: "/Users/dev/.local/bin/claude" }, stdio: ["pipe", "pipe", "ignore"] });
  let buf = "", next = 1; const waiting = new Map(); const updates = [];
  child.stdout.on("data", (d) => { buf += d; let i; while ((i = buf.indexOf("\n")) >= 0) { const line = buf.slice(0, i); buf = buf.slice(i + 1); let m; try { m = JSON.parse(line); } catch { continue; }
    if (m.id !== undefined && !m.method) waiting.get(m.id)?.(m); if (m.method === "session/update") updates.push(`${m.params.sessionId.slice(0, 8)}:${m.params.update.sessionUpdate}`); } });
  return { updates, kill: () => child.kill(), call: (method, params) => new Promise((r) => { const id = next++; waiting.set(id, r); child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n"); }) };
}
const init = { protocolVersion: 2, info: { name: "fork", version: "0" }, capabilities: {} };
const source = process.argv[2];
const a = adapter(); await a.call("initialize", init);
const forked = await a.call("session/fork", { sessionId: source, cwd, mcpServers: [] });
console.log("fork of a session this process never opened:", forked.error ? JSON.stringify(forked.error) : `new id ${forked.result.sessionId}, keys ${Object.keys(forked.result)}`);
console.log("updates sent by the fork:", a.updates);
const id = forked.result?.sessionId;
if (id) {
  a.updates.length = 0;
  const again = await a.call("session/resume", { sessionId: id, cwd, mcpServers: [], replayFrom: { type: "start" } });
  console.log("replay of the fork in the same process:", again.error ? JSON.stringify(again.error) : a.updates);
  const b = adapter(); await b.call("initialize", init);
  await b.call("session/resume", { sessionId: id, cwd, mcpServers: [], replayFrom: { type: "start" } });
  console.log("replay of the fork in a fresh process:", b.updates);
  const listed = await b.call("session/list", { cwd });
  console.log("listed:", listed.result.sessions.filter((s) => [id, source].includes(s.sessionId)).map((s) => `${s.sessionId.slice(0, 8)} "${s.title}"`));
  b.kill();
}
a.kill();

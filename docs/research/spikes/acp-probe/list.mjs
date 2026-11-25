// Asks the Claude adapter for its sessions over ACP v2 (no model call). node list.mjs [cwd]
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
const child = spawn("node", [path.join(here, "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js")], { env: { ...process.env, CLAUDE_CODE_EXECUTABLE: "/Users/dev/.local/bin/claude" }, stdio: ["pipe", "pipe", "ignore"] });
let buf = ""; const waiting = new Map(); let next = 1;
child.stdout.on("data", (d) => { buf += d; let i; while ((i = buf.indexOf("\n")) >= 0) { const line = buf.slice(0, i); buf = buf.slice(i + 1); try { const m = JSON.parse(line); if (m.id !== undefined && !m.method) waiting.get(m.id)?.(m); } catch {} } });
const call = (method, params) => new Promise((r) => { const id = next++; waiting.set(id, r); child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n"); });
await call("initialize", { protocolVersion: 2, info: { name: "probe", version: "0" }, capabilities: {} });
const t = Date.now();
const all = await call("session/list", {});
const here2 = await call("session/list", { cwd: process.argv[2] ?? path.join(here, "work") });
const s = (r) => r.error ? JSON.stringify(r.error) : `${r.result.sessions.length} sessions, nextCursor=${r.result.nextCursor ?? "none"}, keys=${Object.keys(r.result.sessions[0] ?? {})}`;
console.log("all:", s(all)); console.log("cwd:", s(here2), `${Date.now() - t} ms`);
const first = here2.result?.sessions?.[0]; if (first) console.log(JSON.stringify({ ...first, title: "(title hidden)" }));
child.kill();

// Minimal raw ACP client: node client.mjs <claude|pi> <scenario> [--air]
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
const [agent, scenarioName, ...flags] = process.argv.slice(2);
const here = import.meta.dirname;
const cwd = path.join(here, "work");
const SCEN = JSON.parse(fs.readFileSync(path.join(here, "scenarios.json"), "utf8"))[scenarioName];
const log = fs.createWriteStream(path.join(here, "logs", `${agent}-${scenarioName}${flags.includes("--air") ? "-air" : ""}.ndjson`));
const cmd = agent === "claude"
  ? ["node", [path.join(here, "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js")], { ANTHROPIC_MODEL: "haiku", CLAUDE_CODE_EXECUTABLE: "/Users/dev/.local/bin/claude" }]
  : ["node", [path.join(here, "node_modules/pi-acp/dist/index.js")], {}];
const child = spawn(cmd[0], cmd[1], { cwd, env: { ...process.env, ...cmd[2] }, stdio: ["pipe", "pipe", "pipe"] });
let stderr = ""; child.stderr.on("data", (d) => (stderr += d));
const T0 = Date.now(); const t = () => `+${String(Date.now() - T0).padStart(6)}ms`;
const clip = (v, n = 100) => { const s = typeof v === "string" ? v : JSON.stringify(v) ?? ""; return (s.length > n ? s.slice(0, n) + "…" : s).replace(/\n/g, "⏎"); };
let nextId = 1; const pending = new Map(); const counts = {}; const toolKinds = new Set(); const metaKeys = new Set();
const bump = (k) => (counts[k] = (counts[k] ?? 0) + 1);
const send = (m) => { log.write(JSON.stringify({ dir: "out", t: Date.now() - T0, m }) + "\n"); child.stdin.write(JSON.stringify(m) + "\n"); };
const request = (method, params) => new Promise((resolve, reject) => { const id = nextId++; pending.set(id, { resolve, reject, method }); send({ jsonrpc: "2.0", id, method, params }); });
const notify = (method, params) => send({ jsonrpc: "2.0", method, params });
let onTool = null; let chunkRun = null;
const flushChunks = () => { if (chunkRun) { console.log(`  ${chunkRun.t0} ${chunkRun.kind} ×${chunkRun.n} "${clip(chunkRun.text, 90)}"`); chunkRun = null; } };
function collectMeta(o, depth = 0) { if (!o || typeof o !== "object" || depth > 6) return; if (o._meta) for (const k of Object.keys(o._meta)) metaKeys.add(k + (typeof o._meta[k] === "object" && o._meta[k] ? ":" + Object.keys(o._meta[k]).join("|") : "")); for (const v of Object.values(o)) collectMeta(v, depth + 1); }
function handle(m) {
  log.write(JSON.stringify({ dir: "in", t: Date.now() - T0, m }) + "\n");
  collectMeta(m.params);
  if (m.id !== undefined && m.method === undefined) { const p = pending.get(m.id); pending.delete(m.id); if (m.error) p.reject(Object.assign(new Error(`${p.method}: ${JSON.stringify(m.error)}`), { rpc: m.error })); else p.resolve(m.result); return; }
  if (m.method === "session/update") {
    const u = m.params.update; const k = u.sessionUpdate; bump(k);
    const child = m.params.sessionId !== mainSession ? " [child session]" : "";
    if (/_chunk$/.test(k)) { const text = u.content?.text ?? ""; if (chunkRun && chunkRun.kind === k + child) { chunkRun.n++; chunkRun.text += text; } else { flushChunks(); chunkRun = { kind: k + child, n: 1, text, t0: t() }; } return; }
    flushChunks();
    if (k === "tool_call" || k === "tool_call_update") {
      if (u.kind) toolKinds.add(u.kind);
      const content = (u.content ?? []).map((c) => c.type === "content" ? `content:${c.content?.type}` : c.type).join(",");
      console.log(`  ${t()} ${k}${child} ${u.toolCallId?.slice(-6)} kind=${u.kind ?? "-"} status=${u.status ?? "-"} title="${clip(u.title ?? "", 60)}" content=[${content}] meta=${clip(u._meta ? Object.keys(u._meta) : "", 60)}`);
      if (k === "tool_call" && onTool) { const f = onTool; onTool = null; f(u); }
    } else if (k === "plan" || k === "plan_update") console.log(`  ${t()} ${k}${child} ${clip(u.entries?.map((e) => `[${e.status}] ${e.content}`) ?? u, 200)}`);
    else if (!["usage_update","available_commands_update","config_option_update","session_info_update"].includes(k)) console.log(`  ${t()} ${k}${child} ${clip({ ...u, sessionUpdate: undefined }, 260)}`);
    return;
  }
  flushChunks(); bump("REQ " + m.method);
  if (m.method === "session/request_permission") {
    const opts = m.params.options ?? []; const pick = opts.find((o) => o.kind === (SCEN.deny ? "reject_once" : "allow_once")) ?? opts[0];
    console.log(`  ${t()} PERMISSION tool="${clip(m.params.toolCall?.title, 60)}" kind=${m.params.toolCall?.kind} options=${opts.map((o) => o.kind + ":" + o.name).join(" | ")} -> ${pick?.name}`);
    return send({ jsonrpc: "2.0", id: m.id, result: { outcome: { outcome: "selected", optionId: pick.optionId } } });
  }
  if (/elicitation/.test(m.method)) {
    console.log(`  ${t()} ELICITATION ${clip(m.params, 700)}`);
    const props = m.params.requestedSchema?.properties ?? {}; const content = {};
    for (const [key, s] of Object.entries(props)) { const opt = s.enum?.[0] ?? s.oneOf?.[0]?.const ?? s.items?.enum?.[0] ?? s.items?.anyOf?.[0]?.const ?? s.anyOf?.[0]?.const; content[key] = s.type === "array" ? [opt] : opt ?? "ok"; }
    console.log(`  ${t()}   -> accept ${clip(content, 200)}`);
    return send({ jsonrpc: "2.0", id: m.id, result: { action: "accept", content } });
  }
  if (/auth\/status_update/.test(m.method)) return; console.log(`  ${t()} UNHANDLED ${m.method} ${clip(m.params, 300)}`);
  if (m.id !== undefined) send({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "not supported by probe" } });
}
let buf = ""; child.stdout.on("data", (d) => { buf += d; let i; while ((i = buf.indexOf("\n")) >= 0) { const line = buf.slice(0, i); buf = buf.slice(i + 1); if (line.trim()) { try { handle(JSON.parse(line)); } catch (e) { console.log("  !! bad line", clip(line, 200), e.message); } } } });
let mainSession = null;
const air = flags.includes("--air") ? { _meta: { jetbrains: { air: { version: 1, capabilities: ["asyncTasks", "backgroundSubagents", "nativeSubagentSessions"] } } } } : {};
const timeout = setTimeout(() => { console.log("TIMEOUT"); finish(1); }, (SCEN.timeout ?? 150) * 1000);
function finish(code) { flushChunks(); console.log(`  update counts: ${JSON.stringify(counts)}`); console.log(`  tool kinds: ${[...toolKinds].join(", ")}`); if (metaKeys.size) console.log(`  _meta keys: ${clip([...metaKeys].join("; "), 500)}`);  child.kill(); log.end(() => process.exit(code)); clearTimeout(timeout); }
try {
  const init = await request("initialize", { protocolVersion: 1, clientInfo: { name: "acp-probe", version: "0.0.1" },
    clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false, plan: {}, subagents: {}, elicitation: { form: {}, url: {} }, session: { notices: {}, compaction: {}, configOptions: { boolean: {} }, promptSuggestions: {} }, ...air } });
  console.log(`== ${agent} / ${scenarioName}: agent=${clip(init.agentInfo)} protocol=${init.protocolVersion}`);
  if (flags.includes("--caps")) console.log(JSON.stringify(init, null, 1));
  const sess = await request("session/new", { cwd, mcpServers: [] });
  mainSession = sess.sessionId;
  console.log(`  session: modes=${clip(sess.modes?.availableModes?.map((m) => m.id))} current=${sess.modes?.currentModeId} configOptions=${clip(sess.configOptions?.map((o) => `${o.id}=${o.currentValue}`), 300)} models=${clip(sess.models?.currentModelId)}`);
  if (flags.includes("--caps")) { console.log(JSON.stringify(sess, null, 1).slice(0, 6000)); }
  if (process.env.PROBE_MODEL) { try { const r = await request("session/set_config_option", { sessionId: mainSession, configId: "model", value: process.env.PROBE_MODEL }); console.log(`  model set: ${clip(r.configOptions?.find((o) => o.id === "model")?.currentValue)}`); } catch (e) { console.log("  set model failed:", e.message); } }
  if (SCEN.mode && agent === "claude") { try { await request("session/set_mode", { sessionId: mainSession, modeId: SCEN.mode }); console.log(`  mode set: ${SCEN.mode}`); } catch (e) { console.log("  set_mode failed:", e.message); } }
  for (const text of SCEN.prompts) {
    console.log(`\n> user: ${clip(text, 150)}`);
    const p = request("session/prompt", { sessionId: mainSession, prompt: [{ type: "text", text }] });
    if (SCEN.cancelOnTool) onTool = () => setTimeout(() => { console.log(`  ${t()} > client: session/cancel`); notify("session/cancel", { sessionId: mainSession }); }, 1500);
    if (SCEN.steer) setTimeout(() => { console.log(`  ${t()} > client: second session/prompt while busy`); request("session/prompt", { sessionId: mainSession, prompt: [{ type: "text", text: SCEN.steer }] }).then((r) => console.log(`  ${t()} second prompt result ${clip(r)}`), (e) => console.log(`  ${t()} second prompt rejected: ${clip(e.message, 200)}`)); }, 4000);
    const r = await p; flushChunks();
    console.log(`  ${t()} PROMPT RESULT ${clip(r, 300)}`);
  }
  if (SCEN.lingerMs) { console.log(`  (waiting ${SCEN.lingerMs}ms for post-turn updates)`); await new Promise((r) => setTimeout(r, SCEN.lingerMs)); }
  finish(0);
} catch (e) { console.log("FAILED:", clip(e.message, 600)); finish(1); }

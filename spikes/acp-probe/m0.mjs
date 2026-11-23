// Milestone 0 probes: node m0.mjs <test> <claude|pi> [args]
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
const here = import.meta.dirname;
const cwd = path.join(here, "work");
const [test, agent = "claude", ...args] = process.argv.slice(2);
const T0 = Date.now();
const ts = () => `+${String(Date.now() - T0).padStart(6)}ms`;
export const clip = (v, n = 110) => { const s = typeof v === "string" ? v : JSON.stringify(v) ?? ""; return (s.length > n ? s.slice(0, n) + "…" : s).replace(/\n/g, "⏎"); };
const redact = (s) => s.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]{2,}/g, "user@example.com");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const AIR = { _meta: { jetbrains: { air: { version: 1, capabilities: ["asyncTasks", "backgroundSubagents", "nativeSubagentSessions"] } } } };
const V1_CAPS = { fs: { readTextFile: false, writeTextFile: false }, terminal: false, plan: {}, subagents: {}, elicitation: { form: {}, url: {} }, session: { notices: {}, compaction: {}, configOptions: { boolean: {} }, promptSuggestions: {} } };

function connect(label, opts = {}) {
  const cmd = agent === "claude"
    ? [path.join(here, "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"), { ANTHROPIC_MODEL: opts.model ?? "haiku", CLAUDE_CODE_EXECUTABLE: "/Users/dev/.local/bin/claude" }]
    : [path.join(here, "node_modules/pi-acp/dist/index.js"), {}];
  const child = spawn("node", [cmd[0]], { cwd, env: { ...process.env, ...cmd[1], ...(opts.env ?? {}) }, stdio: ["pipe", "pipe", "pipe"] });
  const log = fs.createWriteStream(path.join(here, "logs", `m0-${agent}-${label}.ndjson`));
  let nextId = 1; const pending = new Map(); const c = { updates: [], requests: [], child, permissionDelayMs: 0, quiet: opts.quiet ?? false, onUpdate: null, exited: false };
  child.on("exit", () => (c.exited = true));
  const send = (m) => { log.write(redact(JSON.stringify({ dir: "out", t: Date.now() - T0, m })) + "\n"); child.stdin.write(JSON.stringify(m) + "\n"); };
  c.request = (method, params) => new Promise((resolve, reject) => { const id = nextId++; pending.set(id, { resolve, reject, method }); send({ jsonrpc: "2.0", id, method, params }); });
  c.notify = (method, params) => send({ jsonrpc: "2.0", method, params });
  c.respond = (id, result) => send({ jsonrpc: "2.0", id, result });
  const handle = async (m) => {
    log.write(redact(JSON.stringify({ dir: "in", t: Date.now() - T0, m })) + "\n");
    if (m.id !== undefined && m.method === undefined) { const p = pending.get(m.id); pending.delete(m.id); if (!p) return; return m.error ? p.reject(new Error(`${p.method}: ${JSON.stringify(m.error)}`)) : p.resolve(m.result); }
    if (m.method === "session/update") { const u = { t: Date.now() - T0, sessionId: m.params.sessionId, ...m.params.update }; c.updates.push(u); c.onUpdate?.(u, m.params); return; }
    if (/auth\/status_update/.test(m.method)) return;
    c.requests.push({ t: Date.now() - T0, method: m.method, params: m.params });
    if (opts.onRequest && (await opts.onRequest(m, c)) === true) return;
    if (m.method === "session/request_permission") {
      const o = m.params.options ?? []; const pick = o.find((x) => x.kind === "allow_once") ?? o[0];
      if (!c.quiet) console.log(`  ${ts()} PERMISSION "${clip(m.params.toolCall?.title, 50)}" (answering in ${c.permissionDelayMs}ms)`);
      if (c.permissionDelayMs) await sleep(c.permissionDelayMs);
      if (!c.quiet) console.log(`  ${ts()} -> ${pick?.name}`);
      return c.respond(m.id, { outcome: { outcome: "selected", optionId: pick.optionId } });
    }
    if (!c.quiet) console.log(`  ${ts()} REQUEST ${m.method} ${clip(m.params, 200)}`);
    if (m.id !== undefined) send({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "not supported by probe" } });
  };
  let buf = ""; child.stdout.on("data", (d) => { buf += d; let i; while ((i = buf.indexOf("\n")) >= 0) { const line = buf.slice(0, i); buf = buf.slice(i + 1); if (line.trim()) { try { handle(JSON.parse(line)); } catch (e) { console.log("  !! bad line", clip(line, 160)); } } } });
  c.stderr = ""; child.stderr.on("data", (d) => (c.stderr += d));
  c.close = () => new Promise((r) => { child.kill(); log.end(r); });
  c.init = (extraCaps = {}, protocolVersion = 1) => c.request("initialize", { protocolVersion, clientInfo: { name: "m0-probe", version: "0.0.1" }, clientCapabilities: { ...V1_CAPS, ...extraCaps } });
  c.newSession = async (meta) => {
    const s = await c.request("session/new", { cwd, mcpServers: [], ...(meta ? { _meta: meta } : {}) });
    if (agent === "pi" && process.env.PROBE_MODEL) await c.request("session/set_config_option", { sessionId: s.sessionId, configId: "model", value: process.env.PROBE_MODEL });
    return s;
  };
  c.prompt = (sessionId, text) => c.request("session/prompt", { sessionId, prompt: [{ type: "text", text }] });
  return c;
}
// Fold updates into a comparable summary: ordered list of items.
function summarise(updates, sessionId) {
  const items = []; let last = null; const tools = new Map(); const kinds = {};
  for (const u of updates) {
    kinds[u.sessionUpdate] = (kinds[u.sessionUpdate] ?? 0) + 1;
    const own = sessionId === undefined || u.sessionId === sessionId ? "" : "child:";
    const k = u.sessionUpdate;
    if (/_chunk$/.test(k)) { const kind = own + k.replace(/_chunk$/, ""); if (last && last.kind === kind) last.text += u.content?.text ?? ""; else items.push((last = { kind, text: u.content?.text ?? "" })); continue; }
    if (k === "tool_call") { const it = { kind: own + "tool", id: u.toolCallId, toolKind: u.kind, title: u.title, status: u.status }; tools.set(u.toolCallId, it); items.push(it); last = null; continue; }
    if (k === "tool_call_update") { const it = tools.get(u.toolCallId); if (it) { if (u.status) it.status = u.status; if (u.title) it.title = u.title; if (u.kind) it.toolKind = u.kind; } else items.push({ kind: own + "tool(update only)", id: u.toolCallId, status: u.status, title: u.title }); last = null; continue; }
    if (["usage_update", "available_commands_update", "config_option_update", "session_info_update", "current_mode_update", "prompt_suggestions_update"].includes(k)) continue;
    items.push({ kind: own + k, detail: clip({ ...u, t: undefined, sessionId: undefined, sessionUpdate: undefined }, 120) }); last = null;
  }
  return { items, kinds };
}
const show = (s) => s.items.map((i) => `    - ${i.kind}${i.toolKind ? `(${i.toolKind},${i.status})` : ""} ${clip(i.text ?? i.title ?? i.detail ?? "", 90)}`).join("\n");

const tests = {
  // 1. Several sessions in one adapter process, prompted concurrently.
  async multi() {
    const c = connect("multi", { quiet: true }); await c.init();
    const a = await c.newSession(), b = await c.newSession();
    console.log(`  two sessions in one process: ${a.sessionId !== b.sessionId}`);
    const t0 = Date.now();
    const pa = () => c.prompt(a.sessionId, "Count from 1 to 40, one number per line, nothing else."), pb = () => c.prompt(b.sessionId, "List the letters A to Z, one per line, nothing else.");
    const [ra, rb] = args.includes("seq") ? [await pa(), await pb()] : await Promise.all([pa(), pb()]);
    const span = (id) => { const u = c.updates.filter((x) => x.sessionId === id && /message_chunk/.test(x.sessionUpdate)); return [u[0]?.t, u.at(-1)?.t, u.length, summarise(u).items[0]?.text]; };
    const [a0, a1, an, at] = span(a.sessionId), [b0, b1, bn, bt] = span(b.sessionId);
    console.log(`  A: chunks=${an} ${a0}..${a1}ms stop=${ra.stopReason} text="${clip(at, 50)}"`);
    console.log(`  B: chunks=${bn} ${b0}..${b1}ms stop=${rb.stopReason} text="${clip(bt, 50)}"`);
    console.log(`  overlapped in time: ${a0 < b1 && b0 < a1}; total ${Date.now() - t0}ms; stray session ids: ${new Set(c.updates.map((u) => u.sessionId)).size - 2}`);
    await c.close();
  },
  // 2. session/load and session/resume after a process restart.
  async load() {
    let c = connect("load-1", { quiet: true }); const init = await c.init();
    console.log(`  caps: loadSession=${init.agentCapabilities?.loadSession} session=${clip(Object.keys(init.agentCapabilities?.sessionCapabilities ?? {}))}`);
    const s = await c.newSession();
    if (agent === "claude") await c.request("session/set_mode", { sessionId: s.sessionId, modeId: "bypassPermissions" }).catch((e) => console.log("  set_mode:", e.message));
    await c.prompt(s.sessionId, "Remember the codeword PAPAYA. Then read package.json and tell me its name field in one short sentence.");
    await c.prompt(s.sessionId, agent === "claude"
      ? "Spawn one subagent whose only task is to read tsconfig.json and reply with whether strict is true. Do not run it in the background: wait for its result, then reply with exactly what it said."
      : "Spawn one subagent whose only task is to read tsconfig.json and reply with whether strict is true. Wait for it, then reply with exactly what it said.");
    await sleep(6000);
    const live = summarise(c.updates, s.sessionId); console.log(`  LIVE (${c.updates.length} updates):\n${show(live)}`);
    await c.close();
    c = connect("load-2", { quiet: true }); await c.init();
    const t = Date.now();
    try {
      const r = await c.request("session/load", { sessionId: s.sessionId, cwd, mcpServers: [] }); await sleep(1500);
      const rep = summarise(c.updates, s.sessionId);
      console.log(`  LOADED in ${Date.now() - t}ms (${c.updates.length} updates) result keys=${clip(Object.keys(r ?? {}))}:\n${show(rep)}`);
      const norm = (x) => x.items.map((i) => `${i.kind}|${i.toolKind ?? ""}|${(i.text ?? i.title ?? "").trim()}`);
      const L = norm(live), R = norm(rep);
      console.log(`  identical item lists: ${JSON.stringify(L) === JSON.stringify(R)}; live=${L.length} items, replay=${R.length}; only-live kinds=${clip([...new Set(live.items.filter((_, i) => !R.includes(L[i])).map((i) => i.kind))])} only-replay kinds=${clip([...new Set(rep.items.filter((_, i) => !L.includes(R[i])).map((i) => i.kind))])}`);
      const n = c.updates.length; await c.prompt(s.sessionId, "What was the codeword? One word.");
      console.log(`  after load, recall: "${clip(summarise(c.updates.slice(n)).items.filter((i) => i.kind === "agent_message").map((i) => i.text).join(""), 40)}"`);
    } catch (e) { console.log("  load failed:", clip(e.message, 300)); }
    await c.close();
    if (agent !== "claude") return;
    c = connect("load-3-resume", { quiet: true }); await c.init();
    try {
      await c.request("session/resume", { sessionId: s.sessionId, cwd, mcpServers: [] }); await sleep(1000);
      const replayed = summarise(c.updates, s.sessionId).items.length; const n = c.updates.length;
      await c.prompt(s.sessionId, "What was the codeword? One word.");
      console.log(`  session/resume: replayed items=${replayed}; recall: "${clip(summarise(c.updates.slice(n)).items.filter((i) => i.kind === "agent_message").map((i) => i.text).join(""), 40)}"`);
    } catch (e) { console.log("  resume failed:", clip(e.message, 300)); }
    await c.close();
  },
  // 3. How long an unanswered permission request stays valid.
  async permwait() {
    const wait = Number(args[0] ?? 150) * 1000;
    const c = connect("permwait"); await c.init(); const s = await c.newSession();
    await c.request("session/set_mode", { sessionId: s.sessionId, modeId: "default" });
    c.permissionDelayMs = wait;
    const r = await c.prompt(s.sessionId, "Run this exact shell command: printf 'waited' > perm-wait.txt  Then reply with exactly: done.");
    const sum = summarise(c.updates, s.sessionId);
    console.log(`  after waiting ${wait / 1000}s: stop=${r.stopReason}\n${show(sum)}\n  file written: ${fs.existsSync(path.join(cwd, "perm-wait.txt"))}`);
    await c.close();
  },
  // 4. Background work with the JetBrains AIR capabilities declared.
  async bg() {
    const c = connect("bg-air", { quiet: true }); await c.init(args.includes("noair") ? {} : AIR); const s = await c.newSession();
    await c.request("session/set_mode", { sessionId: s.sessionId, modeId: "bypassPermissions" });
    const r = await c.prompt(s.sessionId, "Start this command as a background task (run_in_background): node -e \"setTimeout(() => console.log('background-done'), 8000)\"  As soon as it is launched, reply exactly: launched  and end your turn without waiting.");
    console.log(`  prompt returned at ${ts()} stop=${r.stopReason}`); await sleep(16000);
    for (const u of c.updates) if (/async_task|background|subagent/.test(u.sessionUpdate)) console.log(`  +${u.t}ms ${u.sessionUpdate} ${clip({ ...u, t: undefined, sessionId: undefined, sessionUpdate: undefined }, 330)}`);
    console.log(`  kinds: ${JSON.stringify(summarise(c.updates).kinds)}\n${show(summarise(c.updates, s.sessionId))}`);
    await c.close();
  },
  // 5. What makes Claude emit thought chunks.
  async think() {
    const variants = { envDisplay: { env: { CLAUDE_CODE_THINKING_DISPLAY: "summarized" } }, metaThinking: { meta: { claudeCode: { options: { thinking: { type: "adaptive", display: "summarized" } } } } }, baseline: {}, effortHigh: { effort: "high" }, showSummaries: { meta: { claudeCode: { options: { settings: { showThinkingSummaries: true } } } } }, both: { effort: "high", meta: { claudeCode: { options: { settings: { showThinkingSummaries: true } } } } } };
    for (const [name, v] of Object.entries(variants)) {
      if (args.length && !args.includes(name)) continue;
      const c = connect(`think-${name}`, { quiet: true, env: v.env }); await c.init(); const s = await c.newSession(v.meta);
      if (v.effort) await c.request("session/set_config_option", { sessionId: s.sessionId, configId: "effort", value: v.effort }).catch((e) => console.log(`  ${name}: set effort failed ${clip(e.message, 120)}`));
      await c.prompt(s.sessionId, "Think carefully step by step about whether 391 is prime, then answer in one short sentence.");
      const k = summarise(c.updates).kinds; const th = summarise(c.updates).items.find((i) => i.kind === "agent_thought");
      console.log(`  ${name}: thought chunks=${k.agent_thought_chunk ?? 0} message chunks=${k.agent_message_chunk ?? 0} thought="${clip(th?.text ?? "", 80)}"`);
      await c.close();
    }
  },
  // 6a. Claude todo list.
  async todo() {
    const c = connect("todo", { quiet: true, model: args[0] ?? "sonnet" }); await c.init(); const s = await c.newSession();
    await c.request("session/set_mode", { sessionId: s.sessionId, modeId: "bypassPermissions" });
    await c.prompt(s.sessionId, "Use your todo or task tracking tool to create three items: alpha, beta, gamma. Then mark alpha completed. If you truly have no such tool, say so and list the tool names you do have. Reply with exactly: done.");
    for (const u of c.updates) if (/plan/.test(u.sessionUpdate)) console.log(`  +${u.t}ms ${u.sessionUpdate} ${clip({ ...u, t: undefined, sessionId: undefined, sessionUpdate: undefined }, 300)}`);
    console.log(show(summarise(c.updates, s.sessionId)));
    await c.close();
  },
  // 6b. Pi compaction on a session big enough to compact.
  async picompact() {
    const c = connect("compact", { quiet: true }); await c.init(); const s = await c.newSession();
    const big = path.join(here, "node_modules/pi-acp/dist/index.js");
    await c.prompt(s.sessionId, `Remember the codeword PAPAYA. Then read the first 900 lines of ${big} and summarise it in two sentences.`);
    await c.prompt(s.sessionId, `Now read lines 900 to 1800 of the same file and summarise in two sentences.`);
    const n = c.updates.length;
    try { const r = await c.prompt(s.sessionId, "/compact"); console.log(`  /compact result ${clip(r)}`); } catch (e) { console.log("  /compact failed:", clip(e.message, 240)); }
    console.log(`  updates during compact:\n${show(summarise(c.updates.slice(n), s.sessionId))}\n  kinds: ${JSON.stringify(summarise(c.updates.slice(n)).kinds)}`);
    const m = c.updates.length; await c.prompt(s.sessionId, "What was the codeword? One word.");
    console.log(`  recall: "${clip(summarise(c.updates.slice(m)).items.filter((i) => i.kind === "agent_message").map((i) => i.text).join(""), 40)}"`);
    await c.close();
  },
  // 7. ACP v2 (preview) against the Claude adapter.
  async v2() {
    const printer = (c) => { let run = null; const flush = () => { if (run) { console.log(`  ${run.t0} ${run.k} ×${run.n} ${clip(run.ids.size ? [...run.ids] : "", 60)} "${clip(run.text, 70)}"`); run = null; } };
      c.onUpdate = (u) => { const k = u.sessionUpdate; if (["usage_update", "available_commands_update", "config_option_update", "session_info_update"].includes(k)) return;
        if (/_chunk$/.test(k)) { const text = u.content?.text ?? u.data ?? ""; const id = u.messageId ?? u.toolCallId ?? u.terminalId; if (run && run.k === k) { run.n++; run.text += text; if (id) run.ids.add(id); } else { flush(); run = { k, n: 1, text, t0: ts(), ids: new Set(id ? [id] : []) }; } return; }
        flush(); console.log(`  ${ts()} ${k} ${clip({ ...u, t: undefined, sessionId: undefined, sessionUpdate: undefined }, 300)}`); };
      return flush; };
    const onRequest = async (m, c) => { if (m.method !== "session/request_permission") return false; console.log(`  ${ts()} PERMISSION(v2) ${clip(m.params, 600)}`); const o = m.params.options ?? []; const pick = o.find((x) => x.kind === "allow_once") ?? o[0]; c.respond(m.id, { outcome: { outcome: "selected", optionId: pick?.optionId } }); return true; };
    let c = connect("v2-1", { quiet: true, onRequest, env: { CLAUDE_CODE_THINKING_DISPLAY: "summarized" } }); let flush = printer(c);
    const init = await c.request("initialize", { protocolVersion: 2, info: { name: "m0-probe", version: "0.0.1" }, capabilities: { elicitation: { form: {}, url: {} }, ...(args.includes("air") ? AIR : {}) } });
    console.log(`  initialize -> ${clip(init, 900)}`);
    const s = await c.request("session/new", { cwd, mcpServers: [] });
    console.log(`  session/new -> keys=${clip(Object.keys(s))} config=${clip(s.configOptions?.map((o) => `${o.id}=${o.currentValue}`), 200)}`);
    await c.request("session/set_config_option", { sessionId: s.sessionId, configId: "mode", value: "default" }).catch((e) => console.log("  set mode failed:", clip(e.message, 200)));
    const waitIdle = () => new Promise((r) => { const iv = setInterval(() => { const st = c.updates.filter((u) => u.sessionUpdate === "state_update").at(-1); if (st && JSON.stringify(st).includes("idle") && c.updates.at(-1).t < Date.now() - T0 - 1500) { clearInterval(iv); r(); } }, 300); });
    console.log("\n> prompt 1 (streamed command)");
    const r1 = await c.request("session/prompt", { sessionId: s.sessionId, prompt: [{ type: "text", text: "Run this exact shell command in the foreground: for i in 1 2 3 4 5 6; do echo stream line $i; sleep 1; done  Then reply with exactly: done." }] });
    console.log(`  ${ts()} prompt returned immediately: ${clip(r1)}`);
    await sleep(3500); console.log("\n> prompt 2 while busy");
    try { const r2 = await c.request("session/prompt", { sessionId: s.sessionId, prompt: [{ type: "text", text: "Also mention the word STEERED in your final reply." }] }); console.log(`  ${ts()} second prompt accepted: ${clip(r2)}`); } catch (e) { console.log(`  ${ts()} second prompt rejected: ${clip(e.message, 200)}`); }
    await waitIdle(); flush();
    console.log("\n> prompt 3 (background task)");
    await c.request("session/prompt", { sessionId: s.sessionId, prompt: [{ type: "text", text: "Start this command as a background task (run_in_background): node -e \"setTimeout(() => console.log('background-done'), 6000)\"  As soon as it is launched, reply exactly: launched  and end your turn without waiting." }] });
    await sleep(4000); await waitIdle(); await sleep(9000); await waitIdle(); flush();
    console.log(`  kinds: ${JSON.stringify(summarise(c.updates).kinds)}`);
    await c.close();
    console.log("\n> new process, session/resume (replay)");
    c = connect("v2-2-resume", { quiet: true, onRequest }); flush = printer(c);
    await c.request("initialize", { protocolVersion: 2, info: { name: "m0-probe", version: "0.0.1" }, capabilities: { elicitation: { form: {}, url: {} } } });
    try { const r = await c.request("session/resume", { sessionId: s.sessionId, cwd, mcpServers: [] }); await sleep(2500); flush(); console.log(`  resume -> keys=${clip(Object.keys(r ?? {}))}; replayed kinds: ${JSON.stringify(summarise(c.updates).kinds)}`); } catch (e) { console.log("  resume failed:", clip(e.message, 300)); }
    await c.close();
  },
  // 7b. v2: replay on resume, then an approval in default mode.
  async v2b() {
    const sid = args[0];
    const onRequest = async (m, c) => { if (m.method !== "session/request_permission") return false; console.log(`  ${ts()} PERMISSION(v2) title="${clip(m.params.title, 60)}" subject=${clip(m.params.subject, 160)} options=${clip(m.params.options?.map((o) => o.kind + ":" + o.name), 200)}`); await sleep(2500); const pick = m.params.options.find((x) => x.kind === "allow_once") ?? m.params.options[0]; c.respond(m.id, { outcome: { outcome: "selected", optionId: pick.optionId } }); return true; };
    const c = connect("v2-3-replay", { quiet: true, onRequest });
    await c.request("initialize", { protocolVersion: 2, info: { name: "m0-probe", version: "0.0.1" }, capabilities: { elicitation: { form: {}, url: {} } } });
    await c.request("session/resume", { sessionId: sid, cwd, mcpServers: [], replayFrom: { type: "start" } }); await sleep(2000);
    const sum = summarise(c.updates);
    console.log(`  replayed ${c.updates.length} updates; kinds=${JSON.stringify(sum.kinds)}`);
    for (const u of c.updates.filter((u) => !/usage|commands|config|session_info|terminal_output_chunk/.test(u.sessionUpdate)).slice(0, 14)) console.log(`    ${u.sessionUpdate} ${clip({ ...u, t: undefined, sessionId: undefined, sessionUpdate: undefined }, 150)}`);
    await c.request("session/set_config_option", { sessionId: sid, configId: "mode", type: "id", value: "default" }).then(() => console.log("  mode=default"), (e) => console.log("  set mode failed:", clip(e.message, 200)));
    const n = c.updates.length;
    await c.request("session/prompt", { sessionId: sid, prompt: [{ type: "text", text: "Run this exact shell command: printf 'v2' > v2-probe.txt  Then reply with exactly: done." }] });
    await sleep(9000);
    for (const u of c.updates.slice(n).filter((u) => u.sessionUpdate === "state_update")) console.log(`    +${u.t}ms state_update ${clip({ state: u.state, background: u.background, stopReason: u.stopReason, pending: u.pending ?? u.requests }, 200)} keys=${Object.keys(u).join(",")}`);
    await c.close();
  },
};
if (!tests[test]) { console.log("tests:", Object.keys(tests).join(", ")); process.exit(1); }
console.log(`== m0 ${test} / ${agent}`);
const guard = setTimeout(() => { console.log("TIMEOUT"); process.exit(2); }, 420000);
try { await tests[test](); } catch (e) { console.log("FAILED:", clip(e.message, 500)); }
clearTimeout(guard); process.exit(0);

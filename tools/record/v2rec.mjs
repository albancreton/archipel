// Records ACP v2 transcripts from the Claude adapter into testdata/transcripts/claude-v2.
// node v2rec.mjs <scenario|all>
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
const here = import.meta.dirname;
const cwd = path.join(here, "work");
const outDir = path.join(here, "../../testdata/transcripts/claude-v2");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const clip = (v, n = 100) => { const s = typeof v === "string" ? v : JSON.stringify(v) ?? ""; return (s.length > n ? s.slice(0, n) + "…" : s).replace(/\n/g, "⏎"); };
const redact = (s) => s.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]{2,}/g, "user@example.com");
const NODE_SLEEP = (label, ms) => `node -e "setTimeout(() => console.log('${label}'), ${ms})"`;
const SCENARIOS = {
  simple: { prompts: ["Respond with the following text: fixture simple ok"] },
  thinking: { prompts: ["Think carefully step by step about whether 391 is prime, then answer in one short sentence."] },
  approval_command: { mode: "default", prompts: ["Run this exact shell command: printf 'approval fixture' > probe-write.txt  Then reply with exactly: done."] },
  approval_denied: { mode: "default", deny: true, prompts: ["Run this exact shell command: printf 'denied fixture' > probe-denied.txt  Then reply with exactly one word saying whether it ran: yes or no."] },
  files: { mode: "default", prompts: ["Read package.json, then search this directory for the word strict, then use the Edit tool to change the version in package.json to 1.0.1. Then reply with exactly: done."] },
  question: { mode: "default", prompts: ["Use the AskUserQuestion tool to ask me one question: which colour I prefer, with options Red and Blue. Then reply with exactly the option I chose."] },
  subagent: { mode: "bypassPermissions", prompts: ["Spawn one subagent whose only task is to read package.json and reply with its name field. Do not run it in the background: wait for its result, then reply with exactly what it said."] },
  subagents_parallel: { mode: "bypassPermissions", prompts: ["Spawn two subagents in parallel: one reads package.json and replies with its name field, the other reads tsconfig.json and replies whether strict is true. Wait for both, then reply with one line per subagent."] },
  plan: { mode: "plan", prompts: ["Plan how to add a NOTES.md with a one-line description to this directory. Keep the plan to three bullet points and present it for approval."] },
  web_search: { mode: "bypassPermissions", prompts: ["Search the web for the current stable Node.js LTS version, then answer in one short sentence."] },
  interrupt: { mode: "bypassPermissions", cancelAfterToolMs: 2500, prompts: [`Run this exact local command and wait for it: ${NODE_SLEEP("late", 30000)}  Do not answer until it completes.`] },
  background: { mode: "bypassPermissions", lingerMs: 12000, prompts: [`Start this command as a background task (run_in_background): ${NODE_SLEEP("background-done", 6000)}  As soon as it is launched, reply exactly: launched  and end your turn without waiting.`] },
  steer: { mode: "bypassPermissions", steerAfterMs: 4000, steer: "Also mention the word STEERED in your final reply.", prompts: [`Run this exact command: ${NODE_SLEEP("A", 9000)}  then reply DONE_A.`] },
  compact: { prompts: ["Remember the codeword PAPAYA. Reply OK.", "/compact", "What was the codeword? One word."] },
  command_output: { mode: "bypassPermissions", prompts: ["Run this exact shell command in the foreground: for i in 1 2 3 4; do echo stream line $i; sleep 1; done  Then reply with exactly: done."] },
  multi_turn: { prompts: ["Respond with exactly: first turn complete", "Respond with exactly: second turn complete"] },
  failing_command: { mode: "bypassPermissions", prompts: ["Run this exact shell command: ls /definitely-not-a-real-path-xyz  Then reply with one short sentence saying what happened."] },
};
async function record(name) {
  const SC = SCENARIOS[name]; const T0 = Date.now(); const ts = () => `+${String(Date.now() - T0).padStart(6)}ms`;
  fs.writeFileSync(path.join(cwd, "package.json"), '{"name":"acp-probe","version":"1.0.0"}\n');
  const child = spawn("node", [path.join(here, "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js")], { cwd, env: { ...process.env, ANTHROPIC_MODEL: "haiku", CLAUDE_CODE_THINKING_DISPLAY: "summarized" }, stdio: ["pipe", "pipe", "ignore"] });
  const lines = []; const updates = []; let nextId = 1; const pending = new Map(); const kinds = {}; let lastUpdateAt = Date.now(); let sawTool = null; const reqs = {};
  const rec = (dir, m) => lines.push(redact(JSON.stringify({ dir, t: Date.now() - T0, m })));
  const send = (m) => { rec("out", m); child.stdin.write(JSON.stringify(m) + "\n"); };
  const request = (method, params) => new Promise((resolve, reject) => { const id = nextId++; pending.set(id, { resolve, reject, method }); send({ jsonrpc: "2.0", id, method, params }); });
  const handle = (m) => {
    if (/auth\/status_update/.test(m.method ?? "")) return; // account details: not recorded
    rec("in", m);
    if (m.id !== undefined && m.method === undefined) { const p = pending.get(m.id); pending.delete(m.id); if (p) m.error ? p.reject(new Error(`${p.method}: ${JSON.stringify(m.error)}`)) : p.resolve(m.result); return; }
    if (m.method === "session/update") { const u = m.params.update; updates.push(u); kinds[u.sessionUpdate] = (kinds[u.sessionUpdate] ?? 0) + 1; lastUpdateAt = Date.now(); if (u.sessionUpdate === "tool_call_update" && sawTool) { const f = sawTool; sawTool = null; f(); } return; }
    reqs[m.method] = (reqs[m.method] ?? 0) + 1;
    if (m.method === "session/request_permission") { const o = m.params.options; const pick = o.find((x) => x.kind === (SC.deny ? "reject_once" : "allow_once")) ?? o[0]; console.log(`  ${ts()} permission "${clip(m.params.title, 50)}" -> ${pick.name}`); return setTimeout(() => send({ jsonrpc: "2.0", id: m.id, result: { outcome: { outcome: "selected", optionId: pick.optionId } } }), 800); }
    if (m.method === "elicitation/create") { const props = m.params.requestedSchema?.properties ?? {}; const content = {}; for (const [k, s] of Object.entries(props)) { const opt = s.enum?.[0] ?? s.oneOf?.[0]?.const ?? s.items?.enum?.[0] ?? s.items?.anyOf?.[0]?.const; if (opt !== undefined) content[k] = s.type === "array" ? [opt] : opt; } console.log(`  ${ts()} elicitation "${clip(m.params.message, 50)}" -> ${clip(content)}`); return setTimeout(() => send({ jsonrpc: "2.0", id: m.id, result: { action: "accept", content } }), 800); }
    console.log(`  ${ts()} unhandled request ${m.method}`); if (m.id !== undefined) send({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "not supported by recorder" } });
  };
  let buf = ""; child.stdout.on("data", (d) => { buf += d; let i; while ((i = buf.indexOf("\n")) >= 0) { const l = buf.slice(0, i); buf = buf.slice(i + 1); if (l.trim()) handle(JSON.parse(l)); } });
  const idle = async (maxMs = 150000) => { const start = Date.now(); for (;;) { const st = updates.filter((u) => u.sessionUpdate === "state_update").at(-1); if (st?.state === "idle" && (st.background ?? []).length === 0 && Date.now() - lastUpdateAt > 2500) return true; if (Date.now() - start > maxMs) { console.log("  !! idle timeout"); return false; } await sleep(250); } };
  let ok = true;
  try {
    const init = await request("initialize", { protocolVersion: 2, info: { name: "bridge-recorder", version: "0.0.1" }, capabilities: { elicitation: { form: {}, url: {} } } });
    const s = await request("session/new", { cwd, mcpServers: [] });
    if (SC.mode) await request("session/set_config_option", { sessionId: s.sessionId, configId: "mode", type: "id", value: SC.mode });
    for (const text of SC.prompts) {
      const n = updates.length;
      if (SC.cancelAfterToolMs) sawTool = () => setTimeout(() => { console.log(`  ${ts()} session/cancel`); send({ jsonrpc: "2.0", method: "session/cancel", params: { sessionId: s.sessionId } }); }, SC.cancelAfterToolMs);
      await request("session/prompt", { sessionId: s.sessionId, prompt: [{ type: "text", text }] });
      if (SC.steer) { await sleep(SC.steerAfterMs); console.log(`  ${ts()} second prompt while busy`); await request("session/prompt", { sessionId: s.sessionId, prompt: [{ type: "text", text: SC.steer }] }).catch((e) => console.log("  second prompt rejected", clip(e.message, 150))); }
      for (let i = 0; i < 40 && updates.length === n; i++) await sleep(250);
      ok = (await idle()) && ok;
      if (SC.lingerMs) { await sleep(SC.lingerMs); ok = (await idle()) && ok; }
    }
    const header = { transcript: 1, scenario: name, agent: "claude", harness: "claude 2.1.330", adapter: `${init.info?.name}@${init.info?.version}`, protocol: init.protocolVersion, model: "haiku", prompts: SC.prompts, recordedAt: new Date().toISOString().slice(0, 10) };
    fs.writeFileSync(path.join(outDir, `${name}.ndjson`), [JSON.stringify(header), ...lines].join("\n") + "\n");
    console.log(`${ok ? "ok " : "!! "} ${name.padEnd(20)} ${String(lines.length).padStart(4)} frames  ${clip(Object.entries(kinds).filter(([k]) => !/usage|available_commands|config_option|session_info/.test(k)).map(([k, v]) => `${k}:${v}`).join(" "), 330)} ${clip(Object.entries(reqs).map(([k, v]) => `REQ ${k}:${v}`).join(" "), 80)}`);
  } catch (e) { console.log(`FAILED ${name}: ${clip(e.message, 400)}`); }
  child.kill(); await sleep(300);
}
const which = process.argv[2] ?? "all";
for (const name of which === "all" ? Object.keys(SCENARIOS) : which.split(",")) await record(name);
process.exit(0);

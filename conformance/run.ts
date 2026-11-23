// Runs the live scenarios for one agent and compares what they saw with the baseline.
//
//   node run.ts <agent> [scenario ...] [--free] [--update]
//
// --free    only the scenarios that spend no model turn
// --update  write what was seen as the new baseline (review the diff before committing)

import type { Json, JsonObject } from "@harness-bridge/client";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { Adapter, Daemon, here, readJson, Run, within, type AgentConfig, type Outcome } from "./lib.ts";
import { scenarios } from "./scenarios.ts";

const args = process.argv.slice(2);
const flags = new Set(args.filter((arg) => arg.startsWith("--")));
const [agentId, ...wanted] = args.filter((arg) => !arg.startsWith("--"));
const config = readJson(path.join(here, "config.json")) as { agents: Record<string, AgentConfig> } | null;
if (!config) throw new Error("copy config.example.json to config.json and fill in the paths");
const agent = agentId ? config.agents[agentId] : undefined;
if (!agentId || !agent) throw new Error(`usage: node run.ts <${Object.keys(config.agents).join("|")}> [scenario ...] [--free] [--update]`);

// A small project for the sessions to live in, with an instruction file to find.
const work = path.join(here, "work", agentId);
mkdirSync(work, { recursive: true });
const marker = "The conformance marker is ZEBRA-417. When asked for the conformance marker, reply with exactly that.\n";
writeFileSync(path.join(work, "CLAUDE.md"), marker);
writeFileSync(path.join(work, "AGENTS.md"), marker);
writeFileSync(path.join(work, "package.json"), '{ "name": "conformance-work", "private": true }\n');

const stable = (value: Json): string =>
  JSON.stringify(value, (_, inner) =>
    inner && typeof inner === "object" && !Array.isArray(inner) ? Object.fromEntries(Object.entries(inner).sort(([a], [b]) => a.localeCompare(b))) : inner);

const baselineFile = path.join(here, "baseline", `${agentId}.json`);
const baseline = (readJson(baselineFile) ?? {}) as Record<string, Record<string, Json>>;
const chosen = scenarios.filter((scenario) =>
  scenario.agents.includes(agentId) && (wanted.length === 0 || wanted.includes(scenario.name)) && !(flags.has("--free") && scenario.cost === "turn"));

const partial = chosen.length < scenarios.filter((scenario) => scenario.agents.includes(agentId)).length;

const daemon = new Daemon(agent);
await daemon.start();
const made = new Set<string>();
const shared = new Map<string, Json>();
const results: { name: string; about: string; verdict: string; outcome: Outcome; changes: string[]; ms: number }[] = [];

for (const scenario of chosen) {
  const run = new Run(agent, daemon, work, made);
  const started = Date.now();
  try {
    await within(600_000, scenario.name, scenario.run(run, shared));
  } catch (error) {
    run.outcome.failed.push(`stopped: ${error instanceof Error ? error.message : String(error)}`);
  }
  await run.close().catch(() => {});
  const expected = baseline[scenario.name];
  const changes: string[] = [];
  if (expected) {
    for (const key of new Set([...Object.keys(expected), ...Object.keys(run.outcome.facts)])) {
      const [was, is] = [expected[key], run.outcome.facts[key]];
      // A partial run cannot see what depends on a scenario it left out.
      if (partial && is === undefined) continue;
      if (stable(was ?? null) !== stable(is ?? null)) changes.push(`${key}: was ${stable(was ?? null)}, is ${stable(is ?? null)}`);
    }
  }
  const verdict = run.outcome.failed.length ? "FAILED" : !expected ? "new" : changes.length ? "CHANGED" : "same";
  results.push({ name: scenario.name, about: scenario.about, verdict, outcome: run.outcome, changes, ms: Date.now() - started });
  console.log(`${verdict.padEnd(8)} ${scenario.name}  (${Math.round((Date.now() - started) / 1000)}s)`);
  for (const line of [...run.outcome.failed, ...changes]) console.log(`           ${line}`);
}
await daemon.stop();

// Leave nothing behind in the harness's own history, where the adapter can delete.
let removed = 0;
let left = made.size;
if (agent.daemon.profile === "claude") {
  const adapter = await Adapter.start(agent);
  const listed = ((await adapter.call("session/list", { cwd: work })) as { sessions?: { sessionId: string }[] }).sessions ?? [];
  for (const sessionId of new Set([...made, ...listed.map((session) => session.sessionId)])) {
    await adapter.call("session/delete", { sessionId }).then(() => (removed += 1), () => {});
  }
  left = (((await adapter.call("session/list", { cwd: work })) as { sessions?: unknown[] }).sessions ?? []).length;
  await adapter.stop();
}
console.log(`sessions made: ${made.size}, deleted from the harness: ${removed}, left: ${left}`);

const facts = (record: Record<string, Json>) => Object.entries(record).map(([key, value]) => `- \`${key}\`: \`${stable(value)}\``).join("\n");
const report = [
  `# Conformance: ${agentId}`,
  "",
  `Run on ${new Date().toISOString().slice(0, 10)}. ${results.length} scenarios.`,
  "",
  "| Scenario | Result | Seconds |",
  "|---|---|---|",
  ...results.map((result) => `| ${result.name} | ${result.verdict} | ${Math.round(result.ms / 1000)} |`),
  "",
  ...results.flatMap((result) => [
    `## ${result.name}: ${result.verdict}`,
    "",
    result.about,
    "",
    ...(result.outcome.failed.length ? ["Failed:", ...result.outcome.failed.map((line) => `- ${line}`), ""] : []),
    ...(result.changes.length ? ["Changed from the baseline:", ...result.changes.map((line) => `- ${line}`), ""] : []),
    facts(result.outcome.facts),
    ...(Object.keys(result.outcome.info).length ? ["", "Not compared:", facts(result.outcome.info)] : []),
    "",
  ]),
].join("\n");
mkdirSync(path.join(here, "reports"), { recursive: true });
writeFileSync(path.join(here, "reports", `${agentId}.md`), report);
writeFileSync(path.join(here, "reports", `${agentId}.json`), JSON.stringify(results, null, 2));

if (flags.has("--update")) {
  const next: JsonObject = { ...baseline };
  for (const result of results) {
    if (result.verdict !== "FAILED") next[result.name] = { ...(partial ? baseline[result.name] : {}), ...result.outcome.facts };
  }
  mkdirSync(path.dirname(baselineFile), { recursive: true });
  writeFileSync(baselineFile, `${JSON.stringify(next, null, 2)}\n`);
  console.log(`baseline written: ${path.relative(process.cwd(), baselineFile)}`);
}
process.exit(results.some((result) => result.verdict === "FAILED" || result.verdict === "CHANGED") ? 1 : 0);

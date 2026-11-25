// Agent and connection status, usage, and what goes on in the background.

import { Conversation, type Archipel, type Json } from "@archipel/client";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Activity, AgentStatus, ChatView, ConnectionStatus, agentProblem, backgroundOf, todosOf, usageOf } from "../src/index.ts";

const states = path.join(import.meta.dirname, "../../../testdata/states");
const load = (name: string) => Conversation.fromJSON(JSON.parse(readFileSync(path.join(states, `${name}.json`), "utf8")));
const agent = (readiness: Archipel.AgentReadiness, detail?: string): Archipel.AgentInfo => ({ agentId: "claude", name: "Claude", readiness, detail });

test("an agent that cannot be used is named with the reason, and one that can is not mentioned", () => {
  assert.equal(agentProblem(agent("ready")), null);
  assert.equal(agentProblem(agent("not_started")), null, "nothing is known yet, so nothing is claimed");
  assert.deepEqual(agentProblem(agent("starting")), { tone: "wait", text: "Claude is starting…" });
  assert.match(agentProblem(agent("needs_login"))!.text, /not signed in/);
  assert.match(agentProblem(agent("not_installed", "claude-agent-acp: No such file"))!.text, /not installed on the daemon's machine \(claude-agent-acp: No such file\)/);
  assert.match(agentProblem(agent("error", "it stopped"))!.text, /could not be started \(it stopped\)/);
  assert.equal(renderToStaticMarkup(<AgentStatus agents={[agent("ready"), agent("not_started")]} />), "");
  const html = renderToStaticMarkup(<AgentStatus agents={[agent("needs_login"), { ...agent("starting"), agentId: "pi", name: "Pi" }]} onRetry={() => {}} />);
  assert.match(html, /role="alert"><span>Claude is not signed in.*?<button type="button">Try again<\/button>/);
  assert.match(html, /role="status"><span>Pi is starting…<\/span><\/div>/, "nothing to retry while it starts");
});

test("a lost connection is said, a working one is not", () => {
  assert.equal(renderToStaticMarkup(<ConnectionStatus connected />), "");
  assert.match(renderToStaticMarkup(<ConnectionStatus connected={false} />), /Reconnecting…/);
});

test("usage shows how full the context is, and the cost when the agent gives one", () => {
  const [, claude] = load("claude-v2/simple").root()!;
  const usage = usageOf(claude);
  assert.equal(usage.percent, 3);
  assert.equal(usage.cost, "$0.0031");
  assert.ok((usage.turnTokens ?? 0) > 0);
  const html = renderToStaticMarkup(<ChatView conversation={load("claude-v2/simple")} />);
  assert.match(html, /role="meter" aria-valuenow="3".*?3% of context<span class="arc-cost">\$0\.0031<\/span>/);
  const [, pi] = load("pi-v2/simple").root()!;
  assert.equal(usageOf(pi).cost, undefined, "Pi gives no cost");
  assert.doesNotMatch(renderToStaticMarkup(<ChatView conversation={load("pi-v2/simple")} />), /arc-cost/);
  pi.values.set("usage", { used: 9000, size: 10000, cost: { amount: 12.5, currency: "EUR" } });
  assert.deepEqual([usageOf(pi).percent, usageOf(pi).cost], [90, "12.50 EUR"]);
  pi.values.delete("usage");
  assert.deepEqual(usageOf(pi), { used: undefined, size: undefined, percent: undefined, cost: undefined, turnTokens: usageOf(pi).turnTokens });
});

test("what runs in the background and the agent's todo list stay in view", () => {
  const conversation = load("claude-v2/background");
  const [, state] = conversation.root()!;
  assert.deepEqual(backgroundOf(state), []);
  assert.equal(todosOf(state), null);
  assert.equal(renderToStaticMarkup(<Activity state={state} />), "");

  const [terminalId, terminal] = [...state.collections.get("terminal")!][0]!;
  const turn = state.values.get("state") as { background: unknown[] };
  turn.background = [{ type: "terminal", terminalId }, { type: "tool_call", toolCallId: "unknown-call" }, { type: "mystery" }];
  assert.deepEqual(backgroundOf(state), [
    { kind: "command", id: terminalId, label: (terminal as { command: string }).command },
    { kind: "tool", id: "unknown-call", label: "unknown-call" },
  ]);
  state.collections.set("plan", new Map<string, Json>([
    ["old", { planId: "old", type: "items", entries: [{ content: "stale", status: "completed" }] }],
    ["notes", { planId: "notes", type: "markdown", content: "# not a list" }],
    ["now", { planId: "now", type: "items", entries: [{ content: "alpha", status: "completed" }, { content: "beta", status: "in_progress" }, { content: "gamma", status: "pending" }] }],
  ]));
  assert.deepEqual(todosOf(state)!.map((todo) => todo.content), ["alpha", "beta", "gamma"], "the newest plan that is a list");
  const html = renderToStaticMarkup(<ChatView conversation={conversation} />);
  assert.match(html, /Tasks<\/span> 1 of 3 done<span class="arc-todo-now"> · beta<\/span>/);
  assert.match(html, /Running in the background<\/span><span class="arc-chip" title="command">/);
});

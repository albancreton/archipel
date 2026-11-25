// The settings bar and the session list, drawn to HTML.

import { Conversation, type Bridge, type SessionSummary } from "@harness-bridge/client";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatView, SessionList, SettingsBar, ago, byFolder, settingsOf, withSession, withoutSession } from "../src/index.ts";

const states = path.join(import.meta.dirname, "../../../testdata/states");
const load = (name: string) => Conversation.fromJSON(JSON.parse(readFileSync(path.join(states, `${name}.json`), "utf8")));

test("the settings an agent reports are read by kind, whatever they are called", () => {
  const [, claude] = load("claude-v2/simple").root()!;
  const settings = settingsOf(claude.values.get("configOptions"));
  assert.deepEqual(settings.map(({ id, kind, category }) => [id, kind, category]), [
    ["model", "choice", "model"],
    ["effort", "choice", "thought_level"],
    ["ultracode", "switch", "model_config"],
    ["mode", "choice", "mode"],
  ]);
  assert.equal(settings[0]!.current, "haiku");
  assert.equal(settings[2]!.current, false);
  const [, pi] = load("pi-v2/simple").root()!;
  assert.deepEqual(settingsOf(pi.values.get("configOptions")).map((setting) => setting.id), ["model", "thought_level"]);

  // Values under headings, and a kind this kit does not know.
  const grouped = settingsOf([
    { configId: "model", name: "Model", type: "select", currentValue: "b", options: [
      { groupId: "g1", name: "Fast", options: [{ value: "a", name: "A" }] },
      { groupId: "g2", name: "Careful", options: [{ value: "b", name: "B" }] },
    ] },
    { configId: "budget", name: "Budget", type: "number", currentValue: 3 },
  ]);
  assert.equal(grouped.length, 1);
  assert.deepEqual(grouped[0]!.groups.map((group) => [group.name, group.choices.map((choice) => choice.value)]), [["Fast", ["a"]], ["Careful", ["b"]]]);
  assert.deepEqual(settingsOf(undefined), []);
});

test("the settings bar draws a menu per choice and a checkbox per switch", () => {
  const [, state] = load("claude-v2/simple").root()!;
  const settings = settingsOf(state.values.get("configOptions"));
  const html = renderToStaticMarkup(<SettingsBar settings={settings} onChange={() => {}} />);
  assert.match(html, /<span class="hb-label">Model<\/span><select>/);
  assert.match(html, /<option value="haiku"[^>]*selected=""/);
  assert.match(html, /<input type="checkbox"\/>Ultracode/);
  assert.doesNotMatch(html, /disabled/);
  assert.match(renderToStaticMarkup(<SettingsBar settings={settings} />), /<select disabled="">/, "shown, not changeable, without onChange");
  assert.equal(renderToStaticMarkup(<SettingsBar settings={[]} onChange={() => {}} />), "");

  // The chat draws it only for an app that can change a setting.
  const conversation = load("claude-v2/simple");
  assert.doesNotMatch(renderToStaticMarkup(<ChatView conversation={conversation} />), /hb-settings/);
  assert.match(renderToStaticMarkup(<ChatView conversation={conversation} actions={{ setOption: () => {} }} />), /hb-settings/);
});

const summary = (sessionId: string, more: Partial<SessionSummary> = {}): SessionSummary => ({
  sessionId, agentId: "claude", cwd: "/work/app", live: false, logged: true, headSeq: 1, ...more,
});

test("the session list groups by folder and says what each session is doing", () => {
  const now = new Date("2026-11-24T12:00:00Z");
  const sessions = [
    summary("s1", { title: "Fix the login", state: "requires_action", updatedAt: "2026-11-24T11:55:00Z" }),
    summary("s2", { title: "Docs", cwd: "/work/site", state: "running", openElsewhere: true }),
    summary("s3", { agentId: "pi", logged: false }),
    summary("s4", { title: "Old", state: "idle", updatedAt: "2026-11-21T12:00:00Z" }),
  ];
  assert.deepEqual(byFolder(sessions).map((folder) => [folder.cwd, folder.sessions.map((session) => session.sessionId)]), [
    ["/work/app", ["s1", "s3", "s4"]],
    ["/work/site", ["s2"]],
  ]);
  const html = renderToStaticMarkup(<SessionList sessions={sessions} selected="s1" now={now} onOpen={() => {}} />);
  assert.match(html, /<h3 class="hb-label" title="\/work\/app">app<\/h3>/);
  assert.match(html, /class="hb-session hb-selected"/);
  assert.match(html, /Fix the login<\/span>.*?needs you<\/span>.*?>5m<\/time>/);
  assert.match(html, /Docs<\/span>.*?running<\/span>.*?open elsewhere<\/span>/);
  assert.match(html, /Untitled<\/span>.*?from pi<\/span>/);
  assert.match(html, /Old<\/span><span class="hb-session-meta"><span class="hb-session-agent">claude<\/span><time[^>]*>3d<\/time>/, "an idle session has no badge");
  assert.doesNotMatch(html, />Rename<|>Delete<|Show more/);
  assert.match(renderToStaticMarkup(<SessionList sessions={sessions} groupBy="none" />), /^<div class="hb-sessions"><ul>/);
  assert.match(renderToStaticMarkup(<SessionList sessions={[]} />), /No sessions/);
});

test("rename and delete are offered only where the agent can do them", () => {
  const sessions = [summary("s1", { title: "On Claude" }), summary("s2", { title: "On Pi", agentId: "pi" })];
  const act = { onRename: () => {}, onDelete: () => {}, onMore: () => {} };
  const all = renderToStaticMarkup(<SessionList sessions={sessions} {...act} />);
  assert.equal(all.match(/>Rename</g)?.length, 2, "without the agent list every session gets the controls");
  assert.match(all, /Show more/);
  const agents: Bridge.AgentInfo[] = [
    { agentId: "claude", name: "Claude", readiness: "ready", capabilities: { session: { title: {}, delete: {} } } },
    { agentId: "pi", name: "Pi", readiness: "ready", capabilities: { session: {} } },
  ];
  const html = renderToStaticMarkup(<SessionList sessions={sessions} agents={agents} {...act} />);
  assert.equal(html.match(/>Rename</g)?.length, 1);
  assert.equal(html.match(/>Delete</g)?.length, 1);
  assert.ok(html.indexOf(">Rename<") < html.indexOf("On Pi"), "the controls are on Claude's row");
});

test("list helpers: a changed session comes first once, a deleted one goes, and times are short", () => {
  const list = [summary("a"), summary("b")];
  assert.deepEqual(withSession(list, summary("b", { title: "new" })).map((session) => [session.sessionId, session.title]), [["b", "new"], ["a", undefined]]);
  assert.deepEqual(withoutSession(list, "a").map((session) => session.sessionId), ["b"]);
  const now = new Date("2026-11-24T12:00:00Z");
  assert.deepEqual(
    ["2026-11-24T11:59:40Z", "2026-11-24T09:00:00Z", "2026-09-01T00:00:00Z", undefined, "nonsense"].map((at) => ago(at, now)),
    ["now", "3h", "2026-09-01", "", ""],
  );
});

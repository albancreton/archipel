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

import { Composer, acceptsOf, commandQuery, commandsOf, mentionQuery, promptContent } from "../src/index.ts";

test("the composer knows when a command or a mention is being typed", () => {
  assert.deepEqual(["/", "/mod", "/model ", "say /model", "", "//"].map(commandQuery), ["", "mod", null, null, null, null]);
  assert.deepEqual(mentionQuery("look at @src/ma", 15), { start: 8, query: "src/ma" });
  assert.deepEqual(mentionQuery("@", 1), { start: 0, query: "" });
  assert.equal(mentionQuery("mail me@example.com", 19), null, "an @ inside a word is not a mention");
  assert.equal(mentionQuery("look at @src then", 17), null, "the caret has left it");
  const [, state] = load("claude-v2/simple").root()!;
  const commands = commandsOf(state.values.get("availableCommands"));
  assert.ok(commands.length > 3 && commands.every((command) => command.name && !command.name.startsWith("/")));
  assert.deepEqual(commandsOf([{ type: "command", name: "review", description: "Review", input: { type: "text", hint: "a PR number" } }, { nameless: true }]),
    [{ name: "review", description: "Review", hint: "a PR number" }]);
});

test("what is sent: text alone, or blocks with what was attached and mentioned", () => {
  assert.equal(promptContent("hello"), "hello");
  const picture = { name: "shot.png", mimeType: "image/png", data: "AAAA" };
  const notes = { name: "my notes.md", mimeType: "text/markdown", data: Buffer.from("# Hé").toString("base64") };
  const binary = { name: "a.zip", mimeType: "application/zip", data: "UEs=" };
  const main = { uri: "file:///work/src/main.ts", name: "src/main.ts" };
  const gone = { uri: "file:///work/gone.ts", name: "gone.ts" };
  assert.deepEqual(promptContent("see @src/main.ts please", [picture, notes, binary], [main, gone, main]), [
    { type: "text", text: "see @src/main.ts please" },
    { type: "resource_link", uri: "file:///work/src/main.ts", name: "src/main.ts" },
    { type: "image", mimeType: "image/png", data: "AAAA" },
    { type: "resource", resource: { uri: "file:///my%20notes.md", mimeType: "text/markdown", text: "# Hé" } },
    { type: "resource", resource: { uri: "file:///a.zip", mimeType: "application/zip", blob: "UEs=" } },
  ], "a mention the user deleted from the text is not sent, and one is sent once");
  assert.deepEqual(promptContent("", [picture]), [{ type: "image", mimeType: "image/png", data: "AAAA" }]);
  assert.deepEqual(acceptsOf({ capabilities: { session: { prompt: { image: {}, embeddedContext: {} } } } }), { image: true, files: true });
  assert.deepEqual(acceptsOf({ capabilities: { session: { prompt: { image: {} } } } }), { image: true, files: false });
  assert.deepEqual(acceptsOf(undefined), { image: false, files: false });
});

test("attaching is offered only for what the agent takes, and the user's message shows what was sent", () => {
  assert.doesNotMatch(renderToStaticMarkup(<Composer running={false} />), /Attach/);
  assert.match(renderToStaticMarkup(<Composer running={false} accepts={{ image: true }} />), /<input type="file" multiple="" hidden="" accept="image\/\*"\/><button type="button" class="hb-attach">Attach/);
  assert.match(renderToStaticMarkup(<Composer running accepts={{ image: true, files: true }} />), /<input type="file" multiple="" hidden=""\/>.*Stop/);

  const conversation = load("claude-v2/simple");
  const [, state] = conversation.root()!;
  state.collections.get("userMessage")!.set("u2", { messageId: "u2", content: [
    { type: "text", text: "look <b>" },
    { type: "image", mimeType: "image/png", data: "AAAA" },
    { type: "image", mimeType: 'image/png" onerror="x', data: "AAAA" },
    { type: "resource_link", uri: "file:///work/src/main.ts", name: "src/main.ts" },
    { type: "resource", resource: { uri: "file:///my%20notes.md", text: "# Hi" } },
  ] });
  state.timeline.push({ target: "userMessage", id: "u2" });
  const html = renderToStaticMarkup(<ChatView conversation={conversation} />);
  assert.match(html, /look &lt;b&gt;<span class="hb-user-attached"><img src="data:image\/png;base64,AAAA" alt="attached picture"\/><span class="hb-chip">image<\/span><span class="hb-chip">src\/main\.ts<\/span><span class="hb-chip">my notes\.md<\/span>/);
});

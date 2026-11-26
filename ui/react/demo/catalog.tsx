// Draws every component of the kit, in each of its states, on one page, light and dark side by
// side: a reference to design from. `pnpm catalog`, then open demo/out/catalog.html.
//
// Most specimens are the components themselves, drawn with made-up data. A few states only exist
// after the user did something (a menu open, a row being renamed); those are drawn by hand with
// the same classes, and say so.

import { Conversation, SessionState, type Json, type JsonObject, type Request, type SessionSummary } from "@archipel/client";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import type { ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  Activity, AgentStatus, Approval, AssistantMessage, ChatView, Compaction, Composer, ConnectionStatus, Diff, Fallback,
  OpenElsewhere, Plan, Question, SessionList, SessionMessage, SettingsBar, StatusBar, Subagent, Terminal, Thought,
  Timeline, ToolCall, Usage, UserMessage, settingsOf,
} from "../src/index.ts";

const here = import.meta.dirname;
const css = readFileSync(path.join(here, "../src/styles.css"), "utf8");
const b64 = (text: string) => Buffer.from(text).toString("base64");

// ---- The kit's CSS, with its dark theme under a class instead of the OS setting ----

/** Moves each `@media (prefers-color-scheme: dark)` block under `.cat-dark`, so both themes show at once. */
function themed(source: string): string {
  let light = "", dark = "", at = 0;
  const marker = /@media\s*\(prefers-color-scheme:\s*dark\)\s*\{/g;
  for (let found = marker.exec(source); found; found = marker.exec(source)) {
    light += source.slice(at, found.index);
    let depth = 1, end = marker.lastIndex;
    for (; depth > 0; end++) depth += source[end] === "{" ? 1 : source[end] === "}" ? -1 : 0;
    const inner = source.slice(marker.lastIndex, end - 1);
    dark += inner.replace(/([^{}]+)\{/g, (_, selectors: string) =>
      `${selectors.split(",").map((one) => `.cat-dark ${one.trim()}`).join(", ")} {`);
    at = marker.lastIndex = end;
  }
  return light + source.slice(at) + "\n/* dark */\n" + dark;
}

// ---- Made-up data ----

function state(values: Record<string, Json>, collections: Record<string, Record<string, Json>> = {}, timeline: [string, string][] = []): SessionState {
  const made = new SessionState();
  for (const [key, value] of Object.entries(values)) made.values.set(key, value);
  for (const [target, entries] of Object.entries(collections)) made.collections.set(target, new Map(Object.entries(entries)));
  made.timeline = timeline.map(([target, id]) => ({ target, id })) as SessionState["timeline"];
  return made;
}

const text = (body: string): Json => [{ type: "text", text: body }];
const PIXEL = "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxNjAiIGhlaWdodD0iMTAwIj48ZGVmcz48bGluZWFyR3JhZGllbnQgaWQ9ImciIHgyPSIxIiB5Mj0iMSI+PHN0b3Agb2Zmc2V0PSIwIiBzdG9wLWNvbG9yPSIjNmVhMGZmIi8+PHN0b3Agb2Zmc2V0PSIxIiBzdG9wLWNvbG9yPSIjZDJhOGZmIi8+PC9saW5lYXJHcmFkaWVudD48L2RlZnM+PHJlY3Qgd2lkdGg9IjE2MCIgaGVpZ2h0PSIxMDAiIGZpbGw9InVybCgjZykiLz48cmVjdCB4PSIxNiIgeT0iMTYiIHdpZHRoPSI4MCIgaGVpZ2h0PSIxMCIgcng9IjMiIGZpbGw9IiNmZmYiIG9wYWNpdHk9Ii44Ii8+PHJlY3QgeD0iMTYiIHk9IjM0IiB3aWR0aD0iMTIwIiBoZWlnaHQ9IjgiIHJ4PSIzIiBmaWxsPSIjZmZmIiBvcGFjaXR5PSIuNSIvPjxyZWN0IHg9IjE2IiB5PSI0OCIgd2lkdGg9IjEwMCIgaGVpZ2h0PSI4IiByeD0iMyIgZmlsbD0iI2ZmZiIgb3BhY2l0eT0iLjUiLz48L3N2Zz4=";

const markdown = `I found the cause. The session list sorts by **creation time**, not by the last update, so a session that was just answered stays at the bottom.

### What changes

1. \`byFolder\` keeps the order the daemon sends.
2. The daemon sends sessions by \`updatedAt\`, newest first.

| File | Change |
|---|---|
| \`sessions.rs\` | sort by \`updated_at\` |
| \`sessions.tsx\` | none |

> The Swift kit had the same bug and gets the same fix.

\`\`\`rust
sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
\`\`\`

See [the design note](https://example.com/notes) for why.`;

const patch = `diff --git a/src/sessions.rs b/src/sessions.rs
--- a/src/sessions.rs
+++ b/src/sessions.rs
@@ -40,14 +40,14 @@ impl Sessions {
     pub fn list(&self) -> Vec<Summary> {
         let mut sessions: Vec<Summary> = self
             .by_id
             .values()
             .map(Summary::from)
             .collect();
-        sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));
+        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
         sessions
     }

     pub fn get(&self, id: &str) -> Option<&Session> {
         self.by_id.get(id)
     }
-
-    // TODO: order
+    /// Newest first.
 }`;

const ansi = "\x1b[1mrunning 3 tests\x1b[0m\ntest sessions::list_sorts ... \x1b[32mok\x1b[0m\ntest sessions::get_missing ... \x1b[32mok\x1b[0m\ntest sessions::rename ... \x1b[31mFAILED\x1b[0m\n\n\x1b[33mwarning\x1b[0m: unused variable: `draft`\n\x1b[2m  --> src/sessions.rs:88:9\x1b[0m\n\ntest result: \x1b[31;1mFAILED\x1b[0m. 2 passed; 1 failed";

const terminals: Record<string, Json> = {
  running: { terminalId: "running", command: "cargo test --workspace", output: { data: b64("   Compiling archipel-server v0.1.0\n   Compiling archipeld v0.1.0") } },
  ok: { terminalId: "ok", command: "git status --short", output: { data: b64(" M crates/archipel-server/src/sessions.rs\n?? notes.md") }, exitStatus: { exitCode: 0 } },
  failed: { terminalId: "failed", command: "cargo test -p archipel-server sessions", output: { data: b64(ansi) }, exitStatus: { exitCode: 101 } },
};

const tool = (status: string, extra: JsonObject = {}): Json => ({
  toolCallId: status, name: "Read", kind: "read", title: "crates/archipel-server/src/sessions.rs", status, ...extra,
});

const planEntries: Json = [
  { content: "Find where the list is sorted", status: "completed", priority: "high" },
  { content: "Sort by the last update", status: "in_progress", priority: "high" },
  { content: "Add a test for the order", status: "pending", priority: "medium" },
  { content: "Same fix in the Swift kit", status: "pending", priority: "low" },
];

const permission = (outcome: Json | null): Request => ({
  method: "session/request_permission",
  params: {
    title: "rm -rf target/debug/incremental",
    options: [
      { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
      { optionId: "allow-always", name: "Always allow", kind: "allow_always" },
      { optionId: "reject", name: "Reject", kind: "reject_once" },
    ],
  },
  outcome,
});

const question = (outcome: Json | null): Request => ({
  method: "session/elicitation",
  params: {
    message: "Which kit should get the fix first?",
    requestedSchema: {
      type: "object",
      properties: {
        kit: { type: "string", title: "Kit", oneOf: [{ const: "react", title: "React" }, { const: "swift", title: "SwiftUI" }] },
        note: { type: "string", title: "Anything else?" },
      },
    },
  },
  outcome,
});

const configOptions: Json = [
  { configId: "model", name: "Model", category: "model", type: "select", currentValue: "sonnet", options: [
    { value: "haiku", name: "Haiku" }, { value: "sonnet", name: "Sonnet" }, { value: "opus", name: "Opus" },
  ] },
  { configId: "effort", name: "Effort", category: "thought_level", type: "select", currentValue: "medium", options: [
    { value: "low", name: "Low" }, { value: "medium", name: "Medium" }, { value: "high", name: "High" },
  ] },
  { configId: "mode", name: "Mode", category: "mode", type: "select", currentValue: "default", options: [
    { value: "default", name: "Ask before edits" }, { value: "acceptEdits", name: "Accept edits" }, { value: "plan", name: "Plan" },
  ] },
  { configId: "fast", name: "Fast mode", type: "boolean", currentValue: false },
];

const usage = (used: number, cost?: number): Json => ({ used, size: 200_000, ...(cost !== undefined ? { cost: { amount: cost, currency: "USD" } } : {}) });

const now = new Date("2026-11-25T12:00:00Z");
const minutesAgo = (minutes: number) => new Date(now.getTime() - minutes * 60_000).toISOString();
const sessions: SessionSummary[] = [
  { sessionId: "a", agentId: "claude", title: "Sort the session list by last update", cwd: "/work/archipel", live: true, logged: true, state: "requires_action", headSeq: 1, updatedAt: minutesAgo(0) },
  { sessionId: "b", agentId: "claude", title: "Fix the diff fold in dark mode", cwd: "/work/archipel", live: true, logged: true, state: "running", headSeq: 1, updatedAt: minutesAgo(4) },
  { sessionId: "c", agentId: "claude", title: "Write the addon coverage script", cwd: "/work/archipel", live: false, logged: true, state: "idle", headSeq: 1, updatedAt: minutesAgo(180) },
  { sessionId: "d", agentId: "claude", title: "Review the launchd plist", cwd: "/work/archipel", live: true, logged: true, openElsewhere: true, state: "idle", headSeq: 1, updatedAt: minutesAgo(60 * 26) },
  { sessionId: "e", agentId: "pi", title: "Port the timeline to SwiftUI", cwd: "/work/harbor-app", live: false, logged: false, headSeq: 0, updatedAt: minutesAgo(60 * 24 * 6) },
  { sessionId: "f", agentId: "pi", cwd: "/work/harbor-app", live: false, logged: true, state: "idle", headSeq: 1, updatedAt: minutesAgo(60 * 24 * 40) },
];

const agent = (agentId: string, name: string, readiness: string, detail?: string) => ({ agentId, name, readiness, detail }) as never;

// A small conversation with a subagent, for the Subagent and ChatView specimens.
const conversation = (() => {
  const made = new Conversation();
  const child = state({ state: { state: "idle" } }, {
    sessionMessage: { task: { content: text("Read package.json and reply with its name field.") } },
    toolCall: { read: tool("completed", { content: [{ type: "content", content: { type: "text", text: '{ "name": "archipel" }' } }] }) },
    agentMessage: { answer: { content: text("archipel") } },
  }, [["sessionMessage", "task"], ["toolCall", "read"], ["agentMessage", "answer"]]);
  const root = state({
    state: { state: "requires_action", background: [{ terminalId: "running" }] },
    sessionInfo: { title: "Sort the session list by last update" },
    usage: usage(61_000, 0.0412),
    configOptions,
    availableCommands: [{ name: "review", description: "Review the current diff" }],
  }, {
    userMessage: { u1: { content: text("The session list puts answered sessions at the bottom. Can you fix it?") } },
    agentThought: { t1: { content: text("The list is probably sorted on the daemon side. Let me look at sessions.rs first.") } },
    subagent: { sub: { sessionId: "sub", title: "Read package.json name field", state: { state: "idle" } } },
    toolCall: {
      edit: { toolCallId: "edit", name: "Edit", kind: "edit", title: "Edit crates/archipel-server/src/sessions.rs", status: "completed", content: [{ type: "diff", changes: [{ path: "src/sessions.rs", operation: "modify" }], patch: { text: patch } }] },
      test: { toolCallId: "test", name: "Bash", kind: "execute", title: "cargo test --workspace", status: "in_progress", content: [{ type: "terminal", terminalId: "running" }] },
    },
    terminal: { running: terminals["running"]! },
    plan: { p1: { entries: planEntries } },
    agentMessage: { a1: { content: text("The list is sorted by creation time. I changed it to the last update and am running the tests.") } },
  }, [["userMessage", "u1"], ["agentThought", "t1"], ["subagent", "sub"], ["plan", "p1"], ["toolCall", "edit"], ["agentMessage", "a1"], ["toolCall", "test"], ["request", "r1"]]);
  root.requests.set("r1", permission(null));
  made.sessions.set("root", root);
  made.sessions.set("sub", child);
  return made;
})();

// ---- Specimens ----

interface Specimen {
  name: string;
  /** How the app writes it. */
  code?: string;
  note?: string;
  /** Drawn by hand with the kit's classes, because the state needs the user to have acted. */
  byHand?: boolean;
  /** `chat` wraps in `.arc-chat` (the variables and the item styles), `bare` draws as is. */
  frame?: "chat" | "bare";
  wide?: boolean;
  draw: () => ReactNode;
  /** Changes the drawn HTML, such as opening a `<details>`. */
  after?: (html: string) => string;
}

const opened = (html: string) => html.replace(/<details(?![^>]*\sopen)/, "<details open");

const sections: { id: string; title: string; intro: string; specimens: Specimen[] }[] = [
  {
    id: "messages", title: "Messages", intro: "What the user and the agent say. The agent's text is Markdown, drawn as React elements.",
    specimens: [
      { name: "UserMessage", code: "<UserMessage item={item} />", note: "Text as written, right-aligned in the accent colour.", draw: () => <UserMessage item={{ content: text("The session list puts answered sessions at the bottom. Can you fix it?") }} /> },
      { name: "UserMessage · with attachments", code: "<UserMessage item={item} />", note: "A picture the user sent is shown from its own bytes; files and mentions are chips.", draw: () => (
        <UserMessage item={{ content: [
          { type: "text", text: "This is what it looks like now." },
          { type: "image", mimeType: "image/svg+xml", data: PIXEL },
          { type: "resource_link", name: "sessions.tsx", uri: "file:///work/archipel/ui/react/src/sessions.tsx" },
          { type: "resource", resource: { uri: "file:///work/notes/order.md", text: "…" } },
        ] }} />
      ) },
      { name: "AssistantMessage", code: "<AssistantMessage item={item} />", note: "Headings, lists, tables, quotes, code, links. No syntax highlighting.", draw: () => <AssistantMessage item={{ content: text(markdown) }} /> },
      { name: "AssistantMessage · short", code: "<AssistantMessage item={item} />", draw: () => <AssistantMessage item={{ content: text("Done. The tests pass.") }} /> },
      { name: "Thought · folded", code: "<Thought item={item} />", note: "Reasoning, folded by default.", draw: () => <Thought item={{ content: text("The list is probably sorted on the daemon side.") }} /> },
      { name: "Thought · open", code: "<Thought item={item} />", after: opened, draw: () => <Thought item={{ content: text("The list is probably sorted on the daemon side. `byFolder` keeps the order it is given, so the fix belongs in **sessions.rs**.") }} /> },
      { name: "SessionMessage", code: "<SessionMessage item={item} />", note: "A task given to a subagent, or the result it returned.", draw: () => <SessionMessage item={{ content: text("Read package.json and reply with only the value of its name field.") }} /> },
    ],
  },
  {
    id: "tools", title: "Tool calls", intro: "A tool call has a kind, a title, a status badge and what it produced: text, a diff, or a terminal.",
    specimens: [
      { name: "ToolCall · pending", code: "<ToolCall item={item} />", draw: () => <ToolCall item={tool("pending")} /> },
      { name: "ToolCall · in progress", code: "<ToolCall item={item} />", draw: () => <ToolCall item={tool("in_progress")} /> },
      { name: "ToolCall · completed, with output", code: "<ToolCall item={item} />", draw: () => <ToolCall item={tool("completed", { content: [{ type: "content", content: { type: "text", text: "pub fn list(&self) -> Vec<Summary> {\n    let mut sessions = …;\n    sessions.sort_by(|a, b| a.created_at.cmp(&b.created_at));" } }] })} /> },
      { name: "ToolCall · failed", code: "<ToolCall item={item} />", draw: () => <ToolCall item={tool("failed", { content: [{ type: "content", content: { type: "text", text: "File does not exist: crates/archipel-server/src/session.rs" } }] })} /> },
      { name: "ToolCall · cancelled", code: "<ToolCall item={item} />", draw: () => <ToolCall item={{ toolCallId: "x", kind: "search", title: "Search \"sort_by\" in crates/", status: "cancelled" }} /> },
      { name: "ToolCall · with a diff", code: "<ToolCall item={item} />", wide: true, draw: () => <ToolCall item={{ toolCallId: "e", kind: "edit", title: "Edit crates/archipel-server/src/sessions.rs", status: "completed", content: [{ type: "diff", changes: [{ path: "src/sessions.rs", operation: "modify" }], patch: { text: patch } }] }} /> },
      { name: "ToolCall · with a terminal", code: "<ToolCall item={item} terminal={find} />", wide: true, draw: () => <ToolCall item={{ toolCallId: "t", kind: "execute", title: "cargo test -p archipel-server sessions", status: "failed", content: [{ type: "terminal", terminalId: "failed" }] }} terminal={(id) => terminals[id]} /> },
    ],
  },
  {
    id: "output", title: "Terminal and diffs", intro: "Command output keeps its colours (16 ANSI variables, --arc-ansi-0 to 15). Diffs are per file, with line numbers and folded context.",
    specimens: [
      { name: "Terminal · running", code: "<Terminal item={terminal} />", wide: true, draw: () => <div className="arc-item"><Terminal item={terminals["running"]!} /></div> },
      { name: "Terminal · exit 0", code: "<Terminal item={terminal} />", wide: true, draw: () => <div className="arc-item"><Terminal item={terminals["ok"]!} /></div> },
      { name: "Terminal · failed, with colours", code: "<Terminal item={terminal} />", wide: true, draw: () => <div className="arc-item"><Terminal item={terminals["failed"]!} /></div> },
      { name: "Diff", code: "<Diff block={block} />", note: "Added and removed counts, line numbers, a hunk header, and \"Show N unchanged lines\" for long context.", wide: true, draw: () => <Diff block={{ changes: [{ path: "src/sessions.rs", operation: "modify" }], patch: { text: patch } }} /> },
      { name: "Diff · new file", code: "<Diff block={block} />", wide: true, draw: () => <Diff block={{ changes: [{ path: "NOTES.md", operation: "add" }], patch: { text: "diff --git a/NOTES.md b/NOTES.md\nnew file mode 100644\n--- /dev/null\n+++ b/NOTES.md\n@@ -0,0 +1,3 @@\n+# Notes\n+\n+Sessions are listed newest first." } }} /> },
    ],
  },
  {
    id: "structure", title: "Plans, subagents, compaction", intro: "Items that hold other content.",
    specimens: [
      { name: "Plan · entries", code: "<Plan item={item} />", note: "Done (✓, struck through), in progress (…), pending (○). Todos arrive the same way.", draw: () => <Plan item={{ entries: planEntries }} /> },
      { name: "Plan · document", code: "<Plan item={item} />", draw: () => <Plan item={{ content: text("1. Sort by updated_at in the daemon.\n2. Leave the kits as they are.\n3. Add a test.") }} /> },
      { name: "Subagent · running", code: "<Subagent item={item}>{timeline}</Subagent>", note: "Open by default; its own session is drawn inside.", draw: () => (
        <Subagent item={{ title: "Find every place that sorts sessions", state: { state: "running" } }}>
          <SessionMessage item={{ content: text("Find every place in the repository that sorts sessions.") }} />
        </Subagent>
      ) },
      { name: "Subagent · done, with its timeline", code: "<Subagent item={item}><Timeline … /></Subagent>", wide: true, draw: () => (
        <Subagent item={{ title: "Read package.json name field", state: { state: "idle" } }}>
          <Timeline conversation={conversation} state={conversation.sessions.get("sub")!} />
        </Subagent>
      ) },
      { name: "Compaction · folded", code: "<Compaction item={item} />", draw: () => <Compaction item={{ status: "completed", summary: text("…") }} /> },
      { name: "Compaction · open", code: "<Compaction item={item} />", after: opened, draw: () => <Compaction item={{ status: "completed", summary: text("The user asked to sort the session list by last update. The daemon now sorts by updated_at; tests pass.") }} /> },
      { name: "Fallback", code: "<Fallback kind={kind} item={item} />", note: "Any kind the kit does not know, such as an extension's x.acme.checkpoint.", draw: () => <Fallback kind="x.acme.checkpoint" item={{ label: "before refactor", commit: "9f4b4ad" }} /> },
      { name: "Fallback · open", code: "<Fallback kind={kind} item={item} />", after: opened, draw: () => <Fallback kind="x.acme.checkpoint" item={{ label: "before refactor", commit: "9f4b4ad" }} /> },
    ],
  },
  {
    id: "requests", title: "Approvals and questions", intro: "The turn waits on these. Pending ones have the running-colour border; once answered they show the answer.",
    specimens: [
      { name: "Approval · pending", code: "<Approval request={request} onChoose={…} />", note: "The first option is the primary button.", draw: () => <Approval request={permission(null)} /> },
      { name: "Approval · answered", code: "<Approval request={request} />", draw: () => <Approval request={permission({ outcome: { outcome: "selected", optionId: "allow-once" } })} /> },
      { name: "Approval · cancelled", code: "<Approval request={request} />", draw: () => <Approval request={permission({ outcome: { outcome: "cancelled" } })} /> },
      { name: "Question · pending", code: "<Question request={request} onAnswer={…} />", note: "A form built from the schema the agent sent: a menu for choices, a field for text.", draw: () => <Question request={question(null)} /> },
      { name: "Question · answered", code: "<Question request={request} />", draw: () => <Question request={question({ action: "accept", content: { kit: "react", note: "then Swift" } })} /> },
      { name: "Question · skipped", code: "<Question request={request} />", draw: () => <Question request={question({ action: "decline" })} /> },
    ],
  },
  {
    id: "chrome", title: "Around the timeline", intro: "The status bar at the top of a chat, and what sits above the composer.",
    specimens: [
      { name: "StatusBar · idle", code: "<StatusBar state={state} />", wide: true, draw: () => <StatusBar state={state({ state: { state: "idle" }, sessionInfo: { title: "Sort the session list by last update" }, usage: usage(24_000, 0.0081) })} /> },
      { name: "StatusBar · running", code: "<StatusBar state={state} />", wide: true, draw: () => <StatusBar state={state({ state: { state: "running" }, sessionInfo: { title: "Fix the diff fold in dark mode" }, usage: usage(92_000, 0.31) })} /> },
      { name: "StatusBar · needs action, context nearly full", code: "<StatusBar state={state} />", note: "The meter turns to the running colour at 80%.", wide: true, draw: () => <StatusBar state={state({ state: { state: "requires_action" }, sessionInfo: { title: "Review the launchd plist" }, usage: usage(171_000, 2.4) })} /> },
      { name: "StatusBar · new session, no usage", code: "<StatusBar state={state} />", wide: true, draw: () => <StatusBar state={state({})} /> },
      { name: "Usage · context only (Pi)", code: "<Usage state={state} />", draw: () => <Usage state={state({ usage: { used: 12_000, size: 200_000 } })} /> },
      { name: "Activity · tasks and background work", code: "<Activity state={state} />", wide: true, draw: () => (
        <Activity state={state({ state: { state: "idle", background: [{ terminalId: "running" }, { sessionId: "sub" }] } }, { plan: { p: { entries: planEntries } }, terminal: { running: terminals["running"]! }, subagent: { sub: { title: "Find every place that sorts sessions" } } })} />
      ) },
      { name: "Activity · tasks open", code: "<Activity state={state} />", wide: true, after: opened, draw: () => <Activity state={state({}, { plan: { p: { entries: planEntries } } })} /> },
      { name: "OpenElsewhere · with fork", code: "<OpenElsewhere holder={holder} onFork={…} />", wide: true, draw: () => <OpenElsewhere holder={{ pid: 48213, status: "busy" }} onFork={() => {}} /> },
      { name: "OpenElsewhere · without fork", code: "<OpenElsewhere holder={holder} />", wide: true, draw: () => <OpenElsewhere holder={{ pid: 48213 }} /> },
      { name: "Session error", code: "<ChatView error={…} />", wide: true, draw: () => <div className="arc-problem arc-session-error" role="alert">The daemon cannot write its log (the disk is full). Messages are refused until it can.</div> },
      { name: "Show earlier", code: "<Timeline pageSize={200} />", note: "At the top of a long session.", draw: () => <div className="arc-timeline"><button type="button" className="arc-earlier">Show earlier (1240 more)</button></div> },
    ],
  },
  {
    id: "input", title: "Settings and composer", intro: "The settings come from what the agent reports: a menu per choice, a checkbox per switch.",
    specimens: [
      { name: "SettingsBar", code: "<SettingsBar settings={settingsOf(configOptions)} onChange={…} />", wide: true, draw: () => <SettingsBar settings={settingsOf(configOptions)} onChange={() => {}} /> },
      { name: "SettingsBar · read only", code: "<SettingsBar settings={settings} />", note: "Without onChange, or while a change is being confirmed.", wide: true, draw: () => <SettingsBar settings={settingsOf(configOptions)} /> },
      { name: "SettingsBar · refused change", byHand: true, wide: true, draw: () => (
        <div className="arc-settings">
          {renderToStaticMarkupInline(<SettingsBar settings={settingsOf(configOptions)} onChange={() => {}} />, (html) => html.replace(/^<div class="arc-settings">|<\/div>$/g, ""))}
          <div className="arc-problem" role="alert">Not changed: Model: opus is not available on this plan</div>
        </div>
      ) },
      { name: "Composer · idle", code: "<Composer running={false} onSend={…} />", wide: true, draw: () => <Composer running={false} onSend={() => {}} /> },
      { name: "Composer · running", code: "<Composer running onSend={…} onCancel={…} />", note: "The placeholder changes and Stop appears.", wide: true, draw: () => <Composer running onSend={() => {}} onCancel={() => {}} /> },
      { name: "Composer · can attach", code: "<Composer accepts={{ image: true, files: true }} />", wide: true, draw: () => <Composer running={false} onSend={() => {}} accepts={{ image: true, files: true }} /> },
      { name: "Composer · command menu", byHand: true, note: "After typing \"/\". The chosen row is highlighted; arrows move it.", wide: true, draw: () => (
        <form className="arc-composer" style={{ marginTop: 150 }}>
          <ul className="arc-menu" role="listbox">
            <li className="arc-chosen"><span className="arc-menu-label">/review</span><span className="arc-menu-detail">Review the current diff</span></li>
            <li><span className="arc-menu-label">/compact</span><span className="arc-menu-detail">[instructions] · Summarise the conversation to free context</span></li>
            <li><span className="arc-menu-label">/init</span><span className="arc-menu-detail">Write a CLAUDE.md for this project</span></li>
          </ul>
          <textarea defaultValue="/" />
          <button type="submit">Send</button>
        </form>
      ) },
      { name: "Composer · mention menu", byHand: true, note: "After typing \"@\", with the files the app supplies.", wide: true, draw: () => (
        <form className="arc-composer" style={{ marginTop: 120 }}>
          <ul className="arc-menu" role="listbox">
            <li className="arc-chosen"><span className="arc-menu-label">@ui/react/src/sessions.tsx</span><span className="arc-menu-detail">182 lines</span></li>
            <li><span className="arc-menu-label">@crates/archipel-server/src/sessions.rs</span><span className="arc-menu-detail">410 lines</span></li>
          </ul>
          <textarea defaultValue="Look at @sess" />
          <button type="submit">Send</button>
        </form>
      ) },
      { name: "Composer · with attachments and a problem", byHand: true, wide: true, draw: () => (
        <form className="arc-composer">
          <ul className="arc-attachments">
            <li><img src={`data:image/svg+xml;base64,${PIXEL}`} alt="" /><span>screenshot.png</span><button type="button">×</button></li>
            <li><span>order.md</span><button type="button">×</button></li>
          </ul>
          <textarea defaultValue="This is what it looks like now." />
          <button type="button" className="arc-attach">Attach</button>
          <button type="submit">Send</button>
          <div className="arc-problem" role="alert">video.mov: larger than 5 MB</div>
        </form>
      ) },
    ],
  },
  {
    id: "notices", title: "Agent and connection", intro: "Notices for the app's own chrome. Both draw nothing when all is well.",
    specimens: [
      { name: "AgentStatus", code: "<AgentStatus agents={agents} onRetry={…} />", note: "Three tones: wait (muted), act (running colour), bad (error colour).", frame: "bare", wide: true, draw: () => (
        <AgentStatus onRetry={() => {}} agents={[
          agent("claude", "Claude Code", "starting"),
          agent("codex", "Codex", "needs_login"),
          agent("pi", "Pi", "not_installed", "pi was not found on PATH"),
          agent("gemini", "Gemini", "error", "exited with code 1"),
        ]} />
      ) },
      { name: "ConnectionStatus · lost", code: "<ConnectionStatus connected={false} />", frame: "bare", wide: true, draw: () => <ConnectionStatus connected={false} /> },
    ],
  },
  {
    id: "sessions", title: "Session list", intro: "Grouped by folder. A row says what needs the user first; rename and delete appear on hover (shown here).",
    specimens: [
      { name: "SessionList", code: "<SessionList sessions={…} selected={id} onRename={…} onDelete={…} onMore={…} />", note: "needs you · running · open elsewhere · from <agent> (started outside the app) · untitled.", frame: "bare", draw: () => (
        <div className="cat-hover"><SessionList sessions={sessions} selected="b" now={now} onOpen={() => {}} onRename={() => {}} onDelete={() => {}} onMore={() => {}} /></div>
      ) },
      { name: "SessionList · empty", code: "<SessionList sessions={[]} />", frame: "bare", draw: () => <SessionList sessions={[]} now={now} /> },
      { name: "SessionRow · renaming", byHand: true, frame: "bare", draw: () => (
        <div className="arc-sessions"><ul>
          <li className="arc-session arc-selected">
            <form className="arc-session-rename">
              <input defaultValue="Sort the session list" aria-label="Session title" />
              <button type="submit">Save</button>
              <button type="button">Cancel</button>
            </form>
          </li>
        </ul></div>
      ) },
      { name: "SessionRow · confirm delete", byHand: true, frame: "bare", draw: () => (
        <div className="arc-sessions"><ul>
          <li className="arc-session">
            {renderToStaticMarkupInline(<SessionList sessions={[sessions[2]!]} groupBy="none" now={now} />, (html) => html.match(/<div class="arc-session-line">[\s\S]*?<\/button><span class="arc-session-actions"><\/span><\/div>/)?.[0] ?? "")}
            <div className="arc-session-confirm" role="alertdialog">
              <span>Delete “Write the addon coverage script”? It is removed from claude’s own history too. This cannot be undone.</span>
              <button type="button" className="arc-danger">Delete</button>
              <button type="button">Cancel</button>
            </div>
          </li>
        </ul></div>
      ) },
      { name: "SessionRow · failed action", byHand: true, frame: "bare", draw: () => (
        <div className="arc-sessions"><ul>
          <li className="arc-session">
            {renderToStaticMarkupInline(<SessionList sessions={[sessions[4]!]} groupBy="none" now={now} />, (html) => html.match(/<div class="arc-session-line">[\s\S]*?<\/button><span class="arc-session-actions"><\/span><\/div>/)?.[0] ?? "")}
            <div className="arc-problem" role="alert">Not deleted: the agent cannot delete sessions (-32601)</div>
          </li>
        </ul></div>
      ) },
    ],
  },
  {
    id: "chat", title: "Assembled: ChatView", intro: "Everything above, put together by ChatView: status, timeline, activity, settings, composer.",
    specimens: [
      { name: "ChatView", code: "<ChatView conversation={…} actions={…} />", frame: "bare", wide: true, draw: () => (
        <ChatView conversation={conversation} sessionId="root" actions={{ prompt: () => {}, cancel: () => {}, approve: () => {}, setOption: () => {} }} />
      ) },
    ],
  },
];

/** Drawn HTML inside a specimen drawn by hand, for the parts that are the real component. */
function renderToStaticMarkupInline(node: ReactNode, pick: (html: string) => string): ReactNode {
  return <span className="cat-inline" dangerouslySetInnerHTML={{ __html: pick(renderToStaticMarkup(<>{node}</>)) }} />;
}

// ---- Foundations: the variables, read from the CSS ----

function variables(selector: RegExp): [string, string][] {
  const block = css.match(selector)?.[1] ?? "";
  return [...block.matchAll(/(--arc-[\w-]+):\s*([^;]+);/g)].map((found) => [found[1]!, found[2]!.trim()]);
}
const lightVars = variables(/\.arc-chat, \.arc-sessions, \.arc-settings \{([^}]*)\}/);
const darkVars = variables(/@media \(prefers-color-scheme: dark\) \{\s*\.arc-chat, \.arc-sessions, \.arc-settings \{([^}]*)\}/);
const ansiVars = (selector: RegExp): [string, string][] =>
  [...(css.match(selector)?.[1] ?? "").matchAll(/(--arc-ansi-\d+):\s*([^;]+);/g)].map((found) => [found[1]!, found[2]!.trim()]);
const lightAnsi = ansiVars(/\n\.arc-chat \{ (--arc-ansi-0[^}]*)\}/);
const darkAnsi = ansiVars(/@media \(prefers-color-scheme: dark\) \{ \.arc-chat \{ ([^}]*)\}/);

const roles: Record<string, string> = {
  "--arc-fg": "Text", "--arc-muted": "Secondary text, labels", "--arc-bg": "Page, buttons, code", "--arc-soft": "Cards, selected row",
  "--arc-line": "Borders, meter track", "--arc-accent": "User bubble, primary button, links", "--arc-ok": "Completed, added lines",
  "--arc-bad": "Failed, removed lines, errors", "--arc-run": "Running, pending requests, warnings",
};

function swatches(light: [string, string][], dark: [string, string][], withRoles: boolean): string {
  const darkOf = new Map(dark);
  return `<div class="cat-swatches">${light.map(([name, value]) => `
    <div class="cat-swatch">
      <div class="cat-chip-pair"><span style="background:${value}"></span><span style="background:${darkOf.get(name) ?? value}"></span></div>
      <div><code>${name}</code>${withRoles && roles[name] ? `<div class="cat-role">${roles[name]}</div>` : ""}<div class="cat-hex">${value} · ${darkOf.get(name) ?? ""}</div></div>
    </div>`).join("")}</div>`;
}

const foundations = `
<section id="foundations" class="cat-section">
  <h2>Foundations</h2>
  <p class="cat-intro">Nine colour variables on <code>.arc-chat</code>, <code>.arc-sessions</code> and <code>.arc-settings</code>, redefined for dark mode. Each swatch shows light, then dark. Every class starts with <code>arc-</code>; there is one stylesheet, <code>src/styles.css</code>.</p>
  <h3>Colour</h3>
  ${swatches(lightVars, darkVars, true)}
  <h3>Terminal colours</h3>
  ${swatches(lightAnsi, darkAnsi, false)}
  <h3>Type</h3>
  <div class="cat-pair">${["light", "dark"].map((theme) => `<div class="cat-pane cat-${theme}"><div class="cat-label">${theme}</div><div class="arc-chat cat-type">
    <div><span class="cat-meta">Body · 14/1.5 system UI</span><div>The list is sorted by creation time.</div></div>
    <div><span class="cat-meta">Title · 14 semibold</span><div class="arc-status-title">Sort the session list by last update</div></div>
    <div><span class="cat-meta">Small · 13</span><div style="font-size:13px">Conversation compacted (completed)</div></div>
    <div><span class="cat-meta">Meta · 12 muted</span><div class="arc-status-usage">31% of context</div></div>
    <div><span class="cat-meta">Label · 11 uppercase, 0.04em</span><div class="arc-label">Permission</div></div>
    <div><span class="cat-meta">Mono · 13 / 12</span><div class="arc-tool-title">crates/archipel-server/src/sessions.rs</div></div>
  </div></div>`).join("")}</div>
  <h3>Small parts</h3>
  <div class="cat-pair">${["light", "dark"].map((theme) => `<div class="cat-pane cat-${theme}"><div class="cat-label">${theme}</div><div class="arc-chat cat-parts">
    <div class="cat-row"><span class="cat-meta">Badges</span><span class="arc-badge arc-ok">completed</span><span class="arc-badge arc-running">in progress</span><span class="arc-badge arc-failed">failed</span></div>
    <div class="cat-row"><span class="cat-meta">Buttons</span><span class="arc-options"><button type="button">Primary</button><button type="button">Secondary</button><button type="button" disabled>Disabled</button></span></div>
    <div class="cat-row"><span class="cat-meta">Danger</span><span class="arc-sessions" style="padding:0;background:none"><span></span><button type="button" class="arc-danger">Delete</button></span></div>
    <div class="cat-row"><span class="cat-meta">Chip</span><span class="arc-chip">sessions.tsx</span></div>
    <div class="cat-row"><span class="cat-meta">Meter</span><span class="arc-status-usage"><span class="arc-meter"><span style="width:31%"></span></span>31% of context</span><span class="arc-status-usage"><span class="arc-meter arc-meter-high"><span style="width:86%"></span></span>86% of context</span></div>
    <div class="cat-row"><span class="cat-meta">Field</span><label class="arc-field" style="margin:0"><input value="text field" readonly></label></div>
    <div class="cat-row"><span class="cat-meta">Card</span><div class="arc-plan" style="flex:1"><div class="arc-label">Card</div>Border, 10px radius, soft background.</div></div>
  </div></div>`).join("")}</div>
</section>`;

// ---- The page ----

const escape = (value: string) => value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function specimen(entry: Specimen): string {
  let html = renderToStaticMarkup(<>{entry.draw()}</>);
  if (entry.after) html = entry.after(html);
  const frame = (entry.frame ?? "chat") === "chat" ? (inner: string) => `<div class="arc-chat cat-frame">${inner}</div>` : (inner: string) => inner;
  const slug = entry.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return `
  <article class="cat-specimen${entry.wide ? " cat-wide" : ""}" id="${slug}">
    <header>
      <h4>${escape(entry.name)}${entry.byHand ? ' <span class="cat-tag" title="This state only exists after the user acts, so it is drawn by hand with the kit\'s classes.">drawn by hand</span>' : ""}</h4>
      ${entry.code ? `<code class="cat-code">${escape(entry.code)}</code>` : ""}
      ${entry.note ? `<p>${escape(entry.note)}</p>` : ""}
    </header>
    <div class="cat-pair">
      <div class="cat-pane cat-light"><div class="cat-label">light</div>${frame(html)}</div>
      <div class="cat-pane cat-dark"><div class="cat-label">dark</div>${frame(html)}</div>
    </div>
  </article>`;
}

const nav = [`<a href="#foundations">Foundations</a>`, ...sections.map((section) => `<a href="#${section.id}">${section.title}</a>`)].join("");
const body = sections.map((section) => `
<section id="${section.id}" class="cat-section">
  <h2>${section.title}</h2>
  <p class="cat-intro">${escape(section.intro)}</p>
  <div class="cat-grid">${section.specimens.map(specimen).join("")}</div>
</section>`).join("");

const count = sections.reduce((sum, section) => sum + section.specimens.length, 0);
const page = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Archipel components</title>
<style>
${themed(css)}
/* ---- The catalogue's own page, not part of the kit ---- */
:root { --cat-fg: #1c1c1e; --cat-muted: #6b6b70; --cat-bg: #f7f7f9; --cat-card: #ffffff; --cat-line: #e2e2e7; --cat-dark-pane: #1b1b1f; }
* { box-sizing: border-box; }
body { margin: 0; background: var(--cat-bg); color: var(--cat-fg); font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
.cat-top { position: sticky; top: 0; z-index: 10; display: flex; flex-wrap: wrap; align-items: center; gap: 4px 16px; padding: 12px 24px; background: color-mix(in srgb, var(--cat-bg) 92%, transparent); backdrop-filter: blur(8px); border-bottom: 1px solid var(--cat-line); }
.cat-top h1 { margin: 0 12px 0 0; font-size: 15px; }
.cat-top nav { display: flex; flex-wrap: wrap; gap: 2px 12px; font-size: 13px; }
.cat-top a { color: var(--cat-muted); text-decoration: none; } .cat-top a:hover { color: var(--cat-fg); }
main { max-width: 1440px; margin: 0 auto; padding: 8px 24px 64px; }
.cat-lede { color: var(--cat-muted); max-width: 760px; }
.cat-section { scroll-margin-top: 64px; padding-top: 24px; }
.cat-section h2 { margin: 16px 0 4px; font-size: 22px; }
.cat-section h3 { margin: 24px 0 8px; font-size: 14px; color: var(--cat-muted); text-transform: uppercase; letter-spacing: 0.05em; }
.cat-intro { margin: 0 0 16px; color: var(--cat-muted); max-width: 760px; }
.cat-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 640px), 1fr)); gap: 16px; }
.cat-specimen { scroll-margin-top: 64px; background: var(--cat-card); border: 1px solid var(--cat-line); border-radius: 12px; overflow: hidden; }
.cat-specimen.cat-wide { grid-column: 1 / -1; }
.cat-specimen header { padding: 12px 16px 10px; border-bottom: 1px solid var(--cat-line); }
.cat-specimen h4 { margin: 0; font-size: 14px; }
.cat-specimen header p { margin: 4px 0 0; color: var(--cat-muted); font-size: 13px; }
.cat-code { display: block; margin-top: 4px; font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; color: var(--cat-muted); overflow-wrap: anywhere; }
.cat-tag { margin-left: 6px; padding: 1px 6px; border-radius: 4px; background: #fff4d6; color: #7a5600; font-size: 11px; font-weight: 500; cursor: help; }
.cat-pair { display: grid; grid-template-columns: 1fr 1fr; }
@media (max-width: 900px) { .cat-pair { grid-template-columns: 1fr; } }
.cat-pane { position: relative; padding: 28px 16px 16px; min-width: 0; }
.cat-pane.cat-light { background: #ffffff; } .cat-pane.cat-dark { background: var(--cat-dark-pane); }
.cat-label { position: absolute; top: 6px; left: 12px; font: 10px ui-monospace, Menlo, monospace; text-transform: uppercase; letter-spacing: 0.06em; color: #9a9aa3; }
.cat-pane .arc-chat.cat-frame { max-width: none; margin: 0; padding: 0; }
.cat-pane .arc-chat:not(.cat-frame) { max-width: none; }
.cat-pane > .arc-chat:not(.cat-frame):not(.arc-notices) { border: 1px solid var(--arc-line); border-radius: 10px; }
.cat-pane .arc-sessions { border-radius: 10px; }
.cat-hover .arc-session-actions { opacity: 1; }
.cat-inline { display: contents; }
/* Every row is drawn, so the page can be screenshotted or imported whole. */
.arc-row { content-visibility: visible; }
.cat-swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
.cat-swatch { display: flex; gap: 10px; align-items: center; padding: 8px; background: var(--cat-card); border: 1px solid var(--cat-line); border-radius: 10px; }
.cat-swatch code { font: 12px ui-monospace, Menlo, monospace; }
.cat-chip-pair { display: flex; flex: none; border-radius: 8px; overflow: hidden; border: 1px solid var(--cat-line); }
.cat-chip-pair span { width: 28px; height: 40px; }
.cat-role { font-size: 12px; } .cat-hex { font: 11px ui-monospace, Menlo, monospace; color: var(--cat-muted); }
.cat-type, .cat-parts { display: flex; flex-direction: column; gap: 12px; padding: 16px; }
.cat-meta { display: block; font: 10px ui-monospace, Menlo, monospace; color: #9a9aa3; margin-bottom: 2px; }
.cat-row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.cat-row > .cat-meta { width: 64px; margin: 0; }
</style>
</head>
<body>
<header class="cat-top"><h1>Archipel components</h1><nav>${nav}</nav></header>
<main>
<p class="cat-lede">Every component of <code>@archipel/react</code> in each of its states, drawn by the components themselves with the kit's stylesheet, light and dark side by side. ${count} specimens. Generated by <code>ui/react/demo/catalog.tsx</code>; states marked <span class="cat-tag">drawn by hand</span> only exist after the user acts and are copies of the components' markup.</p>
${foundations}
${body}
</main>
</body>
</html>`;

mkdirSync(path.join(here, "out"), { recursive: true });
const out = path.join(here, "out", "catalog.html");
writeFileSync(out, page);
console.log(`wrote ${count} specimens to ${out}`);

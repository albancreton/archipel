// The page of parts.test.ts: one part of the kit per `#name`, with written data. What the
// part asks its app to do is noted in `window.calls`; while `window.refuse` is set the app
// says no, as a daemon would.

import { Conversation, type Json, type SessionSummary } from "@archipel/client";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import denied from "../../../../testdata/states/claude-v2/approval_denied.json" with { type: "json" };
import question from "../../../../testdata/states/claude-v2/question.json" with { type: "json" };
import simple from "../../../../testdata/states/claude-v2/simple.json" with { type: "json" };
import thinking from "../../../../testdata/states/claude-v2/thinking.json" with { type: "json" };
import { AgentStatus, ChatView, Composer, Diff, OpenElsewhere, SessionList, SettingsBar, settingsOf, type Mention } from "../../src/index.ts";

declare global {
  interface Window { calls: Json[][]; refuse?: string }
}
window.calls = [];
/** Notes the call, and fails when the test asked for a refusal. */
const call = (name: string) => async (...args: unknown[]) => {
  window.calls.push([name, ...(args as Json[])]);
  if (window.refuse) throw new Error(window.refuse);
};
const load = (state: unknown) => Conversation.fromJSON(state as Json);

const session = (sessionId: string, title: string, cwd: string): SessionSummary =>
  ({ sessionId, agentId: "claude", title, cwd, updatedAt: "2026-11-24T10:00:00Z", logged: true, state: "idle" }) as SessionSummary;
const files: Mention[] = [
  { uri: "file:///work/main.rs", name: "main.rs", detail: "src" },
  { uri: "file:///work/Makefile", name: "Makefile" },
  { uri: "file:///work/README.md", name: "README.md" },
];

/** A patch with one long unchanged stretch between two changes. */
const patch = [
  "--- a/long.txt", "+++ b/long.txt", "@@ -1,40 +1,40 @@", "-first", "+FIRST",
  ...Array.from({ length: 38 }, (_, at) => ` line ${at + 2}`), "-last", "+LAST", "",
].join("\n");

function Waiting({ state, title }: { state: unknown; title: string }) {
  const conversation = load(state);
  for (const request of conversation.root()![1].requests.values()) request.outcome = null;
  return <ChatView conversation={conversation} actions={{ approve: call("approve"), answer: call("answer") }} key={title} />;
}

function Settings() {
  const [, state] = load(simple).root()!;
  return <SettingsBar settings={settingsOf(state.values.get("configOptions"))} onChange={call("setOption")} />;
}

function Long() {
  const conversation = load(simple);
  const [, state] = conversation.root()!;
  const messages = state.collections.get("agentMessage")!;
  const [, first] = [...messages][0]!;
  state.timeline = [];
  for (let at = 1; at <= 450; at++) {
    messages.set(`m${at}`, { ...(first as object), content: [{ type: "text", text: `message ${at}` }] } as Json);
    state.timeline.push({ target: "agentMessage", id: `m${at}` });
  }
  return <ChatView conversation={conversation} />;
}

function Fork() {
  const [forked, setForked] = useState(false);
  return forked ? <p>On the copy</p> : <OpenElsewhere holder={{ pid: 4242 }} onFork={() => call("fork")().then(() => setForked(true))} />;
}

const parts: Record<string, () => React.ReactNode> = {
  sessions: () => (
    <SessionList
      sessions={[session("a", "Fix the parser", "/work/alpha"), session("b", "Write the docs", "/work/alpha"), session("c", "Try an idea", "/work/beta")]}
      selected="b"
      now={new Date("2026-11-24T10:05:00Z")}
      onOpen={call("open")}
      onRename={call("rename")}
      onDelete={call("delete")}
      onMore={call("more")}
    />
  ),
  // The menus open above the composer, so it needs room above it, as in a chat.
  composer: () => (
    <div style={{ paddingTop: 300 }}><Composer
      running={location.search.includes("running")}
      onSend={call("send")}
      onCancel={call("cancel")}
      commands={[{ name: "deep-research", description: "Research a topic" }, { name: "review", description: "Review the changes" }, { name: "rewind" }]}
      mentions={(query) => files.filter((file) => file.name.toLowerCase().includes(query.toLowerCase()))}
      accepts={{ image: true, files: !location.search.includes("pictures-only") }}
      maxBytes={2000}
    /></div>
  ),
  settings: () => <Settings />,
  approval: () => <Waiting state={denied} title="approval" />,
  question: () => <Waiting state={question} title="question" />,
  long: () => <Long />,
  thinking: () => <ChatView conversation={load(thinking)} />,
  diff: () => <Diff block={{ type: "diff", patch: { text: patch }, changes: [{ path: "long.txt", operation: "modify" }] }} />,
  fork: () => <Fork />,
  agents: () => <AgentStatus agents={[{ agentId: "pi", name: "Pi", readiness: "not_installed", error: "pi-acp: not found" } as never]} onRetry={call("retry")} />,
};

createRoot(document.getElementById("root")!).render(parts[location.hash.slice(1)]?.() ?? <p>No such part</p>);

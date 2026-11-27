// Draws every recorded session to HTML and checks that what was said is on the page.

import { Conversation, type Json } from "@archipel/client";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatView, contentText } from "../src/index.ts";

const states = path.join(import.meta.dirname, "../../../testdata/states");
const load = (name: string) => Conversation.fromJSON(JSON.parse(readFileSync(path.join(states, `${name}.json`), "utf8")));
/** Text as it appears in HTML. */
const escaped = (text: string) =>
  text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#x27;");

for (const set of readdirSync(states)) {
  for (const file of readdirSync(path.join(states, set))) {
    const name = `${set}/${path.basename(file, ".json")}`;
    test(`${name} is drawn with everything that was said`, () => {
      const conversation = load(name);
      // As written, so that the text can be found letter for letter. Markdown has its own tests.
      const html = renderToStaticMarkup(<ChatView conversation={conversation} renderText={(text) => text} />);
      // And as Markdown: every word that was said is still on the page.
      const words = new Set(renderToStaticMarkup(<ChatView conversation={conversation} />).replace(/<[^>]*>/g, " ").match(/[\p{L}\p{N}]{3,}/gu));
      for (const state of conversation.sessions.values()) {
        for (const item of state.collections.get("agentMessage")?.values() ?? []) {
          const said = contentText((item as { content?: Json }).content);
          // A link's address is in its `href`, not in the text.
          for (const word of said.replace(/\]\([^)]*\)/g, " ").match(/[\p{L}\p{N}]{3,}/gu) ?? []) assert.ok(words.has(word), `Markdown lost "${word}"`);
        }
      }
      for (const state of conversation.sessions.values()) {
        for (const target of ["userMessage", "agentMessage"]) {
          for (const item of state.collections.get(target)?.values() ?? []) {
            const said = contentText((item as { content?: Json }).content).trim();
            if (said) assert.ok(html.includes(escaped(said)), `missing ${target}: ${said.slice(0, 60)}`);
          }
        }
        for (const tool of state.collections.get("toolCall")?.values() ?? []) {
          const title = (tool as { title?: string }).title;
          if (title) assert.ok(html.includes(escaped(title)), `missing tool call: ${title}`);
        }
      }
    });
  }
}

test("a pending approval offers its options, and an answered one shows the choice", () => {
  const conversation = load("claude-v2/approval_denied");
  const [, state] = conversation.root()!;
  const [id, request] = [...state.requests][0]!;
  assert.match(renderToStaticMarkup(<ChatView conversation={conversation} />), /arc-answer">No</);

  request.outcome = null;
  let chosen = "";
  const html = renderToStaticMarkup(
    <ChatView conversation={conversation} actions={{ approve: (requestId, optionId) => (chosen = `${requestId}:${optionId}`) }} />,
  );
  assert.match(html, /<button type="button">Yes<\/button>/);
  assert.match(html, /<button type="button">No<\/button>/);
  assert.match(html, /arc-pending/);
  assert.equal(chosen, "", `nothing is chosen by drawing (${id})`);
});

test("a subagent's session is drawn inside its item, and a question shows its answer", () => {
  const nested = renderToStaticMarkup(<ChatView conversation={load("claude-v2/subagent")} />);
  const body = nested.slice(nested.indexOf("arc-subagent-body"));
  assert.ok(body.includes("acp-probe"), "the child session's reply is inside the subagent");
  assert.match(renderToStaticMarkup(<ChatView conversation={load("claude-v2/question")} />), /arc-answer">Red</);
});

test("an item of an unknown kind gets the fallback, or the component the app supplies", () => {
  const conversation = load("claude-v2/simple");
  const [, state] = conversation.root()!;
  state.collections.set("x.acme.checkpoint", new Map([["c1", { checkpointId: "c1", label: "after turn 1" }]]));
  state.timeline.push({ target: "x.acme.checkpoint", id: "c1" });
  assert.match(renderToStaticMarkup(<ChatView conversation={conversation} />), /<summary>x\.acme\.checkpoint<\/summary>/);
  const Checkpoint = ({ item }: { item: Json }) => <div className="checkpoint">{(item as { label: string }).label}</div>;
  const html = renderToStaticMarkup(<ChatView conversation={conversation} components={{ "x.acme.checkpoint": Checkpoint }} />);
  assert.match(html, /<div class="checkpoint">after turn 1<\/div>/);
});

test("a session open elsewhere says so, and offers the fork only when the app can show the copy", () => {
  const conversation = load("claude-v2/simple");
  assert.doesNotMatch(renderToStaticMarkup(<ChatView conversation={conversation} />), /arc-elsewhere/);
  const holder = { pid: 4242, status: "busy" };
  const told = renderToStaticMarkup(<ChatView conversation={conversation} openElsewhere={holder} actions={{ prompt: () => {} }} />);
  assert.match(told, /open in another window of the agent \(pid 4242, busy\)/);
  assert.doesNotMatch(told, /Fork and continue here/);
  assert.match(told, /<textarea/, "the composer stays: sending works again once the other window is closed");
  const offered = renderToStaticMarkup(<ChatView conversation={conversation} openElsewhere={holder} actions={{ fork: () => {} }} />);
  assert.match(offered, /<button type="button">Fork and continue here<\/button>/);
});

test("finished tool calls fold into one group, and a call shows its output only while it runs", () => {
  // Recorded: Read, a command, a thought, Edit (with a diff), its approval, then the answer.
  const conversation = load("claude-v2/files");
  const [, state] = conversation.root()!;
  const done = renderToStaticMarkup(<ChatView conversation={conversation} />);
  assert.equal(done.match(/<summary>(\d+) tool calls/)?.[1], "3");
  const group = done.slice(done.indexOf("arc-tool-group-body"), done.indexOf("</details>", done.lastIndexOf("arc-tool-head")));
  assert.match(group, /Thinking/, "the thought between two calls is in the group");
  assert.match(group, /arc-approved.*arc-answer">Yes/s, "and so is the answered approval of its last call");
  assert.doesNotMatch(done, /arc-item arc-request/, "an approval that has its answer is a line, not a card");
  assert.equal(group.match(/arc-tool-head/g)?.length, 3);
  assert.doesNotMatch(done, /arc-tool-body|arc-diff|arc-terminal/, "what the calls produced is folded away");
  assert.doesNotMatch(done, /arc-badge arc-ok">completed/, "a call that went well has no badge");
  assert.match(done, /aria-expanded="false"/);

  // The edit is still going: it leaves the group and shows its diff.
  const edit = [...state.collections.get("toolCall")!.values()].find((tool) => (tool as { kind: string }).kind === "edit") as { status: string };
  const turn = state.values.get("state") as { state: string };
  edit.status = "in_progress";
  turn.state = "running";
  const going = renderToStaticMarkup(<ChatView conversation={conversation} />);
  assert.equal(going.match(/<summary>(\d+) tool calls/)?.[1], "2");
  assert.match(going, /aria-expanded="true".*arc-badge arc-running">in progress.*arc-tool-body.*arc-diff/s);
  assert.ok(going.indexOf("Thinking") > going.indexOf("</details>"), "the thought after the group's last call is on its own again");

  // A call left as going by a turn that was cut short is over, and one that failed is counted.
  turn.state = "idle";
  assert.equal(renderToStaticMarkup(<ChatView conversation={conversation} />).match(/<summary>(\d+) tool calls/)?.[1], "3");
  edit.status = "failed";
  assert.match(renderToStaticMarkup(<ChatView conversation={conversation} />), /3 tool calls<span class="arc-failed"> · 1 failed<\/span>/);
});

test("answered approvals go into the group of the call they follow", () => {
  // Recorded: four calls and an approval, a plan, a thought, then two calls and two approvals.
  const html = renderToStaticMarkup(<ChatView conversation={load("claude-v2/plan")} />);
  const groups = html.split('<details class="arc-item arc-tool-group">').slice(1).map((part) => part.slice(0, part.indexOf("arc-plan\"") < 0 ? part.indexOf("arc-assistant") : part.indexOf("arc-plan\"")));
  assert.deepEqual(groups.map((part) => [part.match(/<summary>(\d+)/)?.[1], part.match(/arc-tool-head/g)?.length, part.match(/arc-approved/g)?.length]), [["4", 4, 1], ["2", 2, 2]]);
});

test("one tool call alone is not a group", () => {
  assert.doesNotMatch(renderToStaticMarkup(<ChatView conversation={load("claude-v2/command_output")} />), /arc-tool-group/);
});

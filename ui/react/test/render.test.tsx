// Draws every recorded session to HTML and checks that what was said is on the page.

import { Conversation, type Json } from "@harness-bridge/client";
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
      const html = renderToStaticMarkup(<ChatView conversation={conversation} />);
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
  assert.match(renderToStaticMarkup(<ChatView conversation={conversation} />), /hb-answer">No</);

  request.outcome = null;
  let chosen = "";
  const html = renderToStaticMarkup(
    <ChatView conversation={conversation} actions={{ approve: (requestId, optionId) => (chosen = `${requestId}:${optionId}`) }} />,
  );
  assert.match(html, /<button type="button">Yes<\/button>/);
  assert.match(html, /<button type="button">No<\/button>/);
  assert.match(html, /hb-pending/);
  assert.equal(chosen, "", `nothing is chosen by drawing (${id})`);
});

test("a subagent's session is drawn inside its item, and a question shows its answer", () => {
  const nested = renderToStaticMarkup(<ChatView conversation={load("claude-v2/subagent")} />);
  const body = nested.slice(nested.indexOf("hb-subagent-body"));
  assert.ok(body.includes("acp-probe"), "the child session's reply is inside the subagent");
  assert.match(renderToStaticMarkup(<ChatView conversation={load("claude-v2/question")} />), /hb-answer">Red</);
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

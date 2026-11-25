// The agent's text as Markdown, and a long session drawn in pages.

import { Conversation } from "@archipel/client";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatView, Markdown } from "../src/index.ts";

const draw = (text: string) => renderToStaticMarkup(<Markdown text={text} />);

test("Markdown is drawn as elements", () => {
  const html = draw([
    "# Title", "", "Some **bold**, *soft*, ~~gone~~ and `code` text.", "", "- one", "- [x] done", "", "1. first", "2. second", "",
    "> quoted", "", "| a | b |", "|---|:-:|", "| 1 | 2 |", "", "```ts", "const a = 1 < 2 && true;", "```", "", "---",
  ].join("\n"));
  assert.match(html, /<h3>Title<\/h3>/, "headings start below the page's own");
  assert.match(html, /<p>Some <strong>bold<\/strong>, <em>soft<\/em>, <del>gone<\/del> and <code>code<\/code> text\.<\/p>/);
  assert.match(html, /<ul><li><span>one<\/span><\/li><li><input type="checkbox" readOnly="" disabled="" checked=""\/><span>done<\/span>/);
  assert.match(html, /<ol start="1"><li><span>first<\/span><\/li>/);
  assert.match(html, /<blockquote><p>quoted<\/p><\/blockquote>/);
  assert.match(html, /<th>a<\/th><th style="text-align:center">b<\/th>/);
  assert.match(html, /<pre data-language="ts"><code>const a = 1 &lt; 2 &amp;&amp; true;<\/code><\/pre>/);
  assert.match(html, /<hr\/>/);
  assert.equal(draw("a &amp; b < c, `x && y`"), '<div class="arc-md"><p>a &amp; b &lt; c, <code>x &amp;&amp; y</code></p></div>', "characters are neither lost nor doubled");
});

test("text that is still arriving is drawn up to where it ends", () => {
  assert.match(draw("Here:\n\n```sh\nnpm te"), /<p>Here:<\/p><pre data-language="sh"><code>npm te<\/code><\/pre>/, "an open code fence is code");
  assert.match(draw("some **bol"), /<p>some \*\*bol<\/p>/);
  assert.match(draw("- one\n- "), /<ul><li><span>one<\/span><\/li><li><\/li><\/ul>/);
});

test("the agent's text cannot put markup, scripts or requests on the page", () => {
  const html = draw('<script>alert(1)</script>\n\n<img src=x onerror=alert(1)> and <b>bold</b>\n\n[click](javascript:alert(1)) [ok](https://example.com "t") ![pic](https://example.com/p.png)');
  assert.doesNotMatch(html, /<script|<img|<b>|javascript:/i);
  assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/, "markup is shown as text");
  assert.match(html, /<span>click<\/span>/, "a link that is not to the web is only its text");
  assert.match(html, /<a href="https:\/\/example\.com" title="t" target="_blank" rel="noreferrer noopener">ok<\/a>/);
  assert.match(html, /\[image: pic\]/, "an image is named, not fetched");
});

const states = path.join(import.meta.dirname, "../../../testdata/states");

/** A recorded session with `count` more assistant messages. */
function long(count: number): Conversation {
  const conversation = Conversation.fromJSON(JSON.parse(readFileSync(path.join(states, "claude-v2/simple.json"), "utf8")));
  const [, state] = conversation.root()!;
  const messages = state.collections.get("agentMessage")!;
  for (let at = 0; at < count; at += 1) {
    messages.set(`long-${at}`, { messageId: `long-${at}`, content: [{ type: "text", text: `Message **number ${at}** with \`code\`.\n\n- a\n- b` }] });
    state.timeline.push({ target: "agentMessage", id: `long-${at}` });
  }
  return conversation;
}

test("a long session draws its newest items and offers the earlier ones", () => {
  const conversation = long(450);
  const total = conversation.root()![1].timeline.length;
  const html = renderToStaticMarkup(<ChatView conversation={conversation} />);
  assert.equal(html.match(/class="arc-row"/g)?.length, 200);
  assert.match(html, new RegExp(`Show earlier \\(${total - 200} more\\)`));
  assert.match(html, /number 449/);
  assert.doesNotMatch(html, /number 0</);
  const all = renderToStaticMarkup(<ChatView conversation={conversation} pageSize={Infinity} />);
  assert.equal(all.match(/class="arc-row"/g)?.length, total);
  assert.doesNotMatch(all, /Show earlier/);
  assert.doesNotMatch(renderToStaticMarkup(<ChatView conversation={long(3)} />), /Show earlier/);
});

test("how long a session of 5000 items takes to draw (reported, not judged)", () => {
  const conversation = long(5000);
  const time = (pageSize: number) => {
    const started = performance.now();
    renderToStaticMarkup(<ChatView conversation={conversation} pageSize={pageSize} />);
    return Math.round(performance.now() - started);
  };
  time(200); // warm up
  console.log(`      5000 items: ${time(200)} ms for the newest 200, ${time(Infinity)} ms for all`);
});

// Terminal output with its colours, and diffs.

import assert from "node:assert/strict";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AnsiText, Diff, ansiPieces, foldContext, parsePatch } from "../src/index.ts";

const ESC = "\u001b";

test("colour codes become styles and other control codes are dropped", () => {
  assert.deepEqual(ansiPieces(`plain ${ESC}[1;31mred bold${ESC}[0m ${ESC}[92mbright${ESC}[39m ${ESC}[38;5;208morange${ESC}[48;2;1;2;3m on rgb${ESC}[m end`), [
    { text: "plain ", style: {} },
    { text: "red bold", style: { bold: true, fg: 1 } },
    { text: " ", style: {} },
    { text: "bright", style: { fg: 10 } },
    { text: " ", style: {} },
    { text: "orange", style: { fg: "rgb(255,135,0)" } },
    { text: " on rgb", style: { fg: "rgb(255,135,0)", bg: "rgb(1,2,3)" } },
    { text: " end", style: {} },
  ]);
  // Cursor moves, the window title, a bell: gone, and the text around them stays.
  assert.deepEqual(ansiPieces(`a${ESC}[2K${ESC}[1Gb${ESC}]0;title\u0007c\u0007${ESC}[?25ld`).map((piece) => piece.text).join(""), "abcd");
  assert.equal(ansiPieces("10%\r50%\r100%\ndone\r\n").map((piece) => piece.text).join(""), "100%\ndone\n", "a progress line shows its last state");
  assert.deepEqual(ansiPieces(""), []);
  const html = renderToStaticMarkup(<AnsiText text={`ok ${ESC}[32m<b>${ESC}[7m inv ${ESC}[0m`} />);
  assert.equal(html, 'ok <span class="hb-ansi-fg-2">&lt;b&gt;</span><span class="hb-ansi-fg-bg hb-ansi-bg-2"> inv </span>');
});

const patch = [
  "diff --git a/src/a.ts b/src/a.ts", "index 1..2 100644", "--- a/src/a.ts", "+++ b/src/a.ts", "@@ -1,12 +1,12 @@",
  " one", " two", " three", " four", " five", " six", " seven", " eight", "-nine", "+NINE", " ten", " eleven",
  "\\ No newline at end of file",
  "--- /dev/null", "+++ b/new.txt", "@@ -0,0 +1,2 @@", "+hello", "+--- not a header",
].join("\n");

test("a patch is read per file, with line numbers and counts", () => {
  const files = parsePatch(patch);
  assert.deepEqual(files.map(({ path, added, removed }) => [path, added, removed]), [["src/a.ts", 1, 1], ["new.txt", 2, 0]]);
  assert.deepEqual(files[0]!.rows.slice(8, 11), [
    { kind: "context", text: "eight", old: 8, new: 8 },
    { kind: "remove", text: "nine", old: 9 },
    { kind: "add", text: "NINE", new: 9 },
  ]);
  assert.deepEqual(files[1]!.rows[2], { kind: "add", text: "--- not a header", new: 2 });
  assert.deepEqual(parsePatch("not a patch\nat all"), []);
  assert.deepEqual(parsePatch("--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n").map((file) => file.path), ["x"], "a deleted file keeps its name");
});

test("long unchanged parts are folded, short ones are not", () => {
  const rows = foldContext(parsePatch(patch)[0]!.rows);
  assert.deepEqual(rows.map((row) => (row.kind === "fold" ? `fold ${row.rows.length}` : row.kind)),
    ["hunk", "fold 5", "context", "context", "context", "remove", "add", "context", "context"]);
  const html = renderToStaticMarkup(<Diff block={{ type: "diff", changes: [{ operation: "modify", path: "src/a.ts" }], patch: { text: patch } }} />);
  assert.match(html, /modify <code>src\/a\.ts<\/code><span class="hb-added"> \+1<\/span> <span class="hb-removed">−1<\/span>/);
  assert.match(html, /Show 5 unchanged lines/);
  assert.match(html, /<tr class="hb-diff-remove"><td class="hb-diff-number">9<\/td><td class="hb-diff-number"><\/td><td class="hb-diff-text"><span class="hb-diff-sign">-<\/span>nine<\/td><\/tr>/);
  assert.doesNotMatch(html, />one</, "folded lines are not on the page until asked for");
  // What is not a patch is shown as it came.
  assert.match(renderToStaticMarkup(<Diff block={{ changes: [{ operation: "add", path: "x" }], patch: { text: "odd text" } }} />), /add <code>x<\/code><\/div><pre>odd text<\/pre>/);
});

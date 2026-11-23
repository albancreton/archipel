// Draws recorded sessions to standalone HTML files in demo/out, to look at the components
// without a daemon: `pnpm demo`, then open one of the files.

import { Conversation } from "@harness-bridge/client";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatView } from "../src/index.ts";

const here = import.meta.dirname;
const states = path.join(here, "../../../testdata/states");
const css = readFileSync(path.join(here, "../src/styles.css"), "utf8");
mkdirSync(path.join(here, "out"), { recursive: true });
for (const set of readdirSync(states)) {
  for (const file of readdirSync(path.join(states, set))) {
    const conversation = Conversation.fromJSON(JSON.parse(readFileSync(path.join(states, set, file), "utf8")));
    const body = renderToStaticMarkup(<ChatView conversation={conversation} actions={{ prompt: () => {} }} />);
    const name = `${set}-${path.basename(file, ".json")}.html`;
    const page = `<!doctype html><meta charset="utf-8"><title>${name}</title><style>body{margin:0;background:#fff}@media(prefers-color-scheme:dark){body{background:#1b1b1f}}${css}</style>${body}`;
    writeFileSync(path.join(here, "out", name), page);
  }
}
console.log(`wrote ${readdirSync(path.join(here, "out")).length} pages to ${path.join(here, "out")}`);

// Draws recorded sessions to standalone HTML files in demo/out, to look at the components
// without a daemon: `pnpm demo`, then open one of the files.

import { Conversation } from "@archipel/client";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { ChatView } from "../src/index.ts";

const here = import.meta.dirname;
const states = path.join(here, "../../../testdata/states");
// `--theme <name>` uses ui/themes/<name>/react/styles.css and writes to demo/out/<name>.
const at = process.argv.indexOf("--theme");
const theme = at > 0 ? process.argv[at + 1] : undefined;
const css = readFileSync(theme ? path.join(here, `../../themes/${theme}/react/styles.css`) : path.join(here, "../src/styles.css"), "utf8");
const outDir = theme ? path.join(here, "out", theme) : path.join(here, "out");
// A theme's page takes its own background, the first --arc-bg it declares.
const pageBackground = theme ? `body{margin:0;background:${css.match(/--arc-bg:\s*([^;]+);/)?.[1] ?? "#fff"}}` : "body{margin:0;background:#fff}@media(prefers-color-scheme:dark){body{background:#1b1b1f}}";
mkdirSync(outDir, { recursive: true });
for (const set of readdirSync(states)) {
  for (const file of readdirSync(path.join(states, set))) {
    const conversation = Conversation.fromJSON(JSON.parse(readFileSync(path.join(states, set, file), "utf8")));
    const body = renderToStaticMarkup(<ChatView conversation={conversation} actions={{ prompt: () => {} }} />);
    const name = `${set}-${path.basename(file, ".json")}.html`;
    const page = `<!doctype html><meta charset="utf-8"><title>${name}</title><style>${pageBackground}${css}</style>${body}`;
    writeFileSync(path.join(outDir, name), page);
  }
}
console.log(`wrote ${readdirSync(outDir).filter((file) => file.endsWith(".html")).length} pages to ${outDir}`);

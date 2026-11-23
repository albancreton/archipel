// Bundles demo/live.tsx and serves it on http://localhost:5173. Run with `pnpm demo:live`.
import { build } from "esbuild";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";

const here = import.meta.dirname;
const bundle = await build({
  entryPoints: [path.join(here, "live.tsx")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  jsx: "automatic",
  // The Node-only transports are loaded on demand and never in a browser.
  external: ["node:*"],
  define: { "process.env.NODE_ENV": '"production"' },
});
const script = bundle.outputFiles[0].text;
const css = readFileSync(path.join(here, "../src/styles.css"), "utf8");
const page = `<!doctype html><meta charset="utf-8"><title>harness-bridge</title>
<style>body{margin:0;font:14px -apple-system,sans-serif}.demo{display:flex}.demo-sessions{list-style:none;margin:0;padding:8px;width:240px;flex:none}
.demo-sessions button{all:unset;cursor:pointer;display:block;padding:6px 8px;border-radius:6px}.demo-sessions small{display:block;color:#888}
.demo .hb-chat{flex:1}${css}</style><div id="root"></div><script type="module" src="/live.js"></script>`;
const port = Number(process.env.PORT ?? 5173);
createServer((request, response) => {
  if (request.url === "/live.js") {
    response.writeHead(200, { "content-type": "text/javascript" }).end(script);
  } else {
    response.writeHead(200, { "content-type": "text/html" }).end(page);
  }
}).listen(port, "127.0.0.1", () => console.log(`http://localhost:${port}`));

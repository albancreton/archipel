import { registerHooks } from "node:module";
import fs from "node:fs";
const parentOf = new Map();
registerHooks({ resolve(spec, ctx, next) { const r = next(spec, ctx); if (r.url.startsWith("file:") && !parentOf.has(r.url)) parentOf.set(r.url, ctx.parentURL); return r; } });
process.on("exit", () => {
  const short = (u) => u ? fs.realpathSync(new URL(u)).replace(/.*t3code-minitel\//, "").replace(/.*node_modules\//, "npm:") : "";
  const chain = (u) => { const c = []; let x = u; while (x && c.length < 12) { c.push(short(x)); x = parentOf.get(x); } return c.join("\n      <- "); };
  for (const needle of ["source-control-github/src/server", "katex", "effect-acp/src", "tailscale/src", "provider-acp/src"]) {
    const hit = [...parentOf.keys()].find((u) => u.includes(needle)); if (hit) console.error(`\n${needle}:\n      ${chain(hit)}`);
  }
  const rows = [...parentOf.keys()].map(short).filter((p) => p.startsWith("apps/server")).map((p) => [fs.readFileSync("/work/t3code-minitel/" + p, "utf8").split("\n").length, p]).sort((a, b) => b[0] - a[0]);
  console.error("\nSERVER FILES"); for (const [n, p] of rows) console.error(String(n).padStart(6), p.replace("apps/server/src/", ""));
});

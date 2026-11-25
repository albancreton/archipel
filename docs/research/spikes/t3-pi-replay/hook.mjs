import { registerHooks } from "node:module";
import fs from "node:fs";
const seen = new Set();
registerHooks({ load(url, ctx, next) { if (url.startsWith("file:")) seen.add(url); return next(url, ctx); } });
process.on("exit", () => {
  const groups = {};
  for (const u of seen) {
    const p = fs.realpathSync(new URL(u));
    let g = "other";
    const m = p.match(/t3code-minitel\/(packages\/[^/]+|apps\/[^/]+)/);
    if (m) g = m[1]; else if (p.includes("node_modules")) { const n = p.split("node_modules/").pop().split("/"); g = "npm:" + (n[0].startsWith("@") ? n[0] + "/" + n[1] : n[0]); }
    const lines = fs.readFileSync(p, "utf8").split("\n").length;
    groups[g] ??= { files: 0, lines: 0 }; groups[g].files++; groups[g].lines += lines;
  }
  console.error("MODULES " + JSON.stringify(Object.entries(groups).sort((a, b) => b[1].lines - a[1].lines)));
});

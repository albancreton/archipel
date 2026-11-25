import { registerHooks } from "node:module";
import fs from "node:fs";
const seen = new Set();
registerHooks({ load(url, ctx, next) { if (url.startsWith("file:")) seen.add(url); return next(url, ctx); } });
process.on("exit", () => {
  const groups = {}; const server = {};
  for (const u of seen) {
    const p = fs.realpathSync(new URL(u));
    let g = "other";
    const m = p.match(/t3code-minitel\/(packages\/[^/]+|apps\/[^/]+)/);
    if (m) g = m[1]; else if (p.includes("node_modules")) { const n = p.split("node_modules/").pop().split("/"); g = "npm:" + (n[0].startsWith("@") ? n[0] + "/" + n[1] : n[0]); }
    const lines = fs.readFileSync(p, "utf8").split("\n").length;
    groups[g] ??= { files: 0, lines: 0 }; groups[g].files++; groups[g].lines += lines;
    if (g === "apps/server") { const d = (p.split("apps/server/")[1] ?? p).split("/").slice(0, -1).join("/") || "."; server[d] ??= { files: 0, lines: 0 }; server[d].files++; server[d].lines += lines; }
  }
  const top = Object.entries(groups).sort((a, b) => b[1].lines - a[1].lines);
  const npm = top.filter(([k]) => k.startsWith("npm:"));
  console.error("T3 GROUPS"); for (const [k, v] of top.filter(([k]) => !k.startsWith("npm:"))) console.error(`  ${k.padEnd(40)} ${String(v.files).padStart(4)} files ${String(v.lines).padStart(7)} lines`);
  console.error(`NPM: ${npm.length} packages, ${npm.reduce((a, [, v]) => a + v.files, 0)} files, ${npm.reduce((a, [, v]) => a + v.lines, 0)} lines; top: ${npm.slice(0, 6).map(([k, v]) => k.slice(4) + "=" + v.lines).join(", ")}`);
  console.error("apps/server BY DIR"); for (const [k, v] of Object.entries(server).sort((a, b) => b[1].lines - a[1].lines).slice(0, 45)) console.error(`  ${k.padEnd(40)} ${String(v.files).padStart(4)} files ${String(v.lines).padStart(7)} lines`);
});

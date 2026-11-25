// What the browser tests share: a page server, a daemon with a fake agent, and a browser.

import { build } from "esbuild";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { chromium, type Browser, type Page } from "playwright";

const here = import.meta.dirname;
const root = path.join(here, "../../../..");
const css = readFileSync(path.join(here, "../../src/styles.css"), "utf8");

/** Bundles a page and serves it. `port` 0 lets the system choose. */
export async function servePage(entry: string): Promise<{ url: string; close: () => Promise<void> }> {
  const bundle = await build({
    entryPoints: [entry],
    bundle: true,
    write: false,
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    external: ["node:*"],
    define: { "process.env.NODE_ENV": '"production"' },
    logLevel: "silent",
  });
  const script = bundle.outputFiles[0]!.text;
  const page = `<!doctype html><meta charset="utf-8"><title>test</title><style>body{margin:0;font:14px sans-serif}.demo{display:flex}.demo-sessions{width:300px;flex:none}.demo .hb-chat{flex:1}${css}</style><div id="root"></div><script type="module" src="/page.js"></script>`;
  const server: Server = createServer((request, response) => {
    if (request.url === "/page.js") response.writeHead(200, { "content-type": "text/javascript" }).end(script);
    else response.writeHead(200, { "content-type": "text/html" }).end(page);
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise((done) => { server.closeAllConnections(); server.close(() => done()); }),
  };
}

export interface Daemon {
  /** `ws://127.0.0.1:PORT` */
  ws: string;
  token: string;
  /** Stops the daemon. `start` brings it back on the same address with the same files. */
  stop: () => Promise<void>;
  start: () => Promise<void>;
  /** Stops it and removes its files. */
  close: () => Promise<void>;
}

/**
 * Starts the daemon with a fake agent that replays the recorded session `transcript`
 * (a name in `testdata/transcripts`), reachable by pages served from `origin`.
 */
export async function startDaemon(transcript: string, origin: string): Promise<Daemon> {
  const program = path.join(root, "target/debug/bridged");
  if (!existsSync(program)) throw new Error(`${program} is missing: run \`cargo build\` first`);
  const home = mkdtempSync(path.join(tmpdir(), "hb-ui-"));
  let child: ChildProcess | undefined;
  let address = "127.0.0.1:0";
  const start = async () => {
    const started = spawn(program, [
      "--home", home, "--ws", address, "--ws-origin", origin,
      "--fake", path.join(root, "testdata/transcripts", `${transcript}.ndjson`), "--fake-speed", "0",
    ], { stdio: ["ignore", "ignore", "pipe"] });
    child = started;
    const said: string[] = [];
    address = await new Promise<string>((found, failed) => {
      createInterface(started.stderr!).on("line", (line) => {
        said.push(line);
        const at = line.split("listening on ws://")[1];
        if (at) found(at.split(" ")[0]!);
      });
      started.on("exit", () => failed(new Error(`the daemon stopped: ${said.join("\n")}`)));
    });
  };
  const stop = async () => {
    const running = child;
    child = undefined;
    if (!running || running.exitCode !== null) return;
    await new Promise((done) => { running.on("exit", done); running.kill(); });
  };
  await start();
  return {
    get ws() { return `ws://${address}`; },
    token: readFileSync(path.join(home, "ws-token"), "utf8").trim(),
    stop,
    start,
    close: async () => { await stop(); rmSync(home, { recursive: true, force: true }); },
  };
}

let shared: Promise<Browser> | undefined;

/** A fresh page in the one browser of this test file. A script error in the page fails the test that follows. */
export async function openPage(url: string): Promise<Page> {
  shared ??= chromium.launch();
  const page = await (await shared).newPage();
  page.setDefaultTimeout(10_000);
  page.on("pageerror", (error) => { throw error; });
  await page.goto(url);
  return page;
}

export async function closeBrowser(): Promise<void> {
  await (await shared)?.close();
  shared = undefined;
}

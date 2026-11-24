// Start the daemon from an app, if it is not already running.
//
// Use this when your app should work without an install step: nobody has set up `bridged`
// as a service, so the app starts it. If a daemon already answers on the socket (a service,
// or another app that started one), that one is used and nothing is started. There is never
// a second daemon on one home folder.
//
//   import { ensureDaemon } from "./daemon.ts";
//   const daemon = await ensureDaemon({ home, command: "/path/to/bridged" });
//   const client = await BridgeClient.connect({ open: () => unixSocketTransport(daemon.socket), name: "my-app" });
//
// Who stops it: by default the daemon this call started stops when the app exits, and its
// sessions stop with it. With `keep: true` it goes on after the app, like a service that
// nobody installed; say so to your user if you do that.

import { unixSocketTransport } from "@harness-bridge/client";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync } from "node:fs";
import path from "node:path";

export interface DaemonOptions {
  /** The daemon's folder: its socket, log and config. Default: `~/.harness-bridge`. */
  home?: string;
  /** The daemon program. Default: `bridged` from PATH. */
  command?: string;
  /** More arguments, such as `["--fake", recording]` for a daemon that costs nothing. */
  args?: string[];
  /** Leave the daemon running when the app exits. */
  keep?: boolean;
  /** How long to wait for a started daemon to answer. Default 10 seconds. */
  timeoutMs?: number;
}

export interface Daemon {
  home: string;
  socket: string;
  /** The process, if this call started the daemon. `null` if one was already running. */
  started: ChildProcess | null;
  /** Stops the daemon if this call started it. */
  stop(): void;
}

/** Whether a daemon answers on the socket. */
async function answers(socket: string): Promise<boolean> {
  try {
    (await unixSocketTransport(socket)).close();
    return true;
  } catch {
    return false;
  }
}

export async function ensureDaemon(options: DaemonOptions = {}): Promise<Daemon> {
  const home = options.home ?? path.join(process.env["HOME"] ?? "", ".harness-bridge");
  const socket = path.join(home, "daemon.sock");
  if (await answers(socket)) return { home, socket, started: null, stop: () => {} };

  mkdirSync(home, { recursive: true, mode: 0o700 });
  let log = "";
  const child = spawn(options.command ?? "bridged", ["--home", home, ...(options.args ?? [])], {
    // The daemon logs to stderr. Keep the end of it to explain a failed start.
    stdio: ["ignore", "ignore", options.keep ? "ignore" : "pipe"],
    detached: options.keep === true,
  });
  child.stderr?.on("data", (chunk) => (log = (log + chunk).slice(-2000)));
  let failure: string | null = null;
  child.once("error", (error) => (failure = `could not start the daemon: ${error.message}`));
  child.once("exit", (code) => (failure ??= `the daemon stopped at once (exit ${code}): ${log.trim()}`));

  const stop = () => void child.kill();
  if (options.keep) child.unref();
  else process.once("exit", stop);

  const deadline = Date.now() + (options.timeoutMs ?? 10_000);
  while (!(await answers(socket))) {
    if (failure) throw new Error(failure);
    if (Date.now() > deadline) {
      stop();
      throw new Error(`the daemon did not answer on ${socket}`);
    }
    await new Promise((done) => setTimeout(done, 50));
  }
  return { home, socket, started: child, stop };
}

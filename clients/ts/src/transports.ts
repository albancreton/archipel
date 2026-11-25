// The two ways to reach a daemon: its WebSocket, and its local socket (Node only).

import type { Json } from "./json.ts";
import type { Transport } from "./connection.ts";

export interface WebSocketOptions {
  /** The daemon's token for this client. */
  token: string;
  /**
   * The WebSocket constructor to use. The default is the global one.
   *
   * The token travels in an `Authorization` header, which a browser's WebSocket cannot set.
   * Node, Electron's main process and React Native can; pass theirs if it is not the global.
   * For a page in a browser, see `subprotocol`.
   */
  WebSocket?: unknown;
  /**
   * Send the token as a WebSocket subprotocol instead of a header. This is the only way from a
   * page in a browser, and the daemon accepts it only from origins it was started with
   * (`--ws-origin`).
   */
  subprotocol?: boolean;
}

/** Opens the daemon's WebSocket, such as `ws://127.0.0.1:7890`. */
export function webSocketTransport(url: string, options: WebSocketOptions): Promise<Transport> {
  const Socket = (options.WebSocket ?? (globalThis as { WebSocket?: unknown }).WebSocket) as
    | (new (url: string, protocols?: unknown, more?: unknown) => WebSocket)
    | undefined;
  if (!Socket) return Promise.reject(new Error("no WebSocket implementation; pass one in the options"));
  const headers = { Authorization: `Bearer ${options.token}` };
  // Node takes `{headers}` as the second argument, React Native as the third.
  const socket = options.subprotocol
    ? new Socket(url, [`archipel.token.${options.token}`, "archipel"])
    : new Socket(url, { headers } as unknown, { headers } as unknown);
  return new Promise((resolve, reject) => {
    const transport: Transport = {
      send: (message) => socket.send(JSON.stringify(message)),
      close: () => socket.close(),
      onMessage: null,
      onClose: null,
    };
    let open = false;
    socket.onopen = () => {
      open = true;
      resolve(transport);
    };
    socket.onmessage = (event: MessageEvent) => {
      try {
        transport.onMessage?.(JSON.parse(String(event.data)) as Json);
      } catch {
        // Not JSON: nothing the daemon sends.
      }
    };
    socket.onclose = () => {
      if (open) transport.onClose?.();
      else reject(new Error(`could not connect to ${url} (refused, or wrong token)`));
    };
    socket.onerror = () => {};
  });
}

/** Opens the daemon's local socket: one JSON message per line. Node only. */
export async function unixSocketTransport(path: string): Promise<Transport> {
  const net = await import("node:net");
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(path);
    const transport: Transport = {
      send: (message) => void socket.write(JSON.stringify(message) + "\n"),
      close: () => void socket.end(),
      onMessage: null,
      onClose: null,
    };
    let buffered = "";
    socket.setEncoding("utf8");
    socket.once("connect", () => resolve(transport));
    socket.once("error", reject);
    socket.on("data", (chunk: string) => {
      buffered += chunk;
      let end: number;
      while ((end = buffered.indexOf("\n")) >= 0) {
        const line = buffered.slice(0, end);
        buffered = buffered.slice(end + 1);
        if (!line.trim()) continue;
        try {
          transport.onMessage?.(JSON.parse(line) as Json);
        } catch {
          // Not JSON: skip, as the daemon does.
        }
      }
    });
    socket.once("close", () => transport.onClose?.());
  });
}

/**
 * Carries the connection on a command's stdin and stdout, such as
 * `ssh host archipel connect --stdio`. Node only.
 */
export async function commandTransport(command: string, args: string[] = []): Promise<Transport> {
  const { spawn } = await import("node:child_process");
  const child = spawn(command, args, { stdio: ["pipe", "pipe", "inherit"] });
  const transport: Transport = {
    send: (message) => void child.stdin.write(JSON.stringify(message) + "\n"),
    close: () => void child.kill(),
    onMessage: null,
    onClose: null,
  };
  let buffered = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    buffered += chunk;
    let end: number;
    while ((end = buffered.indexOf("\n")) >= 0) {
      const line = buffered.slice(0, end);
      buffered = buffered.slice(end + 1);
      try {
        if (line.trim()) transport.onMessage?.(JSON.parse(line) as Json);
      } catch {
        // Not JSON: a login banner or the like.
      }
    }
  });
  child.once("exit", () => transport.onClose?.());
  return new Promise((resolve, reject) => {
    child.once("spawn", () => resolve(transport));
    child.once("error", reject);
  });
}

/**
 * Exchanges a one-time code from `archipeld pair` for a token of the app's own. Keep the token
 * somewhere safe (the platform's keychain) and use it with `webSocketTransport` from then on.
 */
export async function pair(
  url: string,
  code: string,
  options: { name?: string; WebSocket?: unknown; subprotocol?: boolean } = {},
): Promise<{ token: string; scope: string; name: string }> {
  const transport = await webSocketTransport(url, { ...options, token: `pair:${code}` });
  return new Promise((resolve, reject) => {
    transport.onClose = () => reject(new Error("the daemon closed the connection"));
    transport.onMessage = (message) => {
      const answer = message as { result?: { token: string; scope: string; name: string }; error?: { message: string } };
      transport.onClose = null;
      transport.close();
      if (answer.result) resolve(answer.result);
      else reject(new Error(answer.error?.message ?? "pairing failed"));
    };
    transport.send({ jsonrpc: "2.0", id: 1, method: "_archipel/pair", params: options.name ? { name: options.name } : {} });
  });
}

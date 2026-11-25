// JSON-RPC over any transport that carries whole messages.

import { isObject, type Json } from "./json.ts";

/** Something that carries JSON messages to the daemon and back. */
export interface Transport {
  send(message: Json): void;
  close(): void;
  /** Set by the connection. Called for every message from the daemon. */
  onMessage: ((message: Json) => void) | null;
  /** Set by the connection. Called once when the transport is gone. */
  onClose: (() => void) | null;
}

/** The daemon or the agent refused a request. */
export class RpcError extends Error {
  code: number;
  /** What the error says beyond its message. For -32004: who has the session open. */
  data: Json;
  constructor(code: number, message: string, data: Json = null) {
    super(message);
    this.name = "RpcError";
    this.code = code;
    this.data = data;
  }

  /** The session is open in another process of the harness. Fork it to continue. */
  get openElsewhere(): boolean {
    return this.code === -32004;
  }

  /**
   * The daemon could not write its session log, for example because the disk is full. A prompt
   * refused with it did not reach the agent. The message says what the storage reported.
   */
  get storage(): boolean {
    return this.code === -32005;
  }
}

/** The connection closed before a request was answered. */
export class ClosedError extends Error {
  constructor() {
    super("the daemon closed the connection");
    this.name = "ClosedError";
  }
}

export class Connection {
  private nextId = 1;
  private waiting = new Map<number, { resolve: (result: Json) => void; reject: (error: Error) => void }>();
  private transport: Transport;
  closed = false;
  /** Called for every message that is not the response to a `call`: updates and requests. */
  onMessage: (message: Json) => void = () => {};
  /** Called once when the connection is gone. */
  onClose: () => void = () => {};
  /** Called with every message sent, before it goes out. */
  onSend: (message: Json) => void = () => {};

  constructor(transport: Transport) {
    this.transport = transport;
    transport.onMessage = (message) => this.receive(message);
    transport.onClose = () => {
      this.closed = true;
      for (const { reject } of this.waiting.values()) reject(new ClosedError());
      this.waiting.clear();
      this.onClose();
    };
  }

  /** Sends a request and resolves with its result. */
  call(method: string, params: Json = {}): Promise<Json> {
    if (this.closed) return Promise.reject(new ClosedError());
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.send({ jsonrpc: "2.0", id, method, params });
    });
  }

  /** Sends a notification, such as `session/cancel`. */
  notify(method: string, params: Json = {}): void {
    this.send({ jsonrpc: "2.0", method, params });
  }

  /** Answers a request from the agent, such as an approval. */
  answer(requestId: Json, result: Json): void {
    this.send({ jsonrpc: "2.0", id: requestId, result });
  }

  /** Refuses a request from the daemon. */
  refuse(requestId: Json, message: string, code = -32601): void {
    this.send({ jsonrpc: "2.0", id: requestId, error: { code, message } });
  }

  close(): void {
    this.transport.close();
  }

  private send(message: Json): void {
    this.onSend(message);
    this.transport.send(message);
  }

  private receive(message: Json): void {
    if (isObject(message) && message["method"] === undefined && typeof message["id"] === "number") {
      const waiting = this.waiting.get(message["id"]);
      if (waiting) {
        this.waiting.delete(message["id"]);
        const error = message["error"];
        if (isObject(error)) {
          waiting.reject(new RpcError(Number(error["code"] ?? 0), String(error["message"] ?? "refused"), error["data"] ?? null));
        } else {
          waiting.resolve(message["result"] ?? null);
        }
        // The response may also change state (config options in `session/new`).
        this.onMessage(message);
        return;
      }
    }
    this.onMessage(message);
  }
}

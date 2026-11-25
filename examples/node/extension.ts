// An extension: a separate program that adds its own kind of item to sessions and serves
// its own methods, without any change to the daemon.
//
//   node extension.ts [--home DIR]
//
// This one keeps notes. An app calls `_x/notes/add` with a session and a text; the
// extension adds an `x.notes.note` item to that session. Every attached app gets the item,
// it is in the log, and it is replayed to apps that attach later.
//
// An extension is for what can live outside the daemon, in any language. To change or stop
// what the harness and the apps say to each other, write an addon instead
// (`examples/own-daemon`, `docs/design/addons.md`).
//
// The rules, all visible below:
// - It connects like an app. The connection needs the `extend` scope; the local socket has it.
// - It registers one namespace. Its item kinds start with `x.<namespace>.`, its methods
//   with `_x/<namespace>/`.
// - An item carries a state rule that tells every client how to hold it. Without one the
//   update is kept as an event and is not placed among the items.

import { Connection, unixSocketTransport, type Json } from "@archipel/client";
import path from "node:path";
import { fileURLToPath } from "node:url";

const NAMESPACE = "notes";

export async function startExtension(socket: string): Promise<Connection> {
  const daemon = new Connection(await unixSocketTransport(socket));
  await daemon.call("initialize", { protocolVersion: 2, info: { name: "notes-extension", version: "0.0.0" } });
  await daemon.call("_archipel/extension/register", { namespace: NAMESPACE });

  let count = 0;
  daemon.onMessage = (message) => {
    const { id, method, params } = message as { id?: Json; method?: string; params?: Record<string, Json> };
    // The daemon passes on the calls apps make of this namespace. Everything else it sends
    // (session updates, if the extension attaches to sessions) is not a request.
    if (id === undefined || method === undefined) return;
    if (method !== `_x/${NAMESPACE}/add` || typeof params?.["sessionId"] !== "string") {
      daemon.refuse(id, "notes: unknown method, or no sessionId");
      return;
    }
    count += 1;
    const update = { sessionUpdate: `x.${NAMESPACE}.note`, noteId: `note-${count}`, text: params["text"] ?? "" };
    // "upsert by noteId": a later update with the same noteId changes this item.
    const state = { action: "upsert", id: "noteId" };
    daemon
      .call("_archipel/session/append", { sessionId: params["sessionId"], update, state })
      .then((added) => daemon.answer(id, { noteId: update.noteId, seq: (added as { seq: number }).seq }))
      .catch((error: Error) => daemon.refuse(id, error.message, -32603));
  };
  return daemon;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const at = process.argv.indexOf("--home");
  const home = at < 0 ? path.join(process.env["HOME"] ?? "", ".archipel") : process.argv[at + 1]!;
  const daemon = await startExtension(path.join(home, "daemon.sock"));
  daemon.onClose = () => process.exit(0);
  console.log(`notes extension running; apps call _x/${NAMESPACE}/add`);
}

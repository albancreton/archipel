# TypeScript client SDK

`@harness-bridge/client`: everything an app needs to follow and drive sessions on the daemon. For Node, Electron and React Native. No runtime dependency; the source runs as TypeScript on Node 22.18 or later.

```ts
import { BridgeClient, unixSocketTransport } from "@harness-bridge/client";

const client = await BridgeClient.connect({
  open: () => unixSocketTransport(`${process.env.HOME}/.harness-bridge/daemon.sock`),
  name: "my-app",
});
const sessions = await client.listSessions();          // the daemon's and the harness's own
const session = await client.session(sessions[0].sessionId);
session.subscribe(() => console.log(session.turnState, session.items().length));
await session.prompt("Run the tests");
for (const { requestId } of session.pendingRequests()) session.approve(requestId, "allow-once");
```

| File | What |
|---|---|
| `src/state.ts` | The reducer: applies ACP v2 messages by following the state rules. A port of the Rust core |
| `src/mirror.ts` | What a client of the daemon holds, and the daemon's additions (sequence numbers, answered requests) |
| `src/client.ts` | `BridgeClient` and `Session`: attach, reconnect, prompt, answer |
| `src/transports.ts` | The daemon's WebSocket and its local socket |
| `src/generated/` | The state rules and the types of the daemon's extensions, written by `just gen` |

A browser page cannot set the header the WebSocket token travels in; see `subprotocol` in `transports.ts` and the daemon's `--ws-origin`.

`pnpm test` checks the reducer against `testdata/states` (every recorded session must give exactly the reference state) and runs the client against the real daemon (`cargo build` first).

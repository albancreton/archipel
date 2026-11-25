# Milestone 6: client SDKs and UI kits

_2026-11-22. Checked by `cargo test --workspace` (41), `pnpm -r test` (77) and `swift test` (6)._

## What exists

| Package | Where | For |
|---|---|---|
| `@archipel/client` | `clients/ts` | Node, Electron, React Native |
| `@archipel/react` | `ui/react` | React |
| `ArchipelClient` | `clients/swift` | macOS, iOS |
| `ArchipelUI` | `ui/swift` | SwiftUI |

Each SDK has the reducer, the mirror, two transports (the daemon's WebSocket and its local socket), and a client whose sessions stay in step with the daemon and re-attach from their last sequence number after a disconnect. Each UI kit has one component per kind of item, a chat view assembled from them, and a way to plug in a component for an extension's kind.

## How three implementations are kept identical

The reducer now exists in Rust, TypeScript and Swift. The design feared they would drift. What prevents it:

- **The rules are data.** All three read the same `state-rules.json`, extracted from the ACP v2 schema. None has code per message kind. The interpreter is about 150 lines in each language.
- **The reference state is data.** The Rust core writes the exact state every recording ends in as JSON (`testdata/states`, 29 sessions). The TypeScript and Swift tests apply the same recordings and must get exactly that.
- **So is the daemon's side.** `testdata/daemon` holds what an app sees through the daemon for seven sessions, live and as a replay, and the state both must end in.
- **And the real thing.** Each SDK has a test that starts the daemon program with the fake agent, drives a session with an approval over the local socket, reads it back over the WebSocket, and compares with the same reference.

This is stricter than the text renderings used so far, and it needed no port of the renderer.

## Bugs the comparison found

- **In the Rust daemon:** the JSON parser rounded one floating-point number (`0.009103865000000001` became `0.009103865`). The daemon was not passing content through unchanged. Fixed by turning on exact float parsing.
- **In the Swift port:** a `null` field in an update must clear that field. A convenience in my JSON type made `condition ? nil : value` store a JSON null instead. Found on two recordings, fixed, and the convenience removed.

Also new: the Rust tests now assert, for all 28 sessions, that a replay through the daemon gives exactly the live state, not only the same rendering.

## The same session, twice

`claude-v2/files` drawn by the React kit (`img/17-react-files.png`) and by the SwiftUI kit (`img/17-swiftui-files.png`), from the same state: user message, a read with its result, a command with its output and exit code, folded reasoning, an edit with its diff, the approval and its answer, the reply.

The React picture is the test page rendered in a headless browser; the SwiftUI one is drawn off screen by the test.

## Decisions

**No build step for TypeScript.** The packages are TypeScript source that Node runs directly. Tests are `node --test`. A bundler can come when something needs one.

**Types are generated for the daemon's extensions only.** The generator could not read the ACP v2 schema, and the reducer does not need ACP types since it follows the rules. An app that wants them can take them from the ACP SDK.

**UI kits take state, not connections.** `ChatView` draws a conversation and calls back. `useSession` (React) and `LiveChatView` (SwiftUI) are the thin parts that connect it to a session. So the components work on stored sessions, in tests, and with an app's own client.

**Swift objects are observable; TypeScript ones are subscribed to.** Each follows its platform.

## Not done

- **A browser page cannot connect yet.** The token travels in a header a browser's WebSocket cannot set. The TypeScript transport already has the alternative (token as a subprotocol); the daemon side is in milestone 7.
- **React Native was not run.** The SDK has no Node dependency outside the local-socket transport, and passes headers the way React Native expects, but only Node was tested.
- **iOS was not run.** The package declares it; only macOS was built and tested.
- **Markdown is not rendered** in either kit; text is shown as written.
- **No virtualised list** for very long sessions.
- **The SwiftUI demo app was built, not looked at.** Its views were (the render test).
- **Publishing** (npm, a tagged Swift release) is not set up.

# archipel, for coding agents

Read this first if you were asked to build something with this library, or to change it.

## What it is

A library for apps that show and drive the agent loop of terminal coding harnesses (Claude Code, Pi): messages, reasoning, tool calls, approvals, questions, plans, subagents.

- A **daemon** (`archipeld`, Rust) runs the harnesses, logs everything they say, and keeps sessions alive while apps come and go.
- **Apps** connect to it with a client SDK (TypeScript or Swift) and may draw sessions with a component kit (React, SwiftUI or GPUI).
- The messages are **ACP v2** plus a few additions under `_archipel/`.

## Start from an example

Each of these is small. Copy the one that matches the task. The last column says how far the repository's own tests check it.

| To do this | Start from | Checked by |
|---|---|---|
| Follow and drive a session from Node, Electron or React Native | `examples/node/chat.ts` | A test runs it against the fake agent |
| Start the daemon from the app when none is running | `examples/node/daemon.ts` | A test starts a daemon with it, then finds that one |
| The same from a macOS app in Swift | `examples/swift/Daemon.swift` | A test starts a daemon with it, then finds that one |
| Add an item kind or a method from a separate program, in any language | `examples/node/extension.ts` | A test runs it and reads its item in an app |
| Host the daemon inside a Rust program | `examples/embedded-daemon` | Its own test, in-process and through the socket |
| Change, stop or answer what the harness and the apps say to each other; fix a harness fault | `examples/own-daemon` (an addon) | Its own test |
| Draw a session in a browser with React | `ui/react/demo/live.tsx` | Tests click through it in a real browser against the fake agent |
| Draw a session in a macOS or iOS app with SwiftUI | `ui/swift/Demo/ArchipelChatDemo.swift` | Compiled only. Tests press the views it uses |
| Draw a session in a GPUI app (Rust) | `ui/gpui/live/examples/chat.rs` | Compiled, and started once by hand against the fake agent. A test drives the chat it uses through a whole turn against the fake agent |
| Run the daemon as a service (launchd, systemd) | `examples/service/` | The launchd file is linted, and was loaded and exercised by hand once. The systemd file was never loaded |

How to run each is at the top of the file, or in the README next to it.

The three ways to run the daemon, and when each fits:

| Way | Sessions live as long as | Choose it when |
|---|---|---|
| A service | The machine is on | Sessions must outlive every app, or the machine is reached remotely |
| Started by the app | The app runs (or longer, if asked) | The app should work without an install step |
| Inside a Rust program | That program runs | You want one binary and are writing Rust |

## Check your work without spending money

A **fake agent** replays a recorded session, so an integration can be run end to end with no model and no account:

```sh
cargo build
target/debug/archipeld --home /tmp/try --fake testdata/transcripts/claude-v2/approval_command.ndjson --fake-speed 0 &
node examples/node/chat.ts --home /tmp/try --mode default --approve allow-once "anything"
```

- The recordings are in `testdata/transcripts/claude-v2/` (17 of them: `simple`, `approval_command`, `question`, `plan`, `subagent`, `interrupt`, …).
- The fake agent follows its recording: it waits for the kinds of messages the recording has, in that order, and answers with what was recorded. What you send does not matter; the order of the steps does. `approval_command` is: new session, set the mode, one prompt, one approval. If your program hangs against the fake, it sent a step the recording does not have or skipped one.
- Use a real harness only when the task depends on its behaviour, and then its cheapest model. `conformance/` holds the live checks.

## Build and test

| Language | Check | Test |
|---|---|---|
| Rust | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` | `cargo test --workspace` |
| TypeScript | `pnpm -r check` | `pnpm -r test` (run `cargo build` first: several tests start `target/debug/archipeld`) |
| React in a browser | | `pnpm test:browser` in `ui/react` (after `cargo build`, and once `npx playwright install chromium-headless-shell`) |
| Swift | | `swift test` (also after `cargo build`) |
| GPUI kit (a workspace of its own, in `ui/gpui`) | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` there | `cargo test --workspace` there (also after `cargo build` at the root) |

On Linux the build needs the OpenSSL headers and `pkg-config` (`apt install libssl-dev pkg-config` on Debian and Ubuntu); the Rust client uses the system's TLS for `wss://` addresses. macOS needs nothing extra.

**Releases.** The version is `version` under `[workspace.package]` in `Cargo.toml`; `archipeld --version` and `archipel --version` print it. Pushing the tag `v<version>` makes `.github/workflows/release.yml` publish `archipeld` and `archipel` for Linux (x86-64 and ARM, static) and macOS (Apple silicon and Intel) with a `SHA256SUMS` file. `scripts/release-build.sh` is what it runs, and builds the same archives on a Mac. The version is also in the recorded daemon output (`testdata/daemon`), so after changing it run `UPDATE_VECTORS=1 cargo test -p archipel-server --test daemon` and check that nothing else moved.

Node 22.18 or later runs the TypeScript sources directly; there is no build step. `just` lists the same commands if it is installed.

## What the types do not tell you

**Sessions and items**
- A session's content is a list of **items** in order (`session.items()`), each with a kind (`userMessage`, `agentMessage`, `agentThought`, `toolCall`, `terminal`, `plan`, `subagent`, …) and an id. Items change while they stream. Draw from the current state after each change; do not append per update.
- Draw a kind you do not know as a generic item, do not drop it. Extensions and addons add kinds named `x.<namespace>.<name>`.
- `prompt` resolves when the agent has taken the message, not when the turn is over. The turn is over when `turnState` is `idle`.
- Text is the agent's Markdown source. The kits draw it as Markdown without ever building HTML from it; an app that draws text itself must not insert it as HTML either.

**Approvals and questions**
- They arrive as requests (`session.pendingRequests()`), and the turn waits in `requires_action`. Every attached app gets them; the first answer counts and the others are told.
- A request stays open while no app is attached. An app that attaches later gets it.

**Connections**
- The local socket (`~/.archipel/daemon.sock`) gives full control and is readable only by its owner. A WebSocket connection is as limited as its token: `observe` reads, `interact` also acts, `extend` also acts as an extension, `admin` also deletes sessions.
- A session can be deleted by another app at any time. The SDKs then set `removed` on it, end any `until` wait, and stop following it (`onSessionRemoved` in TypeScript; in Swift it also leaves `client.sessions`). What it held stays readable; acting on it fails.
- After a disconnect the SDKs reconnect and continue from the last item they had. An app does not replay by hand.
- A browser page needs the daemon started with `--ws-origin` for its origin, and sends its token as a subprotocol (`docs/remote.md`).

**Harnesses**
- Claude Code is driven through an adapter that is an npm package and needs Node 22 or later: `npm install -g @agentclientprotocol/claude-agent-acp`. With no `config.json`, the daemon starts `claude-agent-acp` from `PATH`, so after that one command a real session works. `archipeld --help` documents `config.json`.
- **`archipel agents` is the first thing to run against a real daemon.** It starts every agent and prints `ready`, `needs_login`, or `error` with the reason, and fails unless all are ready. The daemon only learns whether an agent can start when it first needs it, so without this a missing adapter shows up as a failed first message.
- **Not every harness can do everything.** `client.agents()` gives each agent's `capabilities` once it has started: `session.title` (rename, `client.setTitle`), `session.delete` (`client.deleteSession`), `session.options` (settings before a session exists, `client.sessionOptions`). Claude Code has all three; Pi can neither rename nor delete. A call the harness lacks fails with code -32601, so check before showing the button.
- A new session uses the account's default model, which may be an expensive one. Set the model before the first message when cost matters: `session.setConfigOption("model", "haiku")`.
- A daemon started by a service manager does not have your shell's `PATH`. If every agent fails to start, that is why (`examples/service/README.md`).
- A session that another process of the harness has open (its own terminal UI, say) is refused with error `-32004`. Offer a fork (`session.fork()`); the kits already do.
- When the daemon cannot write its log (a full disk), a prompt is refused with error `-32005` (`error.storage` in the SDKs) and the agent never gets it. If that starts in the middle of a turn, `session.error` is set until updates arrive again; the kits' chat views show both.
- `client.agents()` reports each agent as `not_started`, `starting`, `ready`, `needs_login`, `not_installed` or `error`. `not_started` is not a fault: the daemon starts an agent on the first request that needs it.
- For a long session list, `sessionsPage({ limit, cursor })` returns a page and a `nextCursor`. `listSessions()` still returns everything.
- A session started in the harness's own UI shows up in the session list and is loaded the first time it is opened.
- Pi runs one process per session and is stopped when idle.
- When the daemon or an adapter stops, running turns end as interrupted. Sessions and their history stay, and the next message starts the adapter again.

## If you change the library itself

- **A fix for something a harness, an adapter or ACP gets wrong is an addon, never a change to the daemon's core.** So is an optional feature. Only bugs of this library are fixed in the core. `docs/design/addons.md` has the interface; `crates/archipel-addons` has the shipped ones, one file each.
- The Rust core, the TypeScript SDK and the Swift SDK hold the same state for the same messages. `testdata/states` is the reference all three are tested against. After changing the schema run `just gen` (or the two commands under `gen` in the `justfile`).
- An example that no test runs goes stale without anyone noticing. Add the test with the example, and say in the table above how far it is checked.
- Decisions and their reasons are written down in `docs/research/NN-*.md`, and commits say why. Start with `docs/research/19-where-things-stand.md`, then `docs/design/daemon.md`.
- Two scripts say whether the addon system's tests are enough: `scripts/addon-coverage.sh` and `scripts/addon-mutations.py`.

## Where things are

| Path | What |
|---|---|
| `crates/` | The Rust workspace. `archipeld` is the daemon program, `archipel` a command-line client (both have `--help`), `archipel-server` the daemon as a library |
| `clients/ts`, `clients/swift` | The client SDKs. Their READMEs have a ten-line example and a file table |
| `ui/react`, `ui/swift`, `ui/gpui` | The component kits. `ui/gpui` is a Cargo workspace of its own, so the daemon's build does not carry GPUI; its `live` crate follows a daemon from a GPUI app. `ui/COMPONENTS.md` describes every component a set needs, for anyone making or redesigning one |
| `ui/themes` | Looks for the kits, one folder per theme with its DESIGN.md and a version per kit (`minitel/react`, a stylesheet; `minitel/swift`, the `ArchipelMinitel` library; `minitel/gpui`, the `archipel-gpui-minitel` crate) |
| `examples/` | What the table above points to |
| `testdata/` | Recorded sessions, and the state each must produce |
| `tools/watch/` | Looks for new versions of the adapters, harnesses and ACP, and writes down what changed |
| `conformance/` | Live checks of the real harnesses against a recorded baseline |
| `docs/design/` | The daemon's design, and the addon interface |
| `docs/research/` | How each decision was reached, in order |

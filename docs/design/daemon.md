# Daemon design

Draft 10, 2026-11-22: based on ACP v2; milestones 0 to 5 done (`docs/research/11` to `16`). The daemon runs Claude and Pi side by side. "bridge" stands for the project's temporary name. It rests on the live runs in `docs/research/09`, `10` and `11`. Items marked **to verify** have not been tested.

## 1. What the daemon is for

An app wants to show and drive a coding agent's loop. The harness runs where the code is; the app may be on the same machine, a phone, or another computer. ACP gives a shared vocabulary between one client and one agent process over stdio. It does not give:

- a session that outlives the app's connection,
- catching up after a disconnect,
- two apps on the same session,
- a transport that leaves the machine,
- one consistent behaviour across adapters.

The daemon supplies those five things and nothing else. It is not an orchestrator: no projects, git, checkpoints, worktrees, scheduling or model routing.

Principles:

- **Servers live on hosts** and use the harnesses installed there (claude, pi, ...).
- **The data model is known and standardised**, defined as a schema, not in any one language.
- **No UI in the server.** UIs are fully detached and use only the server's API and data model.
- **Modules.** A developer can take only the parts they need, to use a server or to add one.
- **Extensible from day one.** Another product can add its own content (checkpoints, handoffs) without forking.

## 2. Shape

```
 Swift app ─┐                                   ┌─ claude-agent-acp ── claude
 RN app    ─┼─ bridge protocol ──  DAEMON  ── ACP v2 or v1 (stdio) ─┼─ pi-acp ─────────── pi
 Electron  ─┘  (ACP v2 + extensions)    │           └─ any other ACP agent
                                     └─ event log (one file)
```

The daemon is an ACP **client** towards adapters and an ACP-shaped **agent** towards apps. One daemon per user per machine.

Inside:

| Part | Job |
|---|---|
| Listener | Accepts app connections (local socket, WebSocket, stdio) and authenticates them |
| Agent supervisor | Starts, restarts and stops adapter processes; holds their ACP connections |
| Adapter profiles | Bring each adapter's output to the canonical v2 form (section 6) |
| Session log | Appends every canonical event with a sequence number; serves replay |
| Session state | Folds the log into a current snapshot: items, status, pending requests |
| Hub | Fans events out to attached apps; routes prompts, cancels and answers back |

## 3. The protocol apps speak

**Decision (2026-11-22): ACP v2, unmodified, plus a small set of extensions.** v2 shapes are also the daemon's canonical internal form.

Reasons:

- One vocabulary with a public spec, schema and SDKs, including an official Rust crate.
- v2 already has what a UI needs and v1 lacked: a prompt call that returns at once, turn state as an update, ids on messages, replay on resume, streamed command output, background work (tested on the Claude adapter, `docs/research/11`).
- If ACP later ships a remote transport or multi-client attach, the daemon gets thinner instead of obsolete.

The cost: v2 has been in preview since 2026-11-20 and will change. The daemon is where that churn is absorbed:

- The repository pins one revision of the v2 schema. Apps see that revision, whatever each adapter speaks.
- Moving to a newer revision is one deliberate change: schema, generated types, adapter profiles, recorded transcripts.
- Unknown update kinds and `_meta` keys pass through untouched.

### What v2 gives natively

| Need | v2 |
|---|---|
| Turn state | `state_update`: `running`, `requires_action`, `idle` (with stop reason and usage) |
| Background work | `state_update.background` |
| Start a turn without blocking | `session/prompt` returns a `messageId` |
| Message while busy | A second `session/prompt` is accepted (seen on Claude) |
| Ids | `messageId` on messages and thoughts, `toolCallId`, `terminalId`, `compactionId` |
| Command output | `terminal_update`, `terminal_output_chunk` |
| History | `session/resume` with `replayFrom`, ended by `replay_complete` |
| Approvals, questions | `session/request_permission` with a subject, `elicitation/create` |
| Model, mode, effort | Config options |

### Extensions the daemon adds

ACP reserves `_`-prefixed methods and `_meta` for this.

| Addition | Purpose |
|---|---|
| `_bridge/agents/list` | Which agents the daemon can start, with version and readiness |
| `_bridge/sessions/list` | Sessions across all agents: the daemon's own, and those each harness reports (`logged: false`). Opening one of the latter adopts it (section 4) |
| `_bridge/sessions/updated` (notification) | A session appeared, or its title, turn state or liveness changed. Sent to every connected app |
| `_bridge/session/attach {sessionId, afterSeq?}` | Subscribe to a session another app may already be using. The response says whether a delta or a full replay follows |
| `_bridge/session/detach` | Unsubscribe without closing the session |
| `_meta.bridge.seq` on every logged message | Position in the session log, for catching up |
| `_meta.bridge.rootSessionId` | On a subagent's messages: the session whose log (and sequence numbers) they belong to |
| `_bridge/request/resolved` (notification) | A pending approval or question was answered, and by whom. In a replay it also carries the question |
| `_meta.bridge.agentId` on `session/new` | Which agent to create the session with |
| `_meta.bridge.origin = "daemon"` in an update | The daemon wrote this update itself (section 4) |

Extensions by other products use their own namespace (section 11).

A plain ACP v2 client also works: the daemon answers `session/resume` (with or without `replayFrom`) from its log, in ACP's order. It gets no sequence-based catch-up, only full replays.

Tested: 17 scenarios recorded on v2 through the Claude adapter, including subagents, questions, plans, compaction, steering, interrupt and background work (`testdata/transcripts/claude-v2`). Notices have not been seen yet.

## 4. Session log and replay

Every canonical (v2) update, every request from the agent and every answer gets a sequence number and is appended to the log before it is sent to anyone. Numbers count per conversation: a subagent's entries are in its parent's log.

- **Three kinds of entry:** update, request, answer. Requests have to be there because the state rules let a permission request create its tool call; a replay without them could show different items.
- **Attach with `afterSeq`:** the daemon sends the entries after it, then live ones.
- **Attach without it, or with a position beyond the log's end:** the daemon replays the session from its log in v2's own replay form, ending with `replay_complete`. There is no separate snapshot format: a snapshot is a condensed replay.
- **Requests in a replay:** one that can still be answered is sent as a real request. One that was answered cannot be asked again; it is sent as a single `_bridge/request/resolved` with the question and the answer, at the request's place.
- **Setup values:** `configOptions` and `availableCommands` arrive in the response to `session/new`, not as updates. The daemon writes them into the log as updates marked `origin = "daemon"`, so the log alone rebuilds a session.
- **Condensing:** chunk runs for the same id are merged when replaying, so a replay grows with content, not with token count. Stored entries are not merged yet (to do with retention).
- **Storage:** one SQLite file, one row per entry, keyed by session and sequence. Write-ahead log with `synchronous = NORMAL`.
- **The log is a cache.** The harness owns the transcript of record. A lost log is rebuilt from the adapter: Claude replays everything including subagent child sessions; Pi replays messages and tools but not reasoning (tested).
- **Adoption:** a session the daemon has never seen (started in the harness's own UI) is listed with `logged: false`. The first app that opens it makes the daemon call `session/resume` with a replay; the replayed updates become its log. A harness replay has no approvals or questions, so an adopted session shows less than one watched live (`docs/research/14`).
- **Retention:** by age (`retentionDays`); pruned sessions stay listed because the harness still has them, and are adopted again when opened. By size: not implemented.

SQLite revises the earlier "no database" note. That note was about not owning a domain model like T3's orchestrator; a replay log needs ordered reads, an index and crash safety. Append-only files per session are the fallback if a dependency-free daemon matters more.

## 5. Items: what makes clients thin

The ACP v2 schema says, for each message, how it changes client state (`x-state` annotations). For example: `agent_message_chunk` appends `content` to the `agentMessage` with id `messageId`; `tool_call_update` upserts the `toolCall` with id `toolCallId`; `state_update` replaces `state`.

`just gen` extracts these into `schema/state-rules.json`, and the reducer in `bridge-core` interprets them. There is no per-message code:

- Four actions: append, upsert, replace, remove.
- A target is a collection keyed by an id (messages, thoughts, tool calls, terminals, plans, subagents, compactions) or a single value (state, usage, config options, session info).
- An item is identified by target and id together; a thought and a reply can share a `messageId`.

| Item | v2 updates | Id |
|---|---|---|
| User message | `user_message`, `user_message_chunk` | `messageId` |
| Assistant message | `agent_message`, `agent_message_chunk` | `messageId` |
| Reasoning | `agent_thought`, `agent_thought_chunk` | `messageId` |
| Tool call (command, read, edit with diff, search, fetch, other) | `tool_call_update` | `toolCallId` |
| Command output | `terminal_update`, `terminal_output_chunk` | `terminalId`, linked from the tool call |
| Plan / todo list | `plan_update`, `plan_removed` | plan id |
| Subagent | `subagent_update`; content is a child session with its own items | child session id |
| Compaction | `compaction_update`, `compaction_summary_chunk` | `compactionId` |
| Approval request | `session/request_permission` (a request, not an update) | request id; its subject names the tool call |
| Question | `elicitation/create` (a request) | request id |
| Notice / error | `notice` carries no state rule; kept as an event | **to verify**, none recorded yet |

What the rules leave out and the core adds: the order of items (a timeline of first appearance), requests as pending until answered, one state per session id for subagents, and joining base64 terminal output.

A client SDK needs the same small interpreter plus the rules file. A new message kind in a later v2 revision needs a regenerated rules file, not new code. Kinds the rules do not know, including vendor extensions, are kept and passed on.

Each recorded transcript has a readable rendering of the resulting items in `testdata/vectors`, which every implementation must reproduce.

## 6. Adapter profiles

One profile per adapter: whatever it takes for the adapter to look, to the daemon, like an ACP v2 adapter that hosts many sessions. The daemon itself has no code for any particular harness (done, `docs/research/15`).

### Claude (`claude-agent-acp`, v2)

Driven over v2, so little translation is needed.

- Set `CLAUDE_CODE_THINKING_DISPLAY=summarized`; without it no reasoning is sent (tested).
- One adapter process hosts many sessions (tested).
- Strip account details from `auth/status_update` before logging; expose readiness through `_bridge/agents/list`.
- The session starts in the user's own permission mode (`auto` on the test machine). Report it; the app chooses.
- Todo lists: the adapter maps them to plan entries, but this could not be tested because the installed Claude Code has no todo tool.

### Pi (`pi-acp`, v1)

Translated up to v2 by a translator that sits between the daemon and `pi-acp` (`crates/bridge-agents/src/pi.rs`). It also runs one `pi-acp` per session behind one connection (section 8). Tested on twelve recordings and live.

| v1 behaviour | Canonical form |
|---|---|
| `session/prompt` blocks until the turn ends | Return a `messageId` at once; emit `state_update` from the turn's start and end and from `_meta.piAcp` (`running`, `queueDepth`) |
| Chunks carry no id | Assign a `messageId` per run of chunks |
| No echo of the user message | Emit `user_message` for each prompt |
| Command output under a `terminal_output` meta key | `terminal_update` and `terminal_output_chunk` |
| Queueing announced as assistant text ("Queued message (position 1).") | Dropped; queue depth goes into state |
| Startup banner as an assistant message | Dropped, or `quietStartup` set in Pi's settings |
| Subagent as one tool call with details in `rawOutput` | Kept as a tool call at first; a child session later if the raw shape is stable |
| MCP tools as kind `other`, title is the tool name | Tool call with server and tool name |
| No approvals, questions, plans | Nothing to translate |
| Reasoning not replayed on reload | Accepted; the daemon's own log keeps it |

A second option for Pi is to drop `pi-acp` and have the daemon speak Pi's own RPC protocol (section 8).

## 7. Several apps on one session

- Every attached app receives every event. Done.
- Any app with the `interact` scope may prompt, cancel, or answer. An observer's requests to act are refused (`-32003`); its answers and cancels are ignored. Done.
- A pending approval or question is shown to all, and `state_update` reports `requires_action`; the first answer wins and every app receives `_bridge/request/resolved`. Done.
- A pending request survives all apps disconnecting, and is sent again to every app that attaches, since an app can only answer on its current connection. Done. Tested on Claude: still valid after 150 seconds, no timeout seen.
- Prompts are handed to the adapter in the order the daemon receives them; the adapter queues or steers.
- An app that reads too slowly (4096 messages behind) is disconnected instead of buffered without limit. It catches up when it attaches again.

## 8. Adapter processes

- Whether sessions share an adapter process is a profile setting. Claude's adapter hosts several concurrent sessions (tested).
- **Pi: one process per session.** Pi's RPC mode holds one conversation per `pi` process, and `pi-acp` starts one `pi` per session anyway. `pi-acp` additionally closes every other session when a new one is created, so today each session needs its own `pi-acp` plus its own `pi`. Measured on the test machine: one idle `pi --mode rpc` is about 680 MB across four processes, most of it the user's own extensions and MCP servers. Ways to reduce this, in order of effort:
  1. **Chosen for the first version (2026-11-22):** accept it, and stop idle Pi processes after a timeout. `session/load` brings a session back: tested, about 1.7 seconds, context intact (it is what Pi's `/resume` does). What the app shows comes from the daemon's log, which also keeps the reasoning that Pi's own replay drops. A session is stopped only when truly idle: no turn running, no pending request, no queued message, no background work.
  2. Remove the one-line `closeAllExcept` call in `pi-acp` (ideally upstream) so one `pi-acp` serves all sessions. This saves the adapter process, not the `pi` process.
  3. Write a Pi bridge in the daemon that speaks Pi's RPC protocol directly. No `pi-acp` at all, and it emits v2 natively, which removes the translation table in section 6. Still one `pi` per session. T3's Pi adapter and its recorded transcripts are the specification.
  4. Host several sessions in one Node process through Pi's SDK (`createAgentSession`). The only way to truly share a process, but it means maintaining a Node program that embeds Pi and loses the isolation between sessions. Not recommended.
- Adapters are pinned by version in the daemon's manifest and updated deliberately; the ACP registry is the catalogue for accessory agents.
- If an adapter dies: its sessions stop being live. Open approvals are cancelled, and a turn that was running gets a `notice` and a `state_update` to `idle` with stop reason `cancelled`, all written to the log by the daemon. The next request for such a session restarts the adapter and resumes the session with `session/resume`. Done, tested live.
- If the daemon restarts: sessions are listed and replayed from the log with no adapter running; the adapter is started on the first request that needs it. Done, tested live.
- The daemon never passes app-supplied commands or environment to a spawn. Agents come from its own manifest.

## 9. Transport and security

The daemon can run commands as the user. Anything that can talk to it has that power, so the defaults are closed.

| Path | Transport | Authentication |
|---|---|---|
| Same machine | Unix socket (named pipe on Windows) | File permissions, owner only |
| Remote, over SSH | `bridge connect --stdio` run through SSH, same frames | SSH |
| Remote, direct | WebSocket, off unless asked for | Bearer token; TLS from Tailscale serve or a reverse proxy |

All three carry the same JSON-RPC messages (one per line, or one per WebSocket text frame), so the daemon and clients have one codec.

Implemented in milestone 2: the Unix socket (owner-only file in an owner-only directory) and the WebSocket. The WebSocket requires `Authorization: Bearer <token>`, with the token in a file only the owner can read, and refuses any request that has an `Origin` header: any web page may try to open a WebSocket to a local port, so binding to loopback is not a protection by itself. Browser clients therefore cannot connect yet.

- **Pairing:** the daemon prints a one-time code; the app exchanges it for a long-lived token stored in the platform keychain. Tokens are listed and revocable. Until then, `bridged token add NAME --scope …` makes a token by hand; tokens are kept as they are in owner-only files, and should be stored hashed once pairing exists.
- **Scopes:** `observe` (list and read), `interact` (also create, prompt, answer, cancel, adopt), `admin` (pair, configure; nothing needs it yet). A wall display gets `observe`. The local socket always has every right: whoever can open it is the user. Done.
- **Browser clients:** origin allow-list on the WebSocket; no cookies.
- **Logs:** account details and environment values are stripped before storage.
- **Not built:** an own relay or end-to-end encryption. Tailscale and SSH cover remote access with maintained code. Revisit only if a hosted relay becomes a goal.

Harness credentials never pass through the daemon's protocol. Each harness keeps its own login.

## 10. Data model as a schema

The contract lives in `schema/`:

| File | Role |
|---|---|
| `acp-v2.schema.json` | The pinned ACP v2 schema, vendored unmodified |
| `state-rules.json` | How each message changes client state, extracted from the pinned schema |
| `bridge.schema.json` | The daemon's extensions (section 3) |

- **ACP:** the pinned file is the reference. Rust types come from the official `agent-client-protocol-schema` crate, which is the schema's upstream source. A test parses every recorded update with those types, so the file and the crate cannot drift apart unnoticed.
- **Extensions:** the Rust types in `bridge-schema` are the source, and the schema file is generated from them. This is how ACP itself is produced.
- **Clients:** TypeScript and Swift types are generated from the three files.

`just gen` regenerates the two derived files, and CI fails if they are stale. Capability negotiation at `initialize` tells each side what the other supports, as in ACP.

## 11. Extensions

The requirement: a product such as T3 Code can add checkpoints or handoffs without changing the server.

Two mechanisms, both independent of the server's language. Done; see `docs/research/16`.

**Namespaced content.** Update kinds `x.<namespace>.<name>` and methods `_x/<namespace>/<name>`. The server stores, sequences, replays and fans them out without understanding the payload. An extension's update carries its own state rule (`_meta.bridge.state`, in the form of ACP's rules), so every client's reducer places and updates the item correctly without knowing the extension. A UI that has no component for a kind skips it or shows a generic fallback.

**Extension peers.** An extension is a separate process that connects to the server like an app, with the `extend` scope, and registers its namespace (`_bridge/extension/register`). It can:

- observe session events,
- append and update items in its namespace (`_bridge/session/append`),
- serve methods in its namespace; the server routes app calls to it and returns the answer.

Example: a checkpoint extension observes a turn ending, captures a git ref, and appends an `x.t3.checkpoint` item. The app calls `_x/t3/checkpoint/restore`; the server routes it to the extension. The server knows nothing about git.

An extension crashing does not take the server down, and extensions can be written in any language.

Not in the first version: extensions that block or rewrite the flow (for example, approve a prompt before it is sent). That needs ordering and timeout rules; the namespaces leave room for it.

A product written in Rust has a third option: build its own server from the crates (section 12) and add code in-process.

## 12. Implementation

**Decision: Rust**, chosen for extensibility, maintainability and performance over iteration speed.

- One static binary per platform, no runtime to install on the host.
- The core can later be linked into a native app as a library, the libghostty model.
- The compiler checks that every message variant is handled, which matters on a protocol that is still moving (ACP v2 is in preview).
- A smaller dependency tree for a program that can run commands as the user.

Accepted costs: slower early progress, and a TypeScript product cannot import the server as a package (it uses an extension peer, or a binding later).

A correction to draft 1: it is not true that every setup already needs Node. Pi and `pi-acp` do. Claude Code is a native binary, and `claude-agent-acp` ships standalone executables. A Claude-only host can run this stack with no Node at all.

### Crates (temporary names)

| Crate | Contents | Depends on IO |
|---|---|---|
| `bridge-schema` | Schema files and generated types | no |
| `bridge-core` | Reducer, log records and condensing, the store trait, the client mirror, transcripts | no |
| `bridge-store-sqlite` | The session log on SQLite | yes |
| `bridge-agents` | Starting adapters, profiles (Claude so far), the fake adapter | yes |
| `bridge-transport` | The message codec over lines, a Unix socket and a WebSocket | yes |
| `bridge-server` | The daemon as a library, behind a builder | yes |
| `bridged` | The daemon program: config, listeners | yes |
| `bridge-client` | A client's connection, and the `bridge` command-line tool | yes |
| `bridge-ffi` | C interface for embedding in native apps | later |

`bridge-core` holds no IO so it can be tested with recorded transcripts alone, and so the reducer can later be shipped to clients (FFI or WebAssembly) if hand-written reducers drift.

Adding a harness means: an ACP adapter (any language, its own process) and a profile in `bridge-agents`. A profile is mostly data (how to start the adapter, which capabilities to declare) plus the normalising rules for that adapter.

In use: `agent-client-protocol-schema` for the ACP types, serde, schemars, tokio, rusqlite (SQLite compiled in), tokio-tungstenite.

Two choices made in milestone 2:

- **The connection layer is ours.** The daemon forwards messages it must not interpret, and the `agent-client-protocol` crate's typed connection would drop kinds and fields its types do not know. Reading and writing JSON values line by line is about 60 lines. The official types check shapes in tests.
- **One task owns all state.** Messages from apps, messages from adapters and closed connections are events on one queue, handled one at a time. No locks; the order of events is the only thing to reason about. `crates/bridge-server/src/daemon.rs` opens with a table of what each message causes.

### Client side

Separate packages, none of them in the server:

| Package | Contents |
|---|---|
| Client SDK, TypeScript | Generated types, transports, reducer. For web, Electron, React Native |
| Client SDK, Swift | The same for Apple platforms |
| UI kit, SwiftUI | One component per item kind, and a simple chat view assembled from them |
| UI kit, React | The same for web |

UI kits are optional. An app can use the SDK alone and draw everything itself. Each component takes an item and callbacks, nothing else, so components for extension kinds plug in the same way.

## 13. Repository

One repository, because a schema change touches the server and every client at once, and the test data is shared. Each language uses its own workspace tool, with `just` on top:

| Tool | For |
|---|---|
| Cargo workspace | The Rust crates |
| pnpm workspace | The TypeScript SDK and React kit |
| Swift Package Manager | The Swift SDK and SwiftUI kit; `Package.swift` goes at the repository root when Swift code lands |
| `just` | The cross-language commands: `gen`, `check`, `test`, `ci` |

No Turbo, Nx or Bazel. Turbo can be added on top of pnpm later if the TypeScript side grows.

```
Cargo.toml  justfile  pnpm-workspace.yaml
schema/       the data model (source of truth)
crates/       the Rust workspace
clients/ts    clients/swift
ui/react      ui/swift
testdata/     transcripts and reducer vectors
docs/  spikes/  data/
```

Two rules:

- **Generated code is checked in, and CI fails if it is stale.** `just ci` regenerates from `schema/` and fails on any diff.
- **One version number for the whole repository,** tied to the schema version. Swift packages are versioned by git tag, so per-package versions would not work, and one number tells users which server and client match.

## 14. Testing

- **Recorded transcripts:** `spikes/acp-probe/logs` is already the format: timestamped inbound and outbound frames. A fake adapter replays them, as T3 does with its fixtures.
- **Scenario catalogue:** T3's scenario names and prompts (simple, approval, interrupt mid-tool, subagent, background wake, steering, compaction, resume after restart and so on) are the checklist. T3's fix history for Claude is a backlog of edge cases.
- **Conformance matrix:** scenario by agent, generated from the runs, so a new adapter version shows what changed.
- **Reducer vectors:** `testdata/vectors` holds a readable rendering of the state each transcript ends in; the Rust core reproduces them, and every client SDK must.
- **Invariants on every transcript:** a condensed replay gives the same state as the full log; a client that catches up from the middle reaches the same state; every update parses with the official ACP types.

## 15. Milestones

0. **Close the unknowns.** Done; see `docs/research/11`.
1. **Schema and core.** Done; see `docs/research/12`.
2. **Single-session daemon.** Done; see `docs/research/13`. Fan-out to several apps and "first answer wins" came with it.
3. **Several apps.** Done except Windows; see `docs/research/14`. Scopes, harness sessions listed and adopted, session-list notifications, remembered setup, retention.
4. **Adapter profiles for Claude and Pi.** Done; see `docs/research/15`.
5. **Extensions.** Done; see `docs/research/16`.
6. **Client SDKs and UI kits.** TypeScript and Swift SDKs; a React and a SwiftUI chat view rendering the same session.
7. **Remote.** Pairing, tokens, SSH stdio mode, Tailscale notes.

## 16. Open decisions

1. **Windows local transport:** a named pipe only the owner can open needs the Windows API directly. Allow one audited `unsafe` block, or a small dependency, and test on a Windows machine.
2. **Pi integration, later:** whether to replace `pi-acp` with a direct Pi RPC bridge (section 8, option 3). The first version keeps `pi-acp`, one process per session, with idle sessions stopped and reloaded.
3. **How far to go for Pi subagents and Claude command output** in the first version, or accept the adapters' current behaviour.
4. **Name.** "harness-bridge" is temporary; the copyright line in `LICENSE-MIT` follows it.

Decided (2026-11-22): the daemon lists and adopts the harness's own sessions. SQLite for the log, behind a trait. ACP v2 as the canonical form and the protocol apps speak. Rust for the server. Dual licence, MIT OR Apache-2.0: the Rust convention, compatible with every dependency (ACP and its adapters are Apache-2.0; Pi, pi-acp and T3 Code are MIT), and it gives adopters a patent grant or MIT's simplicity as they prefer. Copyleft was ruled out because it would deter embedding the server in commercial apps.

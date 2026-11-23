# Daemon design

Draft 5, 2026-11-22: based on ACP v2 after milestone 0 (`docs/research/11`). "bridge" stands for the project's temporary name. It rests on the live runs in `docs/research/09`, `10` and `11`. Items marked **to verify** have not been tested.

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
 RN app    ─┼─ bridge protocol ──  DAEMON  ── ACP (stdio) ─┼─ pi-acp ─────────── pi
 Electron  ─┘  (ACP + extensions)    │           └─ any other ACP agent
                                     └─ event log (one file)
```

The daemon is an ACP **client** towards adapters and an ACP-shaped **agent** towards apps. One daemon per user per machine.

Inside:

| Part | Job |
|---|---|
| Listener | Accepts app connections (local socket, WebSocket, stdio) and authenticates them |
| Agent supervisor | Starts, restarts and stops adapter processes; holds their ACP connections |
| Normaliser | Turns each adapter's dialect into one canonical stream (section 6) |
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
| `_bridge/sessions/list` | Sessions across all agents, live or stored, with state and title |
| `_bridge/session/attach {sessionId, afterSeq?}` | Subscribe to a session another app may already be using |
| `_bridge/session/detach` | Unsubscribe without closing the session |
| `_meta.bridge.seq` on every `session/update` | Position in the session log, for catching up |
| `_bridge/request/resolved` (notification) | A pending approval or question was answered, and by whom |

Extensions by other products use their own namespace (section 11).

**To verify:** only three prompts were run on v2. Subagents, questions, plans, compaction and notices need the full scenario set rerun on v2 (milestone 2).

## 4. Session log and replay

Every canonical (v2) update gets a per-session sequence number and is appended to the log before it is sent to anyone.

- **Attach with `afterSeq`:** the daemon sends the updates after it, then live ones.
- **Attach without it, or with too large a gap:** the daemon replays the session from its log in v2's own replay form, ending with `replay_complete`. There is no separate snapshot format: a snapshot is a coalesced replay.
- **Coalescing:** chunk runs for the same id are merged in storage, so replay cost grows with content, not with token count.
- **Storage:** one SQLite file, one row per update, indexed by session and sequence.
- **The log is a cache.** The harness owns the transcript of record. A lost log is rebuilt from the adapter: Claude replays everything including subagent child sessions; Pi replays messages and tools but not reasoning (tested).
- **Retention:** configurable by age and size; pruned sessions stay listed if the harness still has them.

SQLite revises the earlier "no database" note. That note was about not owning a domain model like T3's orchestrator; a replay log needs ordered reads, an index and crash safety. Append-only files per session are the fallback if a dependency-free daemon matters more.

## 5. Items: what makes clients thin

A client keeps a list of items and applies each update to it. With v2 ids the rule is: *find the item by id; a chunk appends; anything else merges; an unknown id creates the item.*

| Item | v2 updates | Id |
|---|---|---|
| User message | `user_message`, `user_message_chunk` | `messageId` |
| Assistant message | `agent_message`, `agent_message_chunk` | `messageId` |
| Reasoning | `agent_thought`, `agent_thought_chunk` | `messageId` |
| Tool call (command, read, edit with diff, search, fetch, other) | `tool_call_update` | `toolCallId` |
| Command output | `terminal_update`, `terminal_output_chunk` | `terminalId`, linked from the tool call |
| Approval request | `session/request_permission` | request id; its subject names the tool call |
| Question | `elicitation/create` | request id |
| Plan / todo list | `plan_update`, `plan_removed` | plan id |
| Subagent | `subagent_update`; content is a child session with its own items | child session id |
| Compaction | `compaction_update`, `compaction_summary_chunk` | `compactionId` |
| Notice / error | `notice`; adapter failure | **to verify** |

Session-level state sits beside the items: `state_update`, config options, usage, available commands, title.

Where an update has no usable id, the daemon adds `_meta.bridge.itemId`. That is expected to be rare on v2.

The rule is small enough to write by hand in Swift or Kotlin, and the same recorded transcripts test every implementation.

## 6. Adapter profiles

One profile per adapter: how to start it, which protocol version and capabilities to use, and how to bring its output to the canonical form.

### Claude (`claude-agent-acp`, v2)

Driven over v2, so little translation is needed.

- Set `CLAUDE_CODE_THINKING_DISPLAY=summarized`; without it no reasoning is sent (tested).
- One adapter process hosts many sessions (tested).
- Strip account details from `auth/status_update` before logging; expose readiness through `_bridge/agents/list`.
- The session starts in the user's own permission mode (`auto` on the test machine). Report it; the app chooses.
- Todo lists: the adapter maps them to plan entries, but this could not be tested because the installed Claude Code has no todo tool.

### Pi (`pi-acp`, v1)

Translated up to v2 by the daemon.

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

- Every attached app receives every event.
- Any app with the right scope may prompt, cancel, or answer.
- A pending approval or question is shown to all, and `state_update` reports `requires_action`; the first answer wins and the others receive `_bridge/request/resolved`.
- A pending request survives all apps disconnecting. It is in the snapshot when one returns. Tested on Claude: still valid after 150 seconds, no timeout seen.
- Prompts are serialised per session by the daemon, then handed to the adapter, which queues or steers.

## 8. Adapter processes

- Whether sessions share an adapter process is a profile setting. Claude's adapter hosts several concurrent sessions (tested).
- **Pi: one process per session.** Pi's RPC mode holds one conversation per `pi` process, and `pi-acp` starts one `pi` per session anyway. `pi-acp` additionally closes every other session when a new one is created, so today each session needs its own `pi-acp` plus its own `pi`. Measured on the test machine: one idle `pi --mode rpc` is about 680 MB across four processes, most of it the user's own extensions and MCP servers. Ways to reduce this, in order of effort:
  1. Accept it for the first version, and stop idle Pi processes after a timeout; `session/load` brings a session back.
  2. Remove the one-line `closeAllExcept` call in `pi-acp` (ideally upstream) so one `pi-acp` serves all sessions. This saves the adapter process, not the `pi` process.
  3. Write a Pi bridge in the daemon that speaks Pi's RPC protocol directly. No `pi-acp` at all, and it emits v2 natively, which removes the translation table in section 6. Still one `pi` per session. T3's Pi adapter and its recorded transcripts are the specification.
  4. Host several sessions in one Node process through Pi's SDK (`createAgentSession`). The only way to truly share a process, but it means maintaining a Node program that embeds Pi and loses the isolation between sessions. Not recommended.
- Adapters are pinned by version in the daemon's manifest and updated deliberately; the ACP registry is the catalogue for accessory agents.
- If an adapter dies: sessions go to `error` with a notice item; the next prompt restarts the adapter and resumes with `session/resume` or `session/load`.
- If the daemon restarts: sessions are listed from the log; adapters are started on first attach or prompt.
- The daemon never passes app-supplied commands or environment to a spawn. Agents come from its own manifest.

## 9. Transport and security

The daemon can run commands as the user. Anything that can talk to it has that power, so the defaults are closed.

| Path | Transport | Authentication |
|---|---|---|
| Same machine | Unix socket (named pipe on Windows) | File permissions, owner only |
| Remote, over SSH | `bridge connect --stdio` run through SSH, same frames | SSH |
| Remote, direct | WebSocket, bound to loopback unless configured | Bearer token from pairing; TLS from Tailscale serve or a reverse proxy |

All three carry the same newline-delimited JSON-RPC, so the daemon and clients have one codec.

- **Pairing:** the daemon prints a one-time code; the app exchanges it for a long-lived token stored in the platform keychain. Tokens are listed and revocable.
- **Scopes:** `observe` (read), `interact` (prompt, answer, cancel), `admin` (pair, configure). A wall display gets `observe`.
- **Browser clients:** origin allow-list on the WebSocket; no cookies.
- **Logs:** account details and environment values are stripped before storage.
- **Not built:** an own relay or end-to-end encryption. Tailscale and SSH cover remote access with maintained code. Revisit only if a hosted relay becomes a goal.

Harness credentials never pass through the daemon's protocol. Each harness keeps its own login.

## 10. Data model as a schema

The contract is a set of schema files, not code:

- ACP's published JSON Schema, unmodified.
- An extension schema for everything in sections 3 to 5: the `_bridge/` methods, the `_meta.bridge` fields, the item kinds and the session state.

Server types, Swift types and TypeScript types are generated from these. A change to the model is a change to the schema first, versioned, with the generated code following. Capability negotiation at `initialize` tells each side what the other supports, as in ACP.

## 11. Extensions

The requirement: a product such as T3 Code can add checkpoints or handoffs without changing the server.

Two mechanisms, both independent of the server's language:

**Namespaced content.** Item kinds, methods and `_meta` keys outside the standard set carry a vendor namespace (`x.<vendor>.<name>`). The server stores, sequences, replays and fans them out without understanding the payload. A UI that does not know a kind skips it or shows a generic fallback.

**Extension peers.** An extension is a separate process that connects to the server like an app, with an `extend` scope. It can:

- observe session events,
- append and update items in its namespace,
- register methods in its namespace; the server routes app calls to it and returns the answer.

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
| `bridge-core` | Items, reducer, session state, sequencing, profile and store traits | no |
| `bridge-store-sqlite` | The session log on SQLite | yes |
| `bridge-agents` | Adapter supervisor, ACP client connection, profiles for Claude and Pi | yes |
| `bridge-transport` | Local socket / named pipe, WebSocket, stdio; auth and scopes | yes |
| `bridge-server` | Assembles the above behind a builder; the library entry point | yes |
| `bridged` | The daemon binary: config, CLI, pairing | yes |
| `bridge-ffi` | C interface for embedding in native apps | later |

`bridge-core` holds no IO so it can be tested with recorded transcripts alone, and so the reducer can later be shipped to clients (FFI or WebAssembly) if hand-written reducers drift.

Adding a harness means: an ACP adapter (any language, its own process) and a profile in `bridge-agents`. A profile is mostly data (how to start the adapter, which capabilities to declare) plus the normalising rules for that adapter.

Likely building blocks, to confirm in milestone 0: the official ACP Rust crate for protocol types and the client role, tokio, serde, rusqlite. Checked: `agent-client-protocol` 5.5.1 has Client, Agent and Proxy roles, the newer update kinds, and v2 behind `unstable_protocol_v2`; a test crate compiled. **To verify:** that update kinds unknown to the crate (such as the AIR `async_task_*` ones) pass through instead of failing to parse.

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
- **Reducer vectors:** transcript in, item list out, shared by the Rust core and every client SDK.

## 15. Milestones

0. **Close the unknowns.** Done; see `docs/research/11`.
1. **Schema and core.** The extension schema, generated types, the reducer, and reducer vectors from the recorded logs. No network yet.
2. **Single-session daemon.** Local socket and WebSocket, one adapter, log, attach with replay. A command-line client prints a live session and survives a reconnect.
3. **Several apps.** Fan-out, request broker, scopes, session state.
4. **Adapter profiles for Claude and Pi.** Section 6, with a recorded transcript for each rule, and the full scenario set rerun on v2.
5. **Extensions.** Namespaced items and extension peers, proven with a toy checkpoint extension.
6. **Client SDKs and UI kits.** TypeScript and Swift SDKs; a React and a SwiftUI chat view rendering the same session.
7. **Remote.** Pairing, tokens, SSH stdio mode, Tailscale notes.

## 16. Open decisions

1. **Log storage:** SQLite (recommended) or append-only files.
2. **Pi integration:** keep `pi-acp` and translate v1 up (first version), or write a direct Pi RPC bridge (section 8, option 3).
3. **How far to go for Pi subagents and Claude command output** in the first version, or accept the adapters' current behaviour.
4. **Name.** "harness-bridge" is temporary; the copyright line in `LICENSE-MIT` follows it.

Decided (2026-11-22): ACP v2 as the canonical form and the protocol apps speak. Rust for the server. Dual licence, MIT OR Apache-2.0: the Rust convention, compatible with every dependency (ACP and its adapters are Apache-2.0; Pi, pi-acp and T3 Code are MIT), and it gives adopters a patent grant or MIT's simplicity as they prefer. Copyleft was ruled out because it would deter embedding the server in commercial apps.

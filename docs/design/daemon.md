# Daemon design

Draft 3, 2026-11-22. "bridge" stands for the project's temporary name. Items marked **to verify** have not been tested; everything else rests on the runs in `docs/research/09` and `10`.

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

**Decision: ACP, unmodified, plus a small set of extensions.** ACP reserves `_`-prefixed method names and `_meta` fields for this.

Reasons:

- One vocabulary to learn, with a public spec, schema and SDKs.
- An app can be tested against a bare adapter, and a stock ACP client can talk to the daemon in a degraded way.
- If ACP ships its remote transport and replay, the daemon gets thinner instead of obsolete.

The alternative, an own item-upsert protocol as T3 has, makes clients simpler but means owning a schema, its docs and its evolution. Section 5 gets most of that simplicity without leaving ACP.

### Extensions

| Addition | Purpose |
|---|---|
| `_bridge/agents/list` | Which agents the daemon can start, with version and readiness |
| `_bridge/sessions/list` | Sessions the daemon knows, live or stored, with status and title |
| `_bridge/session/attach {sessionId, afterSeq?}` | Subscribe; returns a snapshot or the events after `afterSeq` |
| `_bridge/session/detach` | Unsubscribe without ending the session |
| `_bridge/session/state` (notification) | `idle`, `running`, `awaiting_input`, plus background work count |
| `_bridge/request/resolved` (notification) | A pending approval or question was answered, and by whom |
| `_meta.bridge.seq` on every `session/update` | Position in the session log |
| `_meta.bridge.itemId` on every `session/update` | Stable id of the thing the update belongs to (section 5) |

Extensions by other products use their own namespace (section 11).

`session/new`, `session/prompt`, `session/cancel`, `session/set_mode`, `session/set_config_option`, `session/request_permission` and `elicitation/create` pass through with ACP semantics.

### One difference from plain ACP

In ACP v1, `session/prompt` returns when the turn ends. The tests showed that updates keep coming after that (a subagent finishing, a background command waking the agent). Apps must therefore take turn state from `_bridge/session/state`, not from the prompt response. This matches where ACP v2 is going (`state_update`), which should make a later move to v2 mechanical.

## 4. Session log and replay

Every canonical event gets a per-session sequence number and is appended to the log before it is sent to anyone.

- **Attach with no `afterSeq`:** the daemon sends a snapshot (section 5) and its sequence number, then live events.
- **Attach with `afterSeq`:** the daemon sends the events after it. If the gap is larger than a threshold, it sends a snapshot instead and says so.
- **Coalescing:** consecutive text chunks for the same item are merged in storage, so replay cost grows with content, not with token count.
- **Storage:** one SQLite file, one row per event, indexed by session and sequence. The log is a cache: the harness still owns the transcript of record, and a lost log can be rebuilt with `session/load`. **To verify:** how faithfully each adapter's `session/load` replays, including subagents.
- **Retention:** configurable by age and size; pruned sessions remain listed if the harness still has them.

SQLite revises the earlier "no database" note. That note was about not owning a domain model like T3's orchestrator; a replay log needs ordered reads, an index and crash safety, which is what SQLite is for. Append-only files per session are the fallback if a dependency-free daemon matters more.

## 5. Items: what makes clients thin

ACP v1 chunks carry no message id, and tool updates are partial. A client that folds raw ACP has to guess boundaries. The daemon does that once:

- It assigns an `itemId` to every renderable thing and stamps it on each update.
- A snapshot is the ordered list of items in their current state.

Item kinds, from `docs/research/07` mapped to ACP:

| Item | Built from |
|---|---|
| User message | `user_message_chunk`, or the prompt the daemon forwarded |
| Assistant message | `agent_message_chunk` runs |
| Reasoning | `agent_thought_chunk` runs |
| Tool call (command, read, edit with diff, search, fetch, other) | `tool_call` + `tool_call_update`, keyed by `toolCallId` |
| Approval request | `session/request_permission`, linked to its tool call |
| Question | `elicitation/create` |
| Plan | `plan_update` with markdown |
| Todo list | `plan` entries |
| Subagent | `subagent_update`; its content is a child session with its own items |
| Compaction | `compaction_update` |
| Notice / error | `notice`, failed prompt, adapter exit |

Session-level state sits beside the items: status, model, mode, usage, available commands, prompt suggestions, title.

The client rule becomes: *upsert by `itemId`; a chunk appends; anything else merges.* That is small enough to write by hand in Swift or Kotlin, and the same recorded transcripts test every implementation.

## 6. Normaliser

One profile per adapter. Each entry is a difference seen in testing.

| Difference | Claude adapter | Pi adapter | Canonical form |
|---|---|---|---|
| Command output while running | Only at completion | `terminal_output` meta, incremental | Incremental output on the tool item. For Claude: accept end-only at first; later consider tailing the task output file as T3 does |
| Subagents | Child session | One tool call, details in `rawOutput` | Child session. For Pi: show the tool call at first; unpack later if the raw shape is stable |
| Background work | Wake-up text after the turn; task items only behind the JetBrains AIR extension | Not tested | Background count in session state. **To verify:** declare the AIR `asyncTasks` capability and map it |
| Message while busy | Steering, advertised in `_meta` | Queue; announced as assistant text, depth in `_meta.piAcp` | Explicit queued/steered state on the user message; drop the announcement text |
| Reasoning | None seen | Thought chunks | Reasoning item. **To verify:** which setting makes Claude emit summaries (T3 sets `showThinkingSummaries`) |
| Todos | Mapped to plan entries (not reproduced) | None | Todo list item |
| Startup banner | None | Assistant message at session start | Dropped (or set `quietStartup`) |
| MCP tool calls | `claudeCode.toolName` in meta | Title is the tool name, kind `other` | Tool item with server and tool name |
| Auth status | `auth/status_update` with account details | Error on `session/new` | Agent readiness in `_bridge/agents/list`; account details not logged |
| Default permission mode | Inherits the user's (`auto` here) | No approvals | Reported as-is; the app chooses |

The profile also records which ACP capabilities to declare to each adapter, since adapters gate features on them (plans, subagents, elicitation, notices, compaction were all opt-in).

Unknown updates and `_meta` keys pass through untouched, so a client can use something the normaliser does not know yet.

## 7. Several apps on one session

- Every attached app receives every event.
- Any app with the right scope may prompt, cancel, or answer.
- A pending approval or question is shown to all; the first answer wins and the others receive `_bridge/request/resolved`.
- A pending request survives all apps disconnecting. It is in the snapshot when one returns. **To verify:** how long each adapter waits before timing out a permission request.
- Prompts are serialised per session by the daemon, then handed to the adapter, which queues or steers.

## 8. Adapter processes

- One adapter process per agent kind per working set, reused across sessions where the adapter allows it. **To verify:** multiple concurrent sessions per `claude-agent-acp` and `pi-acp` process.
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
| `bridge-core` | Items, reducer, session state, sequencing, normaliser and store traits | no |
| `bridge-store-sqlite` | The session log on SQLite | yes |
| `bridge-agents` | Adapter supervisor, ACP client connection, profiles for Claude and Pi | yes |
| `bridge-transport` | Local socket / named pipe, WebSocket, stdio; auth and scopes | yes |
| `bridge-server` | Assembles the above behind a builder; the library entry point | yes |
| `bridged` | The daemon binary: config, CLI, pairing | yes |
| `bridge-ffi` | C interface for embedding in native apps | later |

`bridge-core` holds no IO so it can be tested with recorded transcripts alone, and so the reducer can later be shipped to clients (FFI or WebAssembly) if hand-written reducers drift.

Adding a harness means: an ACP adapter (any language, its own process) and a profile in `bridge-agents`. A profile is mostly data (how to start the adapter, which capabilities to declare) plus the normalising rules for that adapter.

Likely building blocks, to confirm in milestone 0: the official ACP Rust crate for protocol types and the client role, tokio, serde, rusqlite. **To verify:** that the ACP crate covers the newer parts used here (subagents, elicitation, compaction) and v2.

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

0. **Close the unknowns.** The "to verify" items, one run of ACP v2 against the Claude adapter, and a check of the ACP Rust crate.
1. **Schema and core.** The extension schema, generated types, the reducer, and reducer vectors from the recorded logs. No network yet.
2. **Single-session daemon.** Local socket and WebSocket, one adapter, log, attach with replay. A command-line client prints a live session and survives a reconnect.
3. **Several apps.** Fan-out, request broker, scopes, session state.
4. **Normaliser for Claude and Pi.** The table in section 6, with a recorded transcript for each row.
5. **Extensions.** Namespaced items and extension peers, proven with a toy checkpoint extension.
6. **Client SDKs and UI kits.** TypeScript and Swift SDKs; a React and a SwiftUI chat view rendering the same session.
7. **Remote.** Pairing, tokens, SSH stdio mode, Tailscale notes.

## 16. Open decisions

1. **Log storage:** SQLite (recommended) or append-only files.
2. **ACP v1 or v2 as the base:** v1 is stable; v2 is in preview since 2026-11-20 and already covers state and prompting during work. Recommended: build on v1, shape the internals like v2, decide after milestone 0.
3. **How far to go for Pi subagents and Claude command output** in the first version, or accept the adapters' current behaviour.
4. **Licence and name.**

Decided: Rust for the server (2026-11-22).

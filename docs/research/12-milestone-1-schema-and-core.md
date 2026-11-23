# Milestone 1: schema and core

_2026-11-22. No networking. Everything here is checked by `cargo test --workspace` against the recordings in `testdata/`._

## What exists now

- **17 Claude scenarios recorded over ACP v2** (`testdata/transcripts/claude-v2`). This also closed the open question from milestone 0: subagents, questions, plans, compaction, steering, interrupt and background work all work on v2 through the Claude adapter.
- **`schema/`**: the pinned ACP v2 schema, the state rules extracted from it, and the daemon's extension schema.
- **`bridge-schema`**: ACP types from the official crate, the state rules as data, the extension types.
- **`bridge-core`**: the reducer, the session log with condensed replay, the transcript reader, and a plain-text rendering.
- **`testdata/vectors`**: one readable file per scenario showing what the reducer produced.

## The finding that shaped the reducer

The ACP v2 schema states, for each message, how it changes client state. Its `x-state` annotations say for example that `agent_message_chunk` *appends* `content` to the `agentMessage` with id `messageId`, that `tool_call_update` *upserts* the `toolCall` with id `toolCallId`, and that `state_update` *replaces* `state`.

So the reducer has no per-message code. It interprets those rules:

- 26 update kinds, one request (`session/request_permission`) and six responses carry rules.
- Four actions: append, upsert, replace, remove.
- A target is either a collection keyed by an id (messages, tool calls, terminals, plans, subagents, compactions) or a single value (state, usage, config options, session info).

Consequences:

- A client SDK in Swift or TypeScript needs the same small interpreter and the rules file, not a hand-written case per message.
- A newer v2 revision that adds a message kind needs no reducer change, only a regenerated rules file.
- The "items" idea from the design is the schema's own model; the daemon does not have to invent ids on v2.

What the rules do not say, and the core adds:

- **Order.** The core keeps a timeline: entries in the order they first appeared.
- **Requests.** Approvals and questions are JSON-RPC requests, not updates. The core tracks them as pending until the answer is sent.
- **Subagents.** A subagent's content arrives as updates for a child session id. The core keeps one state per session id; the subagent entry's id is that child session's id.
- **Terminal output.** It is streamed as base64, which cannot be joined as text. The core decodes, joins and re-encodes for the one field concerned.

## Checks, on all 17 transcripts

| Check | Result |
|---|---|
| Rendering equals the stored vector | pass |
| No update kind unknown to the rules | pass |
| Every update parses with the official ACP Rust types | pass, so the pinned schema file and the crate agree |
| A condensed replay gives the same state as the full log | pass; 723 updates condense to 441 |
| A client that applied half and then catches up reaches the same state | pass |

Plus 11 unit tests for the reducer and the log.

## Decisions taken while building

- **Direction of generation.** For ACP, the pinned schema file is the reference and the Rust types come from the official crate, which is the schema's upstream source; a test guards against the two drifting apart. For the daemon's extensions, the Rust types are the source and `schema/bridge.schema.json` is generated from them. This differs from the earlier wording "everything is generated from schema files" and matches how ACP itself works.
- **The reducer works on JSON, not on the typed ACP structs.** That is what lets unknown kinds and vendor extensions pass through untouched. The typed structs are used where code needs to read specific fields.
- **Transcript format fixed** (version 1): a header line, then one frame per line with direction, time and message. See `testdata/README.md`.
- **Seen in the data:** live message chunks arrive without an opening `agent_message`, so an append must create its entry. A thought and a reply can share one `messageId`, so an item is identified by target and id together.

## Not done

- No Pi data yet: Pi's adapter speaks v1, and translating it up is milestone 4.
- The extension schema is a draft. Nothing uses it until the daemon exists.
- Notices and errors have not been seen in a recording.
- TypeScript and Swift types are not generated yet (milestone 6).

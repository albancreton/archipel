# Milestone 4: adapter profiles, and Pi

_2026-11-22. Checked by `cargo test --workspace` (40 tests) and a live run with Pi 1.6.0 on `local/Qwen3.8-27B-BF16-DFLASH2` next to Claude in the same daemon._

## What a profile is now

A profile is whatever it takes for an adapter to look, to the daemon, like an ACP v2 adapter that hosts many sessions.

- **Claude:** almost nothing. Its adapter already is one. The profile sets one environment variable so reasoning is sent.
- **Pi:** a translator in `crates/archipel-agents/src/pi.rs`. It sits between the daemon and `pi-acp`, changes ACP v1 into v2, and runs one `pi-acp` process per session behind a single connection.

The daemon has no Pi-specific code. Everything that differs is at the adapter boundary.

## What the Pi translator does

| `pi-acp` (ACP v1) | What the daemon sees (ACP v2) |
|---|---|
| `session/prompt` answers when the turn ends | Answers at once with a `messageId`; `state_update` `running`, then `idle` with the stop reason |
| No echo of the user's message | `user_message` for each prompt |
| Message and thought chunks without ids | A `messageId` per run of chunks |
| `tool_call`, `tool_call_update` | `tool_call_update`, with the tool's name |
| Command output under `_meta.terminal_info`, `terminal_output`, `terminal_exit` | `terminal_update`, `terminal_output_chunk` |
| Diff as old and new text | Diff as a list of changed files and a patch |
| Config options keyed by `id` | Keyed by `configId` |
| Startup banner as an assistant message | Dropped |
| "Queued message (position 1)." and "Starting queued message. (0 remaining)" as assistant text | Dropped |
| `_meta.piAcp` (queue depth, running flag) | Dropped; the turn state comes from the prompts themselves |
| `current_mode_update` | Dropped; v2 has no modes |
| `session/load`, which always replays | `session/resume`; the replay is passed on only when asked for |
| A permission request with `toolCall` | The same with the tool call under `subject` (not seen from Pi; written from the two schemas) |

With several prompts queued, the session is `idle` once the last one has ended, not in between.

## One process per session

- A new session gets its own `pi-acp` (which starts its own `pi`).
- A process that has had no turn for `idleSeconds` (default 600) is stopped. The daemon is not told: for it the session stays live.
- The next request for that session starts a process, reloads the session with `session/load`, discards the replay, and then carries out the request.
- If a process dies during a turn, the translator reports the turn as interrupted.

## How it is tested

The twelve Pi recordings from the earlier probes are in `testdata/adapters/pi-v1`. A test plays each through the translator, with the fake adapter standing in for `pi-acp`, and stores the resulting v2 conversation in `testdata/transcripts/pi-v2`.

From there the Pi conversations are ordinary test data:

- The core's tests render them (`testdata/vectors/pi-v2`), check that no update kind is unknown, that a condensed replay and a catch-up give the same state, and that **every update parses with the official ACP v2 types**. That last one is the check that the translation produces real v2.
- The daemon's tests run them like Claude's: pass-through, replay, reconnect, adoption. 28 recordings in total.

A second test stops an idle session's process and checks that the next prompt starts another and that the reload's replay is not passed on twice.

## The live run

One daemon with both agents configured.

- `archipel new --agent pi`, then a prompt that reads a file and runs a command. Reasoning, both tool calls and the command's output streamed, ending `idle · end_turn`.
- After 32 seconds without a turn (`idleSeconds: 20`): no `pi-acp` process left. The session list still showed the session as live.
- The next prompt ("What did the echo command print?") started a process and Pi answered `hello-from-pi`: the context survived.
- `archipel sessions` listed 139 sessions: 25 from Pi, 114 from Claude.
- A Pi session from an earlier probe, never seen by this daemon, was adopted and rendered with its three turns.
- No process left after stopping the daemon.

## Limits

- **Pi has no approvals, questions or plans**, so there is nothing to show for them. A subagent is one tool call with its result, not a nested session.
- **A reload does not bring back reasoning** (Pi's own limit). The daemon's log keeps what it saw live.
- **Compaction** shows as the user message `/compact` and a notice; Pi refused it in the recording ("session too small").
- **Tool names:** Pi reports MCP tools with the tool name as the title and kind `other`. The server name is not available.
- **The translator reports no background work** for Pi. Not known whether Pi has any.
- **Model per session:** the configured model is selected when a session is created, not again after a reload. Pi keeps it in the session.
- **The Claude items from the design** that could not be tested stay open: todo lists (the installed Claude Code has no todo tool).

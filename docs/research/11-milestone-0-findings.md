# Milestone 0: closing the unknowns

_2026-11-22. Live runs with `docs/research/spikes/acp-probe/m0.mjs`; the raw logs are not kept in this repository. Claude: `claude` 2.1.330 on Haiku (one run on Sonnet), adapter `@agentclientprotocol/claude-agent-acp` 0.109.1. Pi: 1.6.0 on `local/Qwen3.8-27B-BF16-DFLASH2`, adapter `pi-acp` 0.0.35._

## Results

| # | Question | Claude adapter | Pi adapter |
|---|---|---|---|
| 1 | Several sessions in one adapter process | Yes. Two sessions streamed concurrently, updates correctly tagged. | No. Creating a session closes the others (`closeAllExcept` in its source). A prompt to the closed session returns `end_turn` with no content and no error. |
| 2 | Reloading a session after a restart | `session/load` replays everything, including the subagent's child session, and adds the user messages. `session/resume` restores context with no replay. | `session/load` replays user messages, tool calls and replies. Reasoning and the startup banner are not replayed. |
| 3 | Unanswered approval | Still valid after 150 s; approving then ran the command. No timeout observed. | Not applicable (no approvals). |
| 4 | Background tasks | With the JetBrains AIR capabilities declared: `async_task_spawned`, `async_task_progress` (with the output file path), `async_task_state_update`. The later wake-up turn is itself reported as a task. | Not tested; Pi has no background tool in this setup. |
| 5 | Reasoning | Appears with `CLAUDE_CODE_THINKING_DISPLAY=summarized` in the adapter's environment, or `_meta.claudeCode.options.thinking = {type: "adaptive", display: "summarized"}` on `session/new`. Effort and `showThinkingSummaries` alone did nothing. | Already worked. |
| 6 | Todo list | Not an adapter gap: this Claude Code install exposes no todo or task tool (the model listed its tools; none of TodoWrite, TaskCreate, TaskUpdate). The adapter's mapping to plan entries stays untested. | No such tool. |
| 6 | Compaction | Worked in the first round. | Still not shown: Pi refused ("session too small") even after two 900-line file reads. |

## ACP v2 against the Claude adapter

The adapter serves v2 (in preview since 2026-11-20) when the client asks for protocol version 2. One pass showed that v2 already provides most of what the design planned to add on top of v1:

| Design addition | In v2 |
|---|---|
| Turn state separate from the prompt call | `session/prompt` returns at once with a `messageId`. `state_update` reports `running`, `requires_action` (seen while an approval was pending) and `idle` with the stop reason. |
| Background work count | `state_update.background` lists running background terminals. |
| Stable item ids | Message and thought chunks carry a `messageId`; `user_message`, `agent_message`, `agent_thought` open each one. |
| Command output while running | `terminal_update` and `terminal_output_chunk` (base64), for foreground and background commands. On v1 Claude sent output only at the end. |
| Replay | `session/resume` with `replayFrom: {type: "start"}` replays the history with the same ids and ends with `replay_complete`. |
| Message while busy | A second `session/prompt` during a running tool was accepted and honoured, although the adapter's README says steering is v1 only. |

Also on v2: approvals have a `subject` (the tool call) instead of an embedded tool call; `session/set_config_option` takes `{type: "id", value}`; the mode is a config option; `cursor_update` marks fork points.

Not covered by v2 either: several clients on one session, catching up from a sequence number without a full replay, a remote transport.

Pi's adapter speaks v1 only.

## The ACP Rust crates

- `agent-client-protocol` 5.5.1 (Apache-2.0, repository `agentclientprotocol/rust-sdk`) offers three roles: Client, Agent and Proxy. The daemon is a proxy in its terms.
- v2 is behind the `unstable_protocol_v2` feature; a test crate with `unstable`, `unstable_protocol_v2`, `process` and `stdio` compiled.
- The schema crate (`agent-client-protocol-schema` 1.21.2) contains subagent updates, compaction, elicitation, plan updates, usage, session messages and notices.
- The JetBrains AIR updates (`async_task_*`) are not in it, as expected for an extension.
- Not checked: how the crate treats update kinds it does not know. The daemon must pass them through.
- 107 crates in the dependency tree with those features.

## Consequences for the design

1. **Adapter processes:** one per session for Pi; shared for Claude.
2. **The log is rebuildable** for both harnesses, with a known loss for Pi (reasoning).
3. **Pending approvals can wait** for an app to come back, at least on Claude.
4. **Declare the AIR capabilities to the Claude adapter on v1**, or use v2, to get background work.
5. **Set the thinking display** for Claude in its profile.
6. **Base the canonical model on v2.** Recommended: v2 shapes are the daemon's canonical form and what apps receive; Claude is driven over v2 nearly as-is; Pi's v1 stream is translated up to it. The risk is that v2 is a preview and will change.

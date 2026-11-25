# @agentclientprotocol/claude-agent-acp: 0.109.1 to 0.112.0

## In short

- **Message while busy changed.** A prompt sent while a turn runs is now pushed with priority `now`, which pre-empts the running generation, unless an approval or question is open (then `later`). Before, it waited in Claude Code's queue. `_meta.claudeCode.promptQueueing` is gone from `initialize`. Our README row for Claude and the `turn.message_while_busy` scenario depend on this, and nothing has been run against it.
- **Turn state changed.** `requires_action` now means the turn cannot progress without the user. If another foreground tool or subagent can still run, the turn stays `running`.
- **Wire shape is unchanged for us.** No new item kind, method, capability or state rule reaches our daemon. Rewind, session cursors and the AIR-only `_meta` keys are gated on the JetBrains AIR client, which we do not declare.
- **Work:** small in code, since I found nothing to change in the daemon, SDKs, kits or addons. Verification is the real cost: the behaviour that matters is covered only by turn-cost scenarios and recordings, and none of those ran. The seven free scenarios all match.

## What changed upstream

Changelog entries for 0.110.0 to 0.112.0, plus the diff.

**Prompts while busy (most important)**
- `dist/v2/agent.js`: `_meta: { claudeCode: { promptQueueing: true }, ..._meta }` became `_meta`. The removed comment read "A prompt that comes while Claude works waits in Claude Code's queue."
- `dist/v2/session.js`: a prompt sent while a turn is open gets `message.priority = steeringPriority(...)`. The new `steering-priority.js` returns `"later"` when a request is open and `"now"` otherwise. A `now` prompt goes into `steeredPrompts`, and `closeTurn` waits for it. The `v1/steering.js` comment that the diff removes says `now` "pre-empts the current generation".

**Turn state**
- `dist/v2/session.js`, `foregroundState()`: the doc now reads "`requires_action` means that the open turn cannot progress without user input. Another running foreground tool or subagent keeps it `running`." `dist/v2/tasks.js` changes the subagent-blocked check to match.

**Terminal output**
- `dist/v2/terminal.js`: `terminalBytes` now converts only bare LF to CRLF, keeps existing CRLF, and carries a trailing CR across chunks. The changelog entry is "preserve existing line endings in ACP v2 terminal output" (#1697).

**Rewind and cursors (gated to AIR)**
- `_session/rewind` (`SESSION_REWIND_METHOD`), `session-rewind.js`, `session-cursors.js` (`cursor_update` with a `rewind` capability), and fork honouring `rewind.json`. They are offered only to a client that declares `sessionRewind` or `session.cursors`.

**MCP**
- `mcp/message` gets `sessionId` and `toolCallId` (0.111.0; the schema change is in the ACP SDK update).
- #1708: start a session without the MCP servers Claude Code refuses (`refusedMcpServers`, `sideload_flags_disabled`).
- #1711: stop holding an MCP request that Claude Code gave up.
- `mcpServerStatus` notices can name several servers (`servers` in place of `server`).

**Other**
- `_meta.jetbrains.air.reasoningEfforts` on `model` option entries and `customAnswerFields` on elicitations: AIR clients only.
- Error messages lost their backticks (#1709), for example "configOptions sets … more than once".
- `initialize`: `capabilities._meta` no longer has `claudeCode`. Nothing else in `capabilities` changed.
- Dependencies: the adapter's `package.json` moves `@agentclientprotocol/sdk` to 2.12.0 and `@anthropic-ai/claude-agent-sdk` to 0.3.331. The watch summary's 0.3.332 is the SDK package's own latest, not what the adapter bundles.

## What it touches here

**Message while busy**
- `testdata/transcripts/claude-v2/steer.ndjson` was recorded on 0.109.1 with queued semantics. It would change if re-recorded.
- `conformance/scenarios.ts` `turn.message_while_busy` checks `secondMessageAccepted`, `secondMessageAnswered` and `userMessagesShown`. It does not check whether the first turn finished, so it could pass while the first turn is cut short. It has not been run.
- `README.md:106` says Claude's "Message while busy" is "yes (steering, advertised under `_meta`)". The advertisement is gone, and the behaviour changed. `docs/research/22-claude-terminal-vs-adapter.md` says "Queued, and cancelled with the turn". That is a dated research note.
- `docs/design/daemon.md:187` ("the adapter queues or steers") is still true. The daemon does not change.

**Turn state and approvals**
- `crates/archipel-server/src/daemon.rs` (approvals and `state_update` handling) passes `state_update` through. The Swift and React kits draw `requires_action` as "waiting for you". Behaviour changes only when a tool runs alongside an open approval or question.
- Recordings with approvals or subagents where another tool may run: `approval_command`, `approval_denied`, `question`, `plan`, `subagent`, `subagents_parallel`, `background`. Not checked.

**`promptQueueing` removal**
- `crates/archipel-server/src/daemon.rs:957-970` passes the adapter's capabilities to apps unchanged, so apps no longer see the flag. No code reads it: grep finds no use in `crates/`, `clients/` or `ui/`.
- All 27 Claude fixtures still carry `promptQueueing` in their `initialize` result: 17 files in `testdata/transcripts/claude-v2` and 10 in `testdata/daemon/claude-v2`. They record 0.109.1. The fakes replay them correctly, but they no longer describe the adapter.

**Terminal bytes**
- Only output that contains a CR changes, so LF-only recordings should be unaffected. I could not confirm that, because the frames are base64 and my decoding command was blocked. The `terminal_*` frames are in `approval_command`, `approval_denied`, `background`, `command_output`, `failing_command`, `files`, `interrupt`, `plan` and `steer`.

**Addons (`crates/archipel-addons`)**
- `acp.restore-settings` (`src/acp/restore_settings.rs`): it still applies, because `session.options` is still advertised. The diff does not change how `session/resume` handles `configOptions`; `initial-config-options.js` changes only the error text. The fault is not shown to be repaired, and the check that decides it, `reopen.keeps_settings.unaided`, is a turn scenario that has not run. The addon's `because` text cites "claude-agent-acp 0.109.1" and would need updating if it stays.
- `pi.check-setting-values`: Pi only, not affected.
- No addon can be removed on this evidence.

**Profiles (`crates/archipel-agents`)**
- `claude()` and the `CLAUDE_CODE_THINKING_DISPLAY` default: not touched by the diff.
- `Claude::elsewhere` (the `~/.claude/sessions` registry): `claude-code.diff` changes one line in `sdk-tools.d.ts`, so nothing there.
- The Pi translation from ACP v1: not affected. The daemon speaks v2 to Claude (`protocolVersion: 2` in `initialize.json`), and the v1 changes in `dist/v1/` are not reached.

**Schema, state rules, SDKs and kits**
- `schema/acp-v2.schema.json` and `schema/state-rules.json`: no change needed for this adapter. The new kinds (`cursor_update`, `agent_message_chunk` for handbacks) are AIR-gated, and the state rules are unchanged.
- `archipel-schema` uses only the crate's `v2` module, so the crate's v1 changes do not reach us.
- `clients/ts`, `clients/swift`, `ui/react`, `ui/swift`: nothing new to show. No kit reads `_meta.claudeCode`, `mcpServerStatus` or `cursor_update`.
- The ACP SDK update (`docs/updates/2026-11-24-acp-sdk.md`) concludes that no recording needs re-recording. It did not consider these adapter changes, so read the two reports together.

**Conformance (`conformance/baseline/claude.json`)**
- The seven free scenarios are the whole free set: `settings.listed`, `settings.switch`, `settings.rejects_unknown`, `session.empty`, `session.options`, `session.initial_settings`, `session.rename_delete`. All are `same`, with 8 sessions made and 8 deleted.
- The turn-cost scenarios that touch the changes have not run: `turn.message_while_busy`, `turn.interrupt`, `approval.answered`, `approval.two_apps`, `reopen.keeps_settings` and `.unaided`, `session.fork`, `session.mcp_server`, `shell.output_streams`, `background.listed`, `attach.late`.

## Proposed steps

1. **Record what was run, and leave the 0.109.1 baseline alone.** `docs/research/25` set the pattern: a newer adapter's run is kept beside the old baseline. The seven free scenarios are already `same`.
2. **Run the turn scenarios that the diff touches, live, on Haiku.** Use the commands in `conformance/README.md`. Before running, add a fact to `turn.message_while_busy` for whether the first turn finished, so the change shows. Read `reopen.keeps_settings.unaided` to decide whether `acp.restore-settings` can go. Expected: the message-while-busy fact changes, and the other scenarios stay `same`.
3. **Decide the message-while-busy policy.** Either accept the adapter's `now`, or restore queueing. Restoring it is a fix for an adapter behaviour, so by `AGENTS.md` it is an addon, not a core change. The addon interface can hold a request and release it from a later hook, so holding a second `session/prompt` until the turn is idle may be possible. That is untested.
4. **Update the docs that describe the old behaviour.** `README.md:106`, and the `because` text in `restore_settings.rs` once step 2 has settled it.
5. **Re-record only what step 2 shows to differ.** Start with `steer`, then the approval, subagent and background transcripts. Then run `cargo build`, `cargo test --workspace`, `pnpm -r test` and `swift test`, and read any `testdata/states` diff before accepting it.
6. **Check terminal bytes.** Decode the `terminal_*` frames in the nine transcripts listed above and look for CR. If there is none, keep them.
7. **Only then move the adapter pin** in the watcher config and in the docs that cite 0.109.1.

## What I could not judge

- **Whether `now` pre-empts in practice.** I read it from the code and from the removed comment. It has not been run, and `steer.ndjson` is the first thing to check.
- **How often `requires_action` changes.** It depends on whether any recorded turn runs a tool while an approval is open. I did not trace the recordings.
- **Terminal bytes in the recordings.** Unresolved, as described above.
- **Which Claude Code CLI the adapter drove.** The conformance config points `CLAUDE_CODE_EXECUTABLE` at `~/.local/bin/claude`, and the watch output does not record its version. The recordings used 2.1.330.
- **Parts of the diff I did not trace.** `dist/v2/session.js` is 190 changed lines and `dist/v2/tasks.js` is 70. I read the parts that bear on prompts, state and MCP, and not all of it. The "Closing the gaps for the ACP v2 support" fix (#1542) is opaque from the changelog.
- **The elicitation change.** `dist/v2/elicitation.js` now passes a `tool_call` context to `openRequest`. Whether that changes an `agent_request`'s shape or what cancelling a tool withdraws is unknown.
- **The crate's v2 changes.** `elicitation.rs` is documentation only. I did not read the changes to `v2/agent.rs`, `v2/client.rs` or `v2/content.rs`.
- **What the daemon does with an incoming `mcp/message`.** Not traced, the same open question as in the ACP SDK report. It matters only if an app sends an `acp` MCP server.
- **Effect on external apps.** Apps outside this repository may read `promptQueueing`, and I cannot see them.

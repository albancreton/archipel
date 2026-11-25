# claude-agent-sdk: 0.3.330 to 0.3.332

## In short

- This package is not a direct dependency of ours. The Claude adapter (`claude-agent-acp`) bundles it, and the adapter's 0.112.0 update moves it from 0.3.330 to **0.3.331**, not 0.3.332 (`target/watch/out/2026-11-24/claude-adapter.diff`, `package.json`, around line 3801). Nothing in the SDK diff reaches our daemon, the ACP wire, the state rules, the kits or the recordings directly.
- 0.3.331 is already inside the adapter's update. Its new `sideload_flags_disabled` start failure is what the adapter's #1708 fix handles (`claude-adapter.diff` around lines 230 to 244). We have no addon or code for it.
- 0.3.332 matters only when the adapter moves to it. Two changes need a look then: `Query.setPermissionMode()` now resolves to `{ mode }`, which breaks any code that types it as `Promise<void>`, and auto mode no longer drops mid-turn user messages when feature flags are not fetched. The second may bear on message-while-busy, which we only test live.
- Work for this repo: none for the SDK on its own. The real work belongs to the adapter update that would carry 0.3.332.

## What changed upstream

Taken from the changed lines of the diff and from `claude-agent-sdk.changelog.md`.

**Startup and managed settings (0.3.331)**
- `startup_failure_reason` gains `sideload_flags_disabled`, for a start that managed `disableSideloadFlags` refuses over a flag or launch option.
- `rewindFiles()` refuses under a managed `fileCheckpointingEnabled: false`, and no longer deletes files after a failed restore.
- `listSessions()` without a limit no longer opens every transcript at once. Before, sessions could be left out when the open-file limit was reached.

**Tool results and subagents (0.3.331)**
- Edit and Write tool results now report when `canUseTool` changed the text through `updatedInput`.
- `task_notification.output_file` names a foreground subagent's output file, as it already did for background subagents.
- `getCcrEvent()` gains `processing_at`. The browser SDK fixes `sse` queries and `Retry-After`. Both concern hosted remote sessions.

**Hooks (0.3.332)**
- A new hook event `PreSkillShellCommand`, with `PreSkillShellCommandHookInput` (`tool_name`, `tool_input.command`, `tool_use_id`, `command_name`, `invoked_by`) and `PreSkillShellCommandHookSpecificOutput`. It is added to `HOOK_EVENTS`, `HookEvent` and the hook output union. `updatedInput` is never applied.
- `tool_result_meta[].rewritten_by_plugin` is set when a mod's `session.append` hook changed a tool result.

**Messages and permissions (0.3.332)**
- `Query.setPermissionMode()` now returns `Promise<SDKControlSetPermissionModeResponse | undefined>`, carrying `{ mode }`, the mode in effect. Code that holds the promise as `Promise<void>` no longer compiles.
- Auto mode: mid-turn user messages were missing where feature flags are not fetched (clouds, telemetry off). They now count as the user's, and other relayed text must be marked `isSynthetic: true`.

**Other types, not relevant to us**
- `listedKind` on artifact listings, a `soft_error_result` flag on a tool-call type, `downloadsMirrorUrl`, and a Windows-only `registryPolicyFingerprint` (alpha). Several doc comments on settings sources and checkpointing were reworded.

## What it touches here

- **Dependency chain.** No `package.json` of ours lists `@anthropic-ai/claude-agent-sdk`. It appears only in `tools/watch/sources.json` and in the probe lock file `spikes/acp-probe/package-lock.json`, which pins 0.3.330. The adapter's imports (`forkSession`, `getSessionMessages`, `matchDeniedModels`, from `claude-adapter.diff` lines 63, 1050 and 1177) are not named in the changed lines of the SDK diff.
- **`schema/acp-v2.schema.json` and `schema/state-rules.json`.** No change needed. The SDK's changes are to its own hook, permission and tool types, not to ACP messages.
- **Addons (`crates/bridge-addons`).** None mentions an SDK name. `acp.restore-settings` relies on `session/resume` with `configOptions`, which is ACP, not the SDK. `pi.check-setting-values` is Pi only. No addon works around an SDK fault, so none can be removed on this evidence.
- **Profiles (`crates/bridge-agents/src/lib.rs`).** `claude()` (line 121) starts the adapter and sets `CLAUDE_CODE_THINKING_DISPLAY`. `Claude::elsewhere` (line 162 on) reads Claude Code's `~/.claude/sessions` registry, which the SDK diff does not describe. The Pi translation from ACP v1 is not touched.
- **Recordings and baseline.** `testdata/transcripts/claude-v2` (17 files) and `conformance/baseline/claude.json` were recorded with the 0.109.1 adapter, which bundles SDK 0.3.330. None of the recordings contains `output_file`, `tool_result_meta`, `rewritten_by_plugin`, `task_notification` or `isSynthetic`. The Edit and Write tool calls in `files.ndjson` and `plan.ndjson` involve no approval that edits the input, so the `updatedInput` change does not show in them. The SDK alone gives no reason to re-record. Re-recording depends on the adapter bump.
- **Client SDKs and kits (`clients/ts`, `clients/swift`, `ui/react`, `ui/swift`).** Nothing new to show. The hook and permission changes are not ACP items. `task_notification.output_file` could change what the adapter puts in a subagent's item, but I found no evidence that it does.

## Proposed steps

1. **Nothing to change in this repo now.** Leave `spikes/acp-probe` at 0.3.330; it is a probe, not the library.
2. **When an adapter release bundles 0.3.332** (check its `package.json` for `@anthropic-ai/claude-agent-sdk`), treat it as an adapter update. First run `cargo build`, then `bridge agents` against a real daemon, as `AGENTS.md` asks. It must print `ready` for Claude.
3. **Free checks.** Run `cargo test --workspace` and `pnpm -r test`. The recordings replay through the fake agent and should still pass if the adapter's wire is unchanged.
4. **Free conformance.** Run the seven free Claude scenarios listed in `conformance/baseline/claude.json` (`settings.*`, `session.*`) and compare with the baseline.
5. **Small paid check.** Run `turn.message_while_busy` and `approval.answered` live with the cheapest model (`session.setConfigOption("model", "haiku")`). The `isSynthetic` change is the one that could alter how a message sent mid-turn is shown.
6. **Re-record only if step 5 shows a change**, starting with `steer.ndjson` and `approval_command`, then update the baseline.

## What I could not judge

- **Whether the adapter forwards the new fields.** I could not tell whether the adapter passes `tool_result_meta`, `task_notification.output_file` or `processing_at` into ACP, or calls the SDK's `listSessions()` or `getSessionMessages()`. The adapter's source is not in the repo and the diff shows only changed lines. I found no installed copy of the adapter in the Homebrew or `/usr/local` locations, and `npm root -g` was refused as needing approval. Searching the installed adapter's `dist/` and the SDK's `sdk.d.ts` for these names would settle it.
- **Whether the `isSynthetic` fix changes our steering.** Only a paid live run on message-while-busy can show it. The free recordings cannot.
- **Whether the adapter registers any hooks.** The new `PreSkillShellCommand` event matters only if it does. The adapter diff does not mention `HOOK_EVENTS`.
- **The changelog is thin.** It does not explain `rewritten_by_plugin` beyond its one line, so I cannot say what an app would see.
- **Windows-only and remote-only items.** I judged `registryPolicyFingerprint`, `downloadsMirrorUrl`, `processing_at` and the browser SDK fixes irrelevant. That depends on which platforms we support, which I did not check.
- **The adapter's own ACP-visible changes** (steering, turn state, `promptQueueing`) are the subject of `docs/updates/2026-11-24-claude-adapter.md`. I did not re-verify that report here.

## What was found

- Source: `@anthropic-ai/claude-agent-sdk`, 0.3.330 to 0.3.332. Used by the Claude adapter (`@agentclientprotocol/claude-agent-acp`), which bundles 0.3.331 as of its 0.112.0 release.
- Read: `target/watch/out/2026-11-24/claude-agent-sdk.diff` (the changed lines, which cover `sdk.d.ts` and `sdk-tools.d.ts`) and `claude-agent-sdk.changelog.md` (0.3.331 and 0.3.332 entries).
- Also read: `AGENTS.md`, `schema/README.md`, `docs/design/daemon.md` (sections 5, 6 and 8), `crates/bridge-addons/src/acp/restore_settings.rs`, `crates/bridge-agents/src/lib.rs` (the Claude profile), and `claude-adapter.diff` and `claude-adapter.changelog.md` for the adapter's bump and sideload handling.

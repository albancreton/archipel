# claude-code: 2.1.330 to 2.1.332

## In short

- The only file the watch tool compares (`sdk-tools.d.ts`, per `tools/watch/sources.json`) gains one optional field, `listedKind`, on an artifact listing type. It is type-only, and nothing of ours reads artifacts. The Agent SDK note from the same day already judged this line irrelevant.
- The changelog (2.1.331 and 2.1.332) is mostly terminal, VS Code and Desktop work, Code Mode, and model routing on Bedrock, Foundry and Agent Platform. It says nothing about the `~/.claude/sessions` registry, the fields `Claude::elsewhere` reads, `CLAUDE_CONFIG_DIR`, or resume settings. No addon can be removed.
- The diff cannot show a change to the registry, because the watch tool compares only the declaration file. So the registry is "not mentioned", not "verified unchanged".
- Work: no code change. Adopting the version is a pin bump plus a few free checks, and a paid message-while-busy check, which the Agent SDK note already asks for. Re-recording is needed only if those checks show a change.

## What changed upstream

Declaration file (`sdk-tools.d.ts`, the only file compared):
- `ArtifactOutput`: a new optional `listedKind?: string` on the entries of an artifact's sharing list, beside `external`, `role`, `outsidersMayHaveWritten` and `pinned`. Nothing required was added or removed.

Changelog, grouped by theme (2.1.332 is lines 1 to 153, 2.1.331 lines 154 to 290):
- **Sessions and Remote Control:** "Fixed Remote Control showing a session in the desktop app, VS Code or the Agent SDK as running while it waited there on a dialog or an MCP server's question" (2.1.332). "Fixed local MCP servers seeing a new session ID after a Remote Control session's process restarts" (2.1.332). "Fixed headless and cloud sessions reporting a foreground fork's Agent call as finished the moment the fork started" (2.1.332). "Fixed a resumed conversation sometimes missing its last messages after a crash or kill on Bedrock, Google Cloud's Agent Platform, Foundry and other third-party providers" (2.1.332).
- **Hooks and skills:** a `PreSkillShellCommand` hook event, and skills from MCP servers (beta) that "show as `server:skill` in the `/` menu" (2.1.332).
- **Messages:** "Changed SDK and `claude -p` mid-turn messages to count as the user's where feature flags aren't fetched, as on clouds or with telemetry off, from Opus 4.8 on" (2.1.331).
- **Model routing:** on Bedrock Mantle, Agent Platform and Foundry, "terminal and VS Code sessions moved to Sonnet 5 or 5.5 ... start in auto mode if no mode is set" (2.1.331). `--model default` now steps down to an older model when the current one is unavailable (2.1.331). Also several `sonnet` and `opus` alias changes on the same clouds.
- **Not ours:** Code Mode, Desktop and VS Code UI, computer use, dictation, OpenTelemetry names, self-hosted runners, plugin marketplaces.

## What it touches here

- **`schema/acp-v2.schema.json` and `schema/state-rules.json`:** nothing. The changed type is not an ACP message, so the state rules stay as they are and `just gen` is not needed.
- **The open-elsewhere check** (`Claude::elsewhere`, `crates/bridge-agents/src/lib.rs`, around line 162). It reads `sessionId`, `pid`, `status` and `entrypoint` from `<pid>.json` in `~/.claude/sessions` (or `$CLAUDE_CONFIG_DIR/sessions`). The changelog does not mention that directory or those fields. Two items could change what the registry shows. Line 22 is about Remote Control displaying "running", and line 48 is about session IDs changing across restarts. Either could change the message text or which session matches. A dead pid is already skipped, because `started_by_this_process` returns `None` and `find_map` moves on.
- **Addons:** `acp.restore-settings` depends on `session/resume` with `configOptions`, and its `until` says the fix belongs upstream or in adapters. Nothing in the changelog does that, so it stays. `pi.check-setting-values` is Pi only.
- **Profile** (`claude()` in the same file): `CLAUDE_CODE_THINKING_DISPLAY` and the registry path are not mentioned in the changelog.
- **Recordings and baseline** (`testdata/transcripts/claude-v2`, 17 files, and `conformance/baseline/claude.json`): recorded with `claude 2.1.330` in their headers. The CLI bump alone does not require re-recording. The headless-subagent fix could change the order of updates in `subagent.ndjson` and `subagents_parallel.ndjson`, and the mid-turn change could affect `steer.ndjson` and `turn.message_while_busy`. Those would show only in a live run on 2.1.332.
- **Client SDKs and kits:** no new item kind. `listedKind` would appear only inside an artifact tool's output, which a kit would draw as a generic tool call, if it shows it at all. The `server:skill` names could reach the composer's command list if the adapter forwards them as commands. I did not check how the kits list commands.

## Proposed steps

1. Make no code change for this update. When the version is adopted, bump `current` in `tools/watch/sources.json` from `2.1.330` to `2.1.332`.
2. Read the registry without a model call. With a Claude session open in a terminal on 2.1.332, read `~/.claude/sessions/*.json`, and compare the keys with the ones `docs/research/20` lists for 2.1.330. Check that `cargo test -p bridge-agents` still passes. That test uses a synthetic registry, so it does not check the real format.
3. Check the open-elsewhere refusal for free. In a folder already trusted by Claude, open a terminal session the daemon lists, then send a prompt through the daemon. The expected result is error `-32004` before any model turn, with `openElsewhere` set. `docs/research/20` could not finish this with a real second terminal, because the trust dialog blocked it.
4. Run `cd conformance && node run.ts claude --free` against the same `claude` binary, and compare with `conformance/baseline/claude.json`.
5. Run a small paid check on Haiku: `node run.ts claude turn.message_while_busy approval.answered`. This is the check the Agent SDK note asks for. It costs a few cents.
6. Only if steps 4 or 5 show a change: re-record `steer.ndjson`, `approval_command`, `subagent` and `subagents_parallel`, then update the baseline with `--update`.
7. Do steps 2 to 6 once, together with the adapter update in `docs/updates/2026-11-24-claude-adapter.md`. That update changes prompt-while-busy, and this CLI version is the one it would drive.

## What I could not judge

- **The registry on 2.1.332.** The diff does not include the CLI's code, so I cannot tell whether the writer of `<pid>.json` changed. Step 2 settles it.
- **Remote Control status and session IDs (lines 22 and 48 of the changelog).** I cannot tell whether they change what a terminal session writes to the registry. Steps 2 and 3 should show it.
- **Headless subagent timing (line 47).** The fix may change the order of updates in our `subagent` recordings without changing their end state. Only a re-record or a live `subagent` run on 2.1.332 would show it.
- **Mid-turn messages.** The changelog says they count as the user's "from Opus 4.8 on" where feature flags are not fetched. We test on Haiku, and I cannot see the flag condition. Step 5 covers only part of it.
- **Auto mode and model defaults on Bedrock, Foundry and Agent Platform.** I cannot tell whether the adapter inherits them, or whether `session.initial_settings` or the `settings.*` scenarios would show them. They should not affect a first-party account.
- **`server:skill` names.** I cannot tell whether the adapter forwards them as ACP commands, or how a kit would show a name containing a colon. A recording or live run with an MCP server that exposes skills would show it.
- **Which binary conformance runs.** The conformance config points `CLAUDE_CODE_EXECUTABLE` at `~/.local/bin/claude`, and the watch tool does not record that binary's version. Its version needs checking before any conformance result is read.
- **The changelog's completeness.** I read both sections in full, but a changelog line may not say what an adapter or the registry sees.

## What was found

Source: @anthropic-ai/claude-code (`claude-code`), from 2.1.330 to 2.1.332.
Why we depend on it: The harness itself. The open-elsewhere check reads its registry of running sessions, which is not documented.

Files that matter to us changed (`target/watch/out/2026-11-24/claude-code.diff`):

```
{2.1.330.kept => 2.1.332.kept}/sdk-tools.d.ts | 1 +
 1 file changed, 1 insertion(+)
```

Its changelog for these versions is in `target/watch/out/2026-11-24/claude-code.changelog.md` (290 lines).

Files to read:
- `target/watch/out/2026-11-24/claude-code.diff`
- `target/watch/out/2026-11-24/claude-code.changelog.md`

Also read: `AGENTS.md`, `schema/README.md`, the parts of `docs/design/daemon.md` on ACP, profiles and addons, `crates/bridge-agents/src/lib.rs`, `crates/bridge-addons/src/acp/restore_settings.rs`, `tools/watch/sources.json` and `tools/watch/watch.ts`, `conformance/scenarios.ts` (names only), `docs/research/20`, `docs/updates/2026-11-24-claude-agent-sdk.md`, and the header and grep hits of `docs/updates/2026-11-24-claude-adapter.md`.

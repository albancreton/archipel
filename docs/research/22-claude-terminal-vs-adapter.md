# What Claude Code offers in its terminal, and what comes through the adapter

_2026-11-23. Asked by the user: "we need a list of what Claude exposes in the terminal UI but is not exposed via the adapter"._

How this was made: a research agent read the adapter's code, the Claude Agent SDK's type definitions, `claude --help`, the public docs and our probe logs. No agent turn was run. I spot-checked four of its claims in the adapter's code (client-supplied SDK options, the output poll for shell commands, rewind called only as a dry run, the reasoning environment variable) and they hold. The rest is as the agent reported it and has not been run.

Versions: adapter `@agentclientprotocol/claude-agent-acp` 0.109.1 (ACP v2 path, `dist/v2/`), Claude Agent SDK 0.3.330, Claude Code 2.1.330. File references are relative to the adapter's `dist/`.

How sure each line is: **code** = read in the adapter or SDK; **docs** = read in the docs or `--help`; **log** = seen in our probe logs; **ours** = from our own live tests; **inferred** = not read anywhere.

## Three findings that change what we believed

1. **An app can pass Claude Agent SDK options itself.** `_meta.claudeCode.options` on `session/new`, `resume` and `fork` is merged into the SDK's options (`v2/agent.js`, around line 875; code). The adapter keeps control of the folder, permission mode, tools and a few more; things like `thinking`, `sandbox`, `plugins`, `agents`, `settings`, `enableFileCheckpointing` and `fallbackModel` pass through.
2. **Shell output is not strictly "only at the end".** The adapter polls a running command's output every 500 ms (`background-output.js`, `TaskOutputPoll`; code). The poll only starts once Claude Code has made a task of the command, about three seconds in; a faster command delivers its output with the result. Our observation holds for short commands only.
3. **Reasoning is empty by default, not dropped.** The adapter emits reasoning only when its text is non-empty, and recent models hide it unless asked (`sdk-to-acp.js`; code). It is asked for by the environment variable `CLAUDE_CODE_THINKING_DISPLAY=summarized` (which our config already sets) or through finding 1.

## The table

| Feature | Through the adapter | Evidence | Note |
|---|---|---|---|
| Streaming text | yes | code | |
| Reasoning | partial | code; ours | See finding 3. No way to switch it during a session |
| Tool calls | yes | code | Dedicated handling for shell, read, write, edit, notebook, search, web, subagent, skill, plan; other tools are generic |
| Tool output while running | partial | code | See finding 2. Elapsed time arrives under `_meta`. Hook heartbeats and tool summaries are dropped |
| Diffs | partial | code | The exact patch arrives before the tool runs, the final diff after. No `/diff` panel; a per-turn report of changed files only for one specific client |
| Approvals | yes | code | "Always allow" choices come from Claude Code's own suggestions. A denial by rule becomes a failed tool call with its reason |
| Permission modes | partial | code; log | Offered: `default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`. `dontAsk` is not. No rule editor (`/permissions`) |
| Questions | yes | code; ours | As form elicitations. The tool is switched off if the app does not declare form support |
| Plan mode | partial | code | Entering, the approval and the approved plan all arrive. `/plan` itself is not listed |
| Todos | yes | code; ours | Mapped to ACP plan entries; the todo tool calls themselves are hidden |
| Subagents | partial | code; ours | Child sessions the app can watch and cancel. It cannot prompt, resume or fork a child |
| Background shells and tasks | partial | code | A `background` list on every state; a background command is a terminal fed every 500 ms; each can be cancelled. A running foreground command cannot be moved to the background |
| The turn a background task wakes | partial | code; ours | It starts with no user message, and is only marked as such at its end (`usage_update._meta["_claude/origin"]`) |
| Compaction, manual and automatic | yes | code; log | With the summary. **The adapter also runs a compaction of its own when a session sits idle, before the one-hour cache expires: the app sees a turn it did not start** |
| Slash commands: the list | partial | code | Built-ins, custom commands and skills in one flat list, kept current. Nothing says which is which |
| Built-in commands listed on this machine | partial | log | About 30 of roughly 110, among them `compact`, `context`, `model`, `effort`, `config`, `init`, `rename`, `usage`, `output-style`, `security-review`, plus `mcp` and `remote-control` which the adapter runs itself |
| Built-in commands not listed | no or unknown | code; log | Among them: `add-dir`, `branch`, `btw`, `clear`, `copy`, `cost`, `diff`, `export`, `help`, `hooks`, `ide`, `login`, `logout`, `memory`, `permissions`, `plan`, `plugin`, `resume`, `rewind`, `sandbox`, `skills`, `status`, `statusline`, `tasks`, `theme`, `voice` |
| A built-in sent as a message | partial or unknown | code; docs | The text is forwarded as typed and the command's output comes back as plain agent text. Which of the unlisted ones act is untested |
| Custom commands and skills | yes | code; log | No skills manager |
| **Rewind and checkpoints** | **no** | code | The adapter never restores files. Nearest thing: fork the session at an earlier point, which gives a new session and restores no code |
| Session list | partial | code | Title, folder, date. No search, tags or archive |
| Resume, fork | yes | code | Replay only from the start |
| Delete, rename, status, close | yes | code | The adapter has all four (`close` was not in what it advertised to us). Titles are also generated automatically |
| Model, effort, mode | yes | code; log | Settings `model`, `effort`, `fast`, `ultracode`, `mode`, and `agent` when custom agents exist. A model switched by a typed `/model` is followed |
| Usage, cost, rate limits | partial | code | Context used and size, cost, and rate-limit and quota details under `_meta`. The structured plan-usage data is only fetched on the old protocol version; on v2 `/usage` is text. API retries are not reported |
| Context breakdown | partial | code; log | `/context` answers as text. No structured breakdown by category |
| Memory (CLAUDE.md) | partial | code; inferred | The adapter asks for user, project and local settings; that CLAUDE.md is loaded is inferred. `/memory` is not listed, and which files were loaded is not reported |
| Hooks | partial | code; inferred | The user's hooks should run. Their start, progress and output are dropped; only warnings become a notice |
| MCP servers | partial | code | Servers given per session; `/mcp` to reconnect, enable or disable, with status as text; startup failures as notices. No lasting add or remove |
| MCP questions and sign-in | yes | code | As elicitations, including sign-in by URL |
| Images and files in a message | partial | code | Images (png, jpeg, gif, webp), PDF and text. No audio |
| `@` mentions | partial | code | A linked or embedded file is accepted. No completion service: the app lists files itself |
| Status line | no | log | Can be put together from model, mode, usage and cost. Folder and git branch are not provided |
| Output styles | partial | log; code | Can be set; the current one is never reported |
| Plugins | partial | log; code | Plugin skills load. No install or manage |
| Sign-in, sign-out, status | partial | code; log | Status and its changes are reported; sign-in and sign-out methods exist, but the terminal sign-in is offered only to an app that declares it can show a terminal. Our probe was offered none |
| Notifications | no | code | To be derived from the turn state |
| Prompt suggestions | partial | code | Only if the app declares it wants them |
| Message while busy | partial | code; ours | Queued, and cancelled with the turn. One queued message cannot be withdrawn |
| IDE integration, voice, `!` shell shortcut | no | code; log | `!ls` would reach the model as text (inferred) |
| Settings screens | partial | log; docs | `/config key=value` works by text. No call to read or write settings |
| Diagnostics | partial | log | `doctor` and `debug` are listed; `/status` is not |
| Sandbox | partial | code | Its network questions arrive as approvals. No switch, except through finding 1 |
| Worktrees and folders | partial | code | One folder per session, fixed. Extra folders can be changed between turns |
| Remote Control | yes | code | Run by the adapter |
| Terminal conveniences (`/btw`, `/copy`, `/export`, history search, themes, vim mode) | no | log | App work, except `/btw`, which has no equivalent |

## What the Agent SDK has that the adapter does not use

These could be had by patching the adapter or by driving the SDK directly.

- **Rewind**: `rewindFiles` for real, with `enableFileCheckpointing` and `resumeSessionAt`. This is the whole feature.
- **Structured usage and context**: the plan-usage call and the full context breakdown.
- **Move a running command to the background.**
- **Change the reasoning budget during a session.**
- **Read and write settings; list permission rules; list hooks.**
- **Reload plugins, skills and output styles.**
- **File suggestions** (for `@` completion); **withdraw one queued message**.
- **Archive, tag and look up sessions; list a session's subagents and read their messages.**
- **Messages dropped**: hook start, progress and output; API retries; notifications; reasoning-token progress; tool summaries; plugin installs; lost session writes; the active goal.

## Not determined, and the test that would settle each

Most of these cost nothing or one short turn on the cheapest model. They are candidates for the live conformance tier in `21-second-plan.md` (T3).

1. Which unlisted built-in commands act when sent as text (`/clear`, `/cost`, `/status`, `/memory`, `/rewind`, `/export`, `/help`, …): send each alone on a throwaway session and look at what comes back.
2. What the listed built-ins return (`/context`, `/usage`, `/config k=v`, …), and whether each spends a model turn.
3. Reasoning: with the option set, do reasoning chunks arrive for the model in use.
4. Shell output: a ten-second loop that prints once a second; chunks should arrive from about the third second.
5. Hooks: a test project with a blocking hook, a message hook and a denying hook; see which produce a notice, a failed tool call, or nothing.
6. Sign-in: declare terminal sign-in support and see what is offered.
7. The `dontAsk` mode: set it and expect a refusal.
8. A mode changed by Claude Code itself: does the settings update follow.
9. Images in tool results.
10. Whether CLAUDE.md really loads: a marker instruction in a test project.

## What this means for the library

- **The largest real gap is rewind.** It cannot be had through this adapter at all. The choices are to live without it, to patch the adapter, or to add it as an extension that drives the SDK or git itself.
- **Finding 1 is a door.** Sandbox, plugins, custom agents and reasoning can be set per session by the app today, with no change to the adapter. The daemon passes `_meta` through.
- **The adapter's own idle compaction** is a turn nobody asked for. Apps should not be surprised by it; it needs a test and a line in the docs.
- **The "partial" lines are mostly lists and screens the terminal has and the protocol does not** (settings, permission rules, plugins, hooks). Those are either text commands or the app's own screens.
- **Corrections to earlier notes**: the README's table says command output arrives "only at the end" and that todo lists were "not reproduced". Both are narrower than that: see finding 2, and the todo mapping is in the adapter's code though our Claude Code had no todo tool to trigger it.

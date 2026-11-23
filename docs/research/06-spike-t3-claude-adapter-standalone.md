# Spike: T3 Claude adapter run outside the app

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

> Historical: same caveat as 05.

**The question asked:**

> yes, repeat the experiment on the Claude adapter

---

The Claude adapter also runs outside the app, both on replayed transcripts and on two live Haiku turns through your installed `claude` 2.1.330. So Claude is a wrap too, but it cannot be lifted out as a package the way Pi can; you would vendor the T3 tree and import the adapter by path.

## What I ran

All of this used the `t3code-minitel` worktree (Nov 14 code), with nothing installed and no repo files changed.

| Run | Result |
|---|---|
| `simple`, `multi_turn` | Completed, transcripts fully consumed. |
| `subagent` | Two parallel subagents, each with its own child thread, tool items and reply; 11 subagent updates. |
| `tool_call_read_only_on_request` | Command item, pending approval request, my accept, command completed. |
| `turn_interrupt_mid_tool` | Command running, Stop, item and turn interrupted, thread reusable. |
| `web_search`, `command_output_streaming` | Web search item; command output arrived as a separate delta event. |
| Live turn 1 (Haiku) | Full turn in 4 seconds, first event after 34 ms. |
| Live turn 2 (Haiku) | Reasoning streamed as 22 updates, roughly every 60 ms while text was arriving. |

Each streaming update carries the whole text so far, not a delta. Your protocol would need to diff or accept that on the wire.

## What the live path needs

The adapter itself needs nine services, and the live SDK runner needs two more. All were satisfiable with layers T3 already ships:

- **Stock layers:** Node services, crypto, ID allocator, MCP sessions, installation gate.
- **Test doubles from T3:** a mock port scanner, an empty event logger, and the test server config (it only supplies an attachments directory in a temp folder).

The seam is clean: the adapter talks to the SDK through one service (`ClaudeAgentSdkQueryRunner`), which is what the replay swaps out.

## Why it can't be lifted as-is

Importing the adapter alone loads 49 files from `apps/server` (27,800 lines, 14,400 of them the adapter). Three import chains account for most of the unrelated weight:

- **Model catalog:** it pulls in server settings, then managed policy, then all seven source-control packages and Tailscale.
- **Skills:** they pull in the markdown and KaTeX stack.
- **Process tracking:** it pulls in the ACP runtime.

Import takes about 0.5 seconds and 245 MB resident memory, against 0.3 seconds and 200 MB for Pi.

## What this does not show

- **Live approval:** in live turn 1 the read-only policy I passed disabled Bash outright, so Claude never asked. Approval is verified on replay only.
- **Live subagents, questions, plans and todos:** not exercised live; subagents on replay only.
- **Current main:** the code is a week behind.

The two live turns left two small sessions in your `~/.claude/projects` under the scratch directory's name.

## Where this leaves the spike

Both of your main harnesses can sit behind the same adapter interface without the orchestrator. Vendor T3 as a subtree, import adapters by path, and merge upstream to keep the 165-commits-a-month fix stream. The cost is carrying the whole server tree for Claude until upstream extracts it or you cut those three chains.

The next step is step 2: the session engine that hides the orchestrator-shaped inputs, and the item schema for your protocol.

The runners are in `/private/tmp/claude-501/-work-t3code/dcaa0ec8-03b9-47a6-bdbe-90e81d7a6b80/scratchpad/spike-claude/` (`run-claude.mts` for replay, `run-claude-live.mts` for live); that directory is temporary.

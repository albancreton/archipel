# Spike: Pi through its ACP adapter

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

**The question asked:**

> if you source ~/.zshrc and relaunch pi, you will find a bunch of models to use, try `local/Qwen3.8-27B-BF16-DFLASH2`

---

Pi works through its ACP adapter on `local/Qwen3.8-27B-BF16-DFLASH2`, and better than its source suggested. Both of your main harnesses now run live behind the same 100-line ACP client, so ACP plus a daemon is viable for both.

## Pi through ACP

| Scenario | Result over ACP |
|---|---|
| Simple reply | Works; reasoning streams as thought chunks, then the reply. |
| Shell command | Works; no approval is asked, because Pi has none by default. |
| Read, search, edit | Works; tool kinds `read`, `execute`, `edit`, and the edit carries a structured diff. |
| Command output streaming | Works; output arrives line by line while the command runs. |
| Interrupt mid-tool | Works; tool marked failed, turn ends `cancelled`. |
| Steering while busy | Works as a queue; the second prompt ran after the first finished. The adapter announces this with plain assistant text ("Queued message (position 1)."). |
| Subagent | Partly; it is one opaque tool call titled "subagent" with progress text. The child's messages and tools sit in the raw output, not in a child session. |
| Compaction | Not shown; Pi refused with "Nothing to compact (session too small)". |
| Question, todo list | Not available; Pi has no such tools in this setup. |
| Web search | The model could not find the web-search tool and used your chrome-devtools MCP instead. MCP tools arrive as kind `other`. |

## Claude and Pi side by side

- **Reasoning:** Pi streams it; Claude showed none in my runs.
- **Command output:** Pi streams it under a `terminal_output` meta key (a Zed convention, not core ACP); Claude sends it only at the end.
- **Subagents:** Claude uses real child sessions; Pi gives one tool call you would have to unpack yourself.
- **Approvals, questions, plans, compaction:** Claude has all four; Pi showed none of them here.
- **Startup noise:** Pi's adapter injects a "pi v1.6.0 … Skills" banner as an assistant message at session start. It can be turned off with `quietStartup` in Pi settings.

So the standard part of ACP carries the common core for both harnesses. The differences live in `_meta` keys and conventions that a daemon would need to normalise.

## Side effects

- **Paid call:** the first Pi probe ran before I selected your model and made one short call on `huggingface/moonshotai/Kimi-K2.7-Code`, Pi's default. Everything after used the local model.
- **Chrome tab:** the Pi web scenario opened nodejs.org in Chrome through your chrome-devtools MCP.
- **Pi session files:** about a dozen were added under `~/.pi/agent/sessions`.

Raw logs for every run are in `/private/tmp/claude-501/-work-t3code/dcaa0ec8-03b9-47a6-bdbe-90e81d7a6b80/scratchpad/acp/logs/` (temporary).

## Next

The next step is the daemon design, which has two parts:

- **What ACP lacks:** session persistence, replay on reconnect and multi-client attach.
- **A normalising layer for what the adapters do differently:** command output streaming, subagent shape, background tasks and queue notices.

# Spike: Claude through its ACP adapter

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

**The question asked:**

> yes, run the ACP adapters against the T3 scenarios

---

The Claude ACP adapter passed 12 of 13 scenarios live, so "ACP plus a daemon" holds for Claude. Pi could not be run: Pi 1.6.0 is installed but has no provider login, so its adapter refused to open a session.

## Claude through ACP

I ran the official adapter (`@agentclientprotocol/claude-agent-acp` 0.109.1) against your installed `claude` on Haiku, driven by a 100-line raw JSON-RPC client with no SDK. The prompts are T3's scenario prompts or close variants.

| Scenario | Result over ACP |
|---|---|
| Simple reply, multi-turn | Works; text streams as chunks. |
| Command with approval | Works; permission request with allow once, allow always and reject options. |
| Read, search, edit | Works; tool kinds `read`, `search`, `execute`, `edit`, and the edit carries a structured diff. |
| Question | Works; arrives as a form with options and a free-text field, and the answer flows back. |
| Plan mode | Works; markdown plan, an "Approve Plan" permission, and a mode change event. |
| Subagent | Works; the child is its own session with its own tool calls and reply. |
| Interrupt mid-tool | Works; tool marked failed, turn ends `cancelled`. |
| Background command | Partly; the wake-up reply arrives after the turn, but there is no task item to render. |
| Steering while busy | Works; a second prompt during a running tool was accepted and honoured. |
| Compaction | Works; in-progress and completed events with the summary. |
| Web search | Works; tool kind `fetch`. |
| Command output streaming | Missing; output arrives only when the command finishes. |
| Todo list | Not reproduced; Haiku reported no todo or task tool in the session. The adapter has code that maps them to plan entries. |

Session state also comes through: context usage, cost per turn, rate-limit status, model and mode options, slash commands, session title, and prompt suggestions.

Four things would shape a client:

- **Updates outside the turn:** the subagent finished and the background wake-up arrived after the prompt had already returned. A client cannot treat "prompt returned" as "nothing more will happen".
- **Extensions beyond the standard:** steering and prompt queueing are advertised under `_meta`, and tool names and raw results sit under a `claudeCode` meta key. Background tasks as first-class items exist only behind a JetBrains-specific extension, which I did not enable.
- **No reasoning seen:** no thought chunks appeared in any run, though the adapter has code for them. Through T3's adapter the same model streamed reasoning, so this is likely a setting.
- **Default mode:** sessions started in `auto` permission mode, inherited from your Claude settings. I switched to `default` to see approvals.

## Pi through ACP

Untested live. From its source, `pi-acp` 0.0.35 is much thinner than the Claude adapter:

- **Emits:** message chunks, thought chunks, tool calls, usage, commands and mode updates.
- **Does not emit:** plans, subagents, compaction events, or questions.
- **Tool kinds:** only `execute` and `other`.
- **Status:** its README calls it an MVP centred on Zed.

For Pi, the likely path is your own bridge from Pi's RPC mode that emits ACP-shaped updates, with T3's Pi adapter and its recorded transcripts as the spec.

## What I left behind

- About 20 short Haiku sessions in `~/.claude/projects` and one plan file in `~/.claude/plans`, written by Claude itself.
- The probe, scenarios and raw logs in `/private/tmp/claude-501/-work-t3code/dcaa0ec8-03b9-47a6-bdbe-90e81d7a6b80/scratchpad/acp/` (`client.mjs`, `scenarios.json`, `logs/`). That directory is temporary and holds 430 MB of installed packages.

## Next

To run the Pi half, log Pi into a provider (`pi`, then `/login`) and tell me; it will make paid calls on that provider. Otherwise the next step is the daemon design: session persistence, replay on reconnect, multi-client attach, and how to carry the three gaps above (background tasks, command output streaming, todos).

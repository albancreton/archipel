# Claude provider changes, last 30 days

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

**The question asked:**

> let's look in detail what were the changes for the claude server specififcally for the last 30 days

---

The Claude code took 230 commits in the last 30 days, and almost all of it is hardening how T3 follows Claude Code's own behaviour, not new product surface. This is from commit titles and diffs sizes; I did not read the diffs themselves.

## Shape of the activity

- **Where:** 165 of the 230 commits touch `ClaudeAdapterV2.ts`. The rest land in the model catalog, skills discovery, text generation, usage readers and the driver.
- **Who:** 71 different authors. Julius Marminge wrote 106, Theo Browne 18, and most of the others one or two each.
- **What kind:** 183 fixes, 21 features, 9 performance changes, 9 chores.
- **How specific:** 149 titles name Claude. The other 81 are cross-cutting changes (lint rules, Effect upgrade, multi-repo projects, MCP apps) that had to touch Claude files, and 21 of those changed only tests.

## What the changes were about

Counts are keyword matches on titles, so they overlap and are approximate.

| Theme | Commits | Typical change |
|---|---|---|
| Hangs, timeouts and process death | ~29 | A turn that used to run forever now fails with a named cause |
| Usage limits, cooldowns, context meter, compaction | ~26 | Reset times recovered from proxy errors; meter follows the real auto-compact point |
| Resume, fork, revert, `/clear`, imported sessions | ~22 | A session gone on resume starts fresh instead of failing every send |
| Background work and self-started turns | ~20 | Monitors, scheduled wakeups and background commands keep the thread Working and wake it correctly |
| Subagents and delegated tasks | ~19 | Results, model and effort are reported correctly across turns and restarts |
| MCP | ~14 | Registration timeouts on busy hosts, MCP apps, elicitations, sign-in |
| Skills and plugins | ~10 | Skills from plugin dirs, managed plugins and claude.ai-synced plugins show in the picker |
| Steering | ~9 | An unread steer no longer hangs or gets lost; model and effort changes apply mid-turn |
| Launch arguments | ~7 | `--settings`, `--add-dir`, `--plugin-dir` and append text are honoured |
| Windows | ~4 | WinGet launchers, non-blocking process launches |

### Liveness and process lifecycle

This is the largest group, and it is all the same class of bug: the Claude CLI stopped, and T3 did not notice.

- A request that gets no answer now fails after 10 minutes (#37728).
- A CLI that exits before its first frame fails the turn (#33008); a killed process names its signal (#26086).
- A fresh session is retried once after a startup or MCP registration timeout (#37943), which rewrote about 500 lines.
- Jobs the CLI moved out of its process group are stopped at session close (#34505), and leftover Linux processes are reaped before recovery (#22834).

### Background work and wakes

- A cancelled queued continuation no longer drops later wakes (#30114), the biggest single fix at about 1,280 lines including tests.
- A thread waiting on a scheduled wakeup stays in Working (#23648).
- A Monitor line no longer wakes a delegated child's parent (#36339).
- The agent is woken when Claude exits while awaiting background work (#30214).

### Session continuity

- Imported sessions resume on their first turn (#23219) and send history once (#38575).
- Threads resume the session that `/clear` started (#35054).
- A running thread can now be forked natively (#37955), which resolves the mismatch I flagged earlier between the capability flag and the adapter.
- Stopped threads resume natively (#36526).

### Features that landed

- MCP Apps rendered from Claude tool calls (#29803, about 1,100 lines).
- PDF attachments sent as documents (#21358, about 1,280 lines).
- Bedrock runs the model picked in the composer (#24695, across seven files).
- MCP elicitations on the approval card (#21903) and MCP server sign-in from its warning (#39921).
- Ultracode with any effort level (#27034), Fable reset credits (#34106), and models listed from what Claude Code reports (#30511).
- Session name follows thread renames, and `/rename` retitles the thread (#34775, #37185, #36879).

### Keeping up with Claude Code itself

A steady trickle exists only because the CLI changed under T3:

- Full access rejects Claude Code's reworded dangerous-removal check (#38581).
- Fable 5.2 is offered only from Claude Code 2.1.321 (#36805).
- `ENABLE_TOOL_SEARCH=auto` no longer costs 10x cache writes (#24654).
- New Max and Ultra plan names (#27745).
- Replies stream paragraph by paragraph again (#35037).

## What this says about extraction

The churn is not refactoring toward a package boundary: only two refactor commits touched Claude in 30 days, and both were repo-wide. The adapter is still absorbing Claude Code behaviours one bug at a time, mostly from many one-off contributors, which makes a file move costly while it continues.

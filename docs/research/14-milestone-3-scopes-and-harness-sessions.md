# Milestone 3: scopes, the harness's own sessions, cleaner endings

_2026-11-22. Checked by `cargo test --workspace` (38 tests) and a live run with the real Claude adapter that made no model call._

## What was added

| Addition | In short |
|---|---|
| Scopes | A connection is `observe`, `interact` or `admin`. Observers read; their attempts to act are refused or ignored |
| Tokens with a scope | `bridged token add NAME --scope observe` for WebSocket clients; added and removed while the daemon runs |
| Harness sessions in the list | `_bridge/sessions/list` also asks each adapter (ACP `session/list`) and marks sessions that are not in the log |
| Adoption | Opening such a session makes the adapter replay its history, which becomes the session's log |
| `_bridge/sessions/updated` | Every connected app is told when a session appears or its title, turn state or liveness changes |
| Remembered setup | A session is resumed with the MCP servers and extra directories it was created with |
| Stale tool calls | When a turn is cut off, tool calls still waiting are marked `cancelled` |
| Retention | `retentionDays` in `config.json` removes unused sessions from the log |
| Agent readiness | `needs_login` when the adapter reports `unauthenticated` |

Fan-out to several apps and "first answer wins" were already done in milestone 2.

## Decision from the user

"List and adopt the harness sessions too" (2026-11-22). Before, the daemon only showed sessions it had created.

## How adoption works

1. `_bridge/sessions/list` returns the daemon's sessions (`logged: true`) and what each harness reports that the daemon does not have (`logged: false`). `loggedOnly: true` leaves the second group out and starts no adapter.
2. An app attaches to, resumes, or sends anything to a session with `logged: false`.
3. The daemon creates the session's record, starts the adapter if needed, and calls `session/resume` with `replayFrom: {type: "start"}`.
4. The adapter replays the history as ordinary updates. The daemon numbers and stores them like live ones. Requests that arrive for the session meanwhile wait.
5. When the adapter answers, the session is live and logged, and the waiting requests go ahead.

The same path rebuilds a log that was lost or pruned.

Adopting starts the harness, so it needs the `interact` scope. An observer can read a session once someone else has opened it.

## The live run

Fresh daemon home, real adapter, no prompt sent.

- `bridge sessions --logged`: 0 sessions. `bridge sessions`: 114, all marked as harness-only. The adapter answered the list in under 100 ms.
- An observer token over WebSocket: listing works; opening a harness-only session is refused (`-32003`).
- The owner opened the session recorded as `subagent` in `testdata`. The daemon adopted it: 24 log entries, including the subagent's child session nested under its parent.
- The rendering of the adopted session against the one recorded live differs in two places only: one line break inside a message, and the final turn state has no stop reason.
- The observer could then read the session, and its prompt was refused.
- No process left behind after stopping the daemon. The log file is readable by its owner only.

## What a harness replay does not bring back

Found by comparing recorded sessions with their replay (tests) and the live run:

- **Approvals and questions.** They were requests, and a harness replays updates. An adopted session shows the tool calls, not who allowed them.
- **The last stop reason.** After a replay Claude's adapter reports `idle` without one.
- **For Pi:** reasoning (known since `docs/research/10`).

So a session watched live through the daemon has a richer log than the same session adopted later. Nothing is wrong in either; they are not identical.

## Decisions taken while building

**Scopes are checked in one place.** A request is either one of six reading methods or it acts. Notifications and answers always act. `admin` exists in the type and nothing needs it yet; pairing and configuration will.

**The local socket is the owner.** Anyone who can open it already is the user. Scopes apply to WebSocket tokens.

**Tokens are stored as they are**, in files only the owner can read (`ws-token` for the owner, `tokens.json` for the others). Storing only a hash would be better and belongs with pairing in milestone 7.

**The transport does not know scopes.** The WebSocket server asks a function what a token grants and hands the answer over with the connection. The daemon program decides that a grant is a scope.

**Session summaries go to every connected app**, not only attached ones. An app showing a list of sessions is attached to none of them.

**Cancelling stale tool calls reads the log.** The daemon keeps no item state in memory; when a turn is cut off it folds that session's log once to find tool calls still `pending`, `in_progress` or `paused`. Rare, and it keeps memory flat.

## Not done

- **Windows.** The local transport is still a Unix socket only. A named pipe that only the owner can open needs an explicit access list, which in Rust means the Windows API directly (the workspace forbids `unsafe`), and it cannot be tested on this machine. Writing security code that has never run seemed worse than not having it. It needs a Windows machine and a decision on allowing one audited `unsafe` block or a small dependency.
- **A session open in two places.** Adopting a session that is also running in the harness's own UI puts two processes on one transcript. Not tested, and nothing prevents it.
- **Harness-only sessions do not update by themselves.** `_bridge/sessions/updated` covers the daemon's sessions. A session started in the harness's UI appears on the next list.
- **Paging** of `_bridge/sessions/list`: everything is returned at once (114 sessions was 30 KB).
- **Terminals of a cut-off turn** still read as running. Tool calls are fixed; a terminal has no "cancelled" state to set without inventing an exit status.
- **Settings after a wake-up** are still unchecked (does Claude resume in the mode set before?).
- **Stored chunks are not merged**; retention is by age only.

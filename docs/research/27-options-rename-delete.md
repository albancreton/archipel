# Settings before a session, rename and delete, through the daemon and the SDKs

2026-11-24. Second step of the second plan (`21-second-plan.md`): the session list (A6) and the settings bar (A2) need `session/options`, `session/set_title` and `session/delete` to work through the daemon.

## What was already there

More than the plan said. The daemon forwards any request it has no rule for, so all three calls already reached the adapter, and `session/delete` got its own rule on 2026-11-23. What was not known was whether the result was right, so the first thing done was a live scenario, before any code.

**`session.rename_used` (new scenario).** A session with one turn is renamed through the daemon, then gets another turn, then is renamed again after the daemon restarted, when no process holds it.

| | Claude Code | Pi |
|---|---|---|
| Says it can rename (`session.title`) | yes | no |
| Rename | ok, title stored as sent | error -32601 |
| The session itself shows the new title | yes: the adapter sends a `session_info_update` | |
| The daemon's list shows it | yes | |
| The harness's own list shows it | yes | |
| Kept after another turn (no automatic title over it) | yes | |
| Rename while no process holds the session | ok, and the three places above agree | |

So renaming needed nothing in the daemon: it wakes the session like any other request, the adapter announces the new title as an update, and the existing code that keeps titles for the list does the rest. A rule that renamed without waking the session was considered and not written: the adapter could then not announce the title, the daemon would have to write that update itself, and what exists works.

## What was changed

**`session/options` now goes to the right agent and wakes nothing.** Before, a call without a session always went to the first agent, so the settings of a second one (Pi, next to Claude) could not be read; and a call with a session went down the general path, which resumes the session first and refuses one that is open elsewhere. The protocol says this call must not resume anything. It now goes to the session's agent when a session is named, to the agent under `_meta.archipel.agentId` otherwise (the same field `session/new` uses), else to the first, and is passed straight on.

**The agent list says what each agent can do.** `_archipel/agents/list` gains `capabilities`: what the agent answered to `initialize`, once it has been started. Without it an app cannot know that Pi has no rename, and would only find out from a failed call. `initialize` on the daemon still gives the first agent's capabilities, as before.

**The SDKs have the three calls**, in TypeScript and Swift: `sessionOptions`, `setTitle` (returns the title the harness stored), `deleteSession` (also stops following the session; Swift also takes it out of its list). Swift gains `agents()`, which it lacked.

## How it is checked

- Rust, `tests/options.rs`: two agents; a call naming the second starts only that one; the answer comes back unchanged; the agent list then has its capabilities; a call naming a session only the harness knows is passed on with no resume before it; an unknown session or agent is refused without asking anyone.
- TypeScript and Swift: a test each, against a stand-in that records the calls.
- Live: `session.rename_used` on both harnesses, and `session.options` now also reads an existing session's settings through the daemon. Both baselines gained these facts; nothing recorded before changed.

## Left open

- **Paging of the session list**, which the plan lists next to rename and delete for A6. Not done: the daemon returns the whole list today. To do when a list is long enough to need it.
- Pi's settings without a session: `session.options` runs for Claude only. Pi's adapter was not asked whether it has `session.options`.

Cost: four Haiku turns, and two turns on Pi's local model.

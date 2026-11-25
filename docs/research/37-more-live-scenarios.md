# More live scenarios

2026-11-24. Step 5 of the second plan, second half: behaviours that were "seen once by hand" or "never tested" in note 21 are now conformance scenarios, run against the real harnesses and recorded in the baselines. Eight scenarios, 27 in all. Claude on Haiku; Pi on the local model.

## What was added and what it found

| Scenario | Claude | Pi |
|---|---|---|
| `approval.answered`: refuse, then allow | Refused: the command does not run, the tool call is `failed`, the turn ends normally, the approval shows the answer. Allowed: it runs. Options offered: allow once, allow always, reject once | not run: Pi asks nothing |
| `approval.two_apps`: two apps answer at the same moment, one no and one yes | The first answer (no) won for both apps, the command did not run, and both apps ended with exactly the same session | not run |
| `sessions.concurrent`: three sessions, one turn each, at once | Each got its own answer, none crossed (2.4 s) | The same (10 to 15 s) |
| `session.mcp_server`: an MCP server given at creation | The agent finds the tool (after a tool search) and calls it; its text comes back. No approval was asked | **The session is created without complaint and the server is ignored**: the agent has no such tool |
| `session.fork` | The copy shows the history at once and knows it; the original's items are untouched; both are listed | **Not supported** (`-32601`) |
| `background.listed`: a command left running | Listed after the turn (as a terminal), and no longer listed once it ended | **Not listed**: Pi starts it with `nohup … &` in its shell tool and knows nothing more about it |
| `attach.late`: one app attaches during the turn, one after | Both end with exactly what the first app has | The same |
| `compact.command`: the user sends `/compact` | A compaction item appears; the session still knows what was said | No item and no reply; the session still knows (nothing was compacted, or it was and says nothing) |

These were all `must` or fact checks on structure; none depends on the wording of an answer beyond a marker word.

## What this settles

- **"First answer wins" holds live**, with two real connections and a real pending approval. Until now it was proven on recordings only.
- **An app that joins late sees the same session**, including one that joins in the middle of a turn. Also only proven on recordings before.
- **The daemon keeps sessions apart under load** on both harnesses (three at once; more was not tried).
- **Fork works end to end on Claude.** While checking, the original session's values did change after the fork: Claude names a session a moment after its first turn, and refreshes its command list. Nothing from the copy reaches the original.

## What an app has to know about Pi

Three differences an app would otherwise find out the hard way:

1. `mcpServers` at session creation are accepted and dropped. Pi has no MCP support of its own; its users add it with an extension. The answer to `session/new` does not say so. An app can know beforehand: Pi's capabilities have no `session.mcp`, Claude's have `session.mcp.stdio`, `http` and `acp`. The daemon could refuse servers for an agent that does not advertise them, so the mistake is loud; **proposed, not built**.
2. No fork (`session.fork` is not in its capabilities either): an app must look before offering it. The React and SwiftUI kits only offer fork in the open-elsewhere notice, which Pi never triggers.
3. No background list and no compaction item.

## Loose ends

- **One unexplained timeout.** Pi's `background.listed` ran five times: four took 4 to 6 seconds, one hung until the 240-second limit. The model may have started the command differently that time; the scenario does not record enough when it times out to say. It is in the baseline from a good run and may fail again.
- Pi cannot delete sessions, so each run leaves its sessions in Pi's folder for `conformance/work/pi` (already the case before).
- Still not live scenarios: extra folders at session creation (the TypeScript and Swift SDKs do not pass `additionalDirectories` yet), a slow client being disconnected, a session of several thousand items, the cost of the first message after reopening a long session, a process that opens a session between the daemon's check and the prompt, and a full disk with the real store.
- The newest adapter version was not tried (as agreed).

## Cost

Three runs of the new scenarios on Claude while writing them: roughly 45 short Haiku turns. Pi is local and free.

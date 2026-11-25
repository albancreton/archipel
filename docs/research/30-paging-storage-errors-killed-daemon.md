# Paging, an error for a log that cannot be written, and adapters after a kill

2026-11-24. Three items left open by `27-options-rename-delete.md` and `29-fault-tests.md`, decided by the user the same day.

## Session list in pages

`_archipel/sessions/list` takes `limit` and `cursor` and answers with `nextCursor` while sessions are left. The fields were in the schema since the first plan and did nothing.

- Without `limit` the answer is everything, as before. Nothing that exists had to change.
- The cursor is the time and the id of the last session of the page (`<updatedAt>|<sessionId>`, opaque to apps). The next page is what comes after that place in the order: newest first, by id among sessions of the same time. So the daemon keeps nothing between two pages.
- A session that changes between two pages moves to the front and is in no later page. The app already has it from `_archipel/sessions/updated`.
- **What paging does not do:** each page still asks every harness for its whole list, because the daemon's sessions and the harnesses' must be merged before they can be cut. Pages make the answer smaller, not the work. Keeping a harness's list between pages would need a rule for when it is stale; not done until a list is slow enough to matter.

SDKs: `sessionsPage` in TypeScript and Swift. `listSessions` and `refreshSessions` are unchanged.

## A log that cannot be written

Before: updates were dropped, no app was told, only stderr said so. The user's decision: the app must get an error it can show.

The daemon's errors are JSON-RPC errors (`code`, `message`, optional `data`), with two codes of its own so far: `-32003` scope, `-32004` open elsewhere. Added: **`-32005`, the session log cannot be written.**

- **A prompt is refused with it**, and the agent never gets the message. The daemon tries a write (the session's record) before it passes a prompt on. A turn that cannot be kept is not started, and the user's text stays in the composer of the kits, which show "Not sent: …".
- Requests that failed on the store with the general `-32603` (`session/new`, `session/fork`, adopting a session, `_archipel/session/append`, the list) now use `-32005` too.
- **In the middle of a turn there is no request to refuse.** Prompts are answered when the agent has taken them, not when the turn ends. So the attached apps get `_archipel/session/error` with `sessionId`, `code` and `message`: the same error, without a request. Once when the first write fails, not for each lost update. It is not stored (it could not be). The SDKs keep it as `session.error` until the next update arrives, and the chat views show it.
- The turn is not stopped. Stopping it would lose work the harness still records itself, and the daemon could not write the reason into the log anyway.
- Still true: what cannot be stored is not sent, so no app holds more than the log.

Not checked: a really full disk. The test uses a store that fails every write; that SQLite refuses the trial write on a full disk is expected, not seen.

## Adapters when the daemon is killed

The user's view: they should be killed too. Measured before writing anything (macOS, Claude adapter 0.109.1 and `pi-acp`, one Haiku turn and one local Pi turn, each running `sleep 300`):

| Moment of the `kill -9` | Left a few seconds later |
|---|---|
| Sessions idle | Nothing: adapter, harness and Pi's MCP servers all gone |
| A turn running a shell command in each harness | Nothing: the `sleep` processes too |

They stop because the adapter reads from a pipe that closes with the daemon. So the daemon does what was asked without new code, and nothing was added. A second mechanism (a parent-death signal on Linux, a list of process ids to clear at the next start) would need `unsafe` code or kills by recorded id, for a case that did not occur. If an adapter ever ignores its closed stdin, that is the moment; under systemd the control group already covers it.

## Numbers

106 Rust tests, 50 TypeScript, and one more Swift test.

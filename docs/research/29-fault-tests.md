# The daemon when things around it fail

2026-11-24. Second half of the third step of the second plan: the test tier called T2 in `21-second-plan.md`. Free, no model, run with every change.

The rule the tests hold the daemon to: **whatever fails, an app that asked something gets an answer, and the daemon carries on.**

## What was already covered

- The adapter dies during a turn with an approval open: the turn and the approval are closed, the next prompt starts a new adapter (`tests/daemon.rs`).
- The daemon is shut down cleanly in the middle of a turn and started again on the same log.
- An app loses its connection and catches up.
- Two apps answer one approval: the first answer counts.
- Everything about addons when the adapter stops or an app leaves (`tests/addons.rs`, `tests/app_side.rs`).

## What was added

`crates/bridge-server/tests/faults.rs`, with adapters scripted by the test:

| Test | What fails | What must hold |
|---|---|---|
| The adapter dies at each step | It dies while the app waits on `initialize`, `session/new`, `session/prompt`, the wake-up of a sleeping session, the session list, `session/delete`, `session/options` | The waiting request is answered (an error; for the list, the daemon's own sessions). The next request starts a new adapter and gets through |
| The adapter cannot be started | The program is missing | The request is refused with the reason, the agent list says `error` with the same reason, reading sessions still works, and the next request tries again |
| The adapter sends what it should not | Text that is no message, answers to questions nobody asked, updates without their fields or about a session that does not exist, a request about such a session | Nothing reaches an app, the request is refused so the adapter is not left waiting, the real session still works |
| An app stops reading | More updates than its queue holds (4096) | It is disconnected, an app that does read gets everything, and the slow one gets exactly the rest in order when it returns |
| The store fails | Every write fails for a while | Requests are still passed on and answered, what could not be stored is not shown to any app, and numbering continues without a gap when the store works again |

`crates/bridged/tests/killed.rs`, with the real program:

| Test | What fails | What must hold |
|---|---|---|
| Killed in the middle of a turn | `SIGKILL` while an approval is open; the next daemon starts over the stale socket file | The list no longer says the session is running or waiting; its history ends with the turn stopped and the approval cancelled |
| A damaged log file | `log.sqlite` is not a database | The daemon stops with a message that names the file and says what to do, and leaves the file as it is |

## What the tests found

Two faults of our own, fixed in the core:

1. **After a kill, the session list showed the session as waiting for an approval, for ever.** The daemon closes an interrupted turn when the session is first opened, so the history was right for whoever opened it, but the list reads the stored summary and nobody may ever open the session. The daemon now closes interrupted turns when it starts.
2. **A damaged log gave "store: file is not a database"**, without the file's name. It now names the file and says that moving it away starts an empty log.

Everything else passed as written.

## What the tests show and leave open

**A store that fails loses events without telling anyone.** While writes fail, updates are dropped: no app is sent them, which keeps the rule that no app ever holds more than the log, but the user's own message of that time is not in the history either, and no app is told that anything is wrong. The daemon only writes a line on stderr. The test records this as it is. What should happen on a full disk is a decision that has not been made: the choices are to tell every app once (a notice that is not stored), or to stop the turn. Open.

**A notification for an adapter is dropped if the adapter's queue is full** (4096 messages it has not read). A request in the same situation is treated as "the agent stopped responding" and the adapter is restarted; a notification such as `session/cancel` is dropped silently. No test, since an adapter that far behind is already lost. Noted.

## Not covered

- A real full disk with SQLite, and a log file that is damaged in the middle rather than at the start.
- Whether adapter processes are left behind when the daemon is killed. They read from a pipe that closes, so they should stop; under systemd they are stopped with the service. Not checked.
- Many sessions at once, and a session of several thousand items end to end through an SDK.
- The SDKs against a daemon that fails: their reconnect is tested against stand-ins (`28-sdk-gaps.md`).

## Numbers

105 Rust tests (97 before the second plan's first step), 49 TypeScript, 33 React, 4 for the Node examples, and the Swift suites, all passing. The addon coverage check still reports no unrun line.

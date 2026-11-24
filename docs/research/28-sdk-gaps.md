# The two SDK gaps: a deleted session, and starting the daemon from Swift

2026-11-24. First half of the third step of the second plan. The second half is the fault tests.

## A session deleted by someone else

The daemon has told every app about a deleted session since 2026-11-23 (`_bridge/sessions/removed`). Neither SDK did anything with it: the session stayed in Swift's list, and a `Session` object an app held went on looking alive.

Looking at it turned up a real fault next to the gap. **A session deleted while the connection was down stopped the client from ever coming back.** After a disconnect both SDKs attach again to every session they follow. The daemon refuses one it no longer knows, the SDKs treated any refusal as "not back yet", and tried again for ever. In Swift each try also opened a new connection.

What the SDKs do now, the same in TypeScript and Swift:

- On `_bridge/sessions/removed`: the session gets `removed = true`, its listeners are called once, it is no longer followed, and the app is told (`onSessionRemoved` in TypeScript; in Swift the session also leaves the observable `sessions` list).
- A wait with `until` ends. Nothing will change after a delete, so a wait would never return. The caller checks `removed`. It ends rather than fails so that Swift's `until`, which cannot throw, and TypeScript's behave the same.
- What the session held stays readable. Acting on it fails at the daemon ("unknown session").
- On reconnect, a session the daemon refuses as unknown (code -32602) is treated as removed, and the others are attached as before.
- `deleteSession` marks the session at once, without waiting for the notification.

One case where "removed" is not exactly true: a session older than the daemon's retention time is dropped from the daemon's log but still exists at the harness. If the daemon restarts while an app follows such a session, the app is told it was removed. The next session list shows it again as "in the harness only", and it can be opened. Rare, and the app loses nothing.

The two demos now close a session that is deleted under them.

## Starting the daemon from a Swift app

`examples/swift/Daemon.swift`, the counterpart of `examples/node/daemon.ts`, with the same rule: if a daemon answers on the socket it is used, otherwise one is started and waited for. It is an example and not an SDK function for the reason given in `25-examples-for-agents.md`: who owns the daemon and when it stops are the app's choices. It is a target of the Swift package so that it stays compiling and has tests.

Two things differ from Node:

- macOS only. An iOS app cannot start programs.
- A child process is not stopped when a Mac app exits. The example stops the daemons it started from an `atexit` handler, which covers an ordinary quit and not a crash. After a crash the daemon is still there, and the next start finds it and uses it, so nothing piles up.

## How it is checked

- TypeScript, `test/sessions.test.ts`: a session another app deleted is marked, its wait ends, the app is told, another session is untouched; a session deleted while the connection was down is marked after the reconnect and the client is back.
- Swift, `SessionsTests.swift`: the same two.
- Swift, `examples/swift/Tests`: a daemon is started with the fake agent and answers an app; a second call finds it and starts nothing; a start that fails says why, with the program's last words.

Not checked against the real daemon end to end: the SDK tests use a stand-in. The daemon's side (it sends the notification, it refuses an unknown session with -32602) is in `crates/bridge-server/tests/delete.rs`.

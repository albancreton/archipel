# Where things stand

_2026-11-22, after milestone 7. One place for what exists, what was checked, and everything still open._

## What exists

| Part | Where | Size |
|---|---|---|
| Data model: pinned ACP v2 schema, state rules, extension schema | `schema/` | |
| Rust: core, store, transports, adapter profiles, server, daemon and command-line client | `crates/` (8 crates) | 7,800 lines |
| TypeScript client SDK and React kit | `clients/ts`, `ui/react` | 1,600 lines |
| Swift client SDK and SwiftUI kit | `clients/swift`, `ui/swift` | 1,500 lines |
| Recordings and reference results | `testdata/` | 29 sessions |
| Design, research notes, remote guide | `docs/` | |

The daemon is one 4.8 MB program with no runtime to install.

## What was checked

| Suite | Tests | Covers |
|---|---|---|
| `cargo test --workspace` | 44 | Reducer, log, store, transports, Pi translation, the daemon against 28 recorded sessions, extensions, pairing, the command-line client |
| `pnpm -r test` | 77 | The TypeScript reducer against every reference state, the client against the real daemon, every session drawn by React |
| `swift test` | 6 | The Swift reducer against every reference state, the client against the real daemon, sessions drawn by SwiftUI |

Live runs with real harnesses: Claude Code (approval, daemon killed and restarted mid-approval, catch-up, wake-up, adopting sessions) and Pi on a local model (a turn with tools, idle stop and wake-up, adoption), both in one daemon. A browser page answering an approval.

## Decisions waiting for you

1. **The name.** "harness-bridge" is temporary; it is in crate names, package names, the copyright line and the daemon's home directory.
2. **Windows.** A local pipe only its owner can open needs the Windows API directly: one audited `unsafe` block or a small dependency, and a Windows machine to test on.
3. ~~A session open in two places.~~ Decided and built: block by default, offer to fork (`docs/research/20`). Claude only; Pi has no way to tell yet.
4. **Pi, later:** keep `pi-acp` behind the translator, or speak Pi's own protocol directly. The translator works; the direct route would remove a dependency and a process per session's adapter.
5. **Publishing:** npm packages, a tagged Swift release, binaries. Nothing is set up.
6. **ACP v2 is a preview.** The pinned revision is from the SDK of 2026-11-20. Moving to a newer one is one deliberate change (schema, rules, recordings), and should be scheduled, not stumbled into.

## Known gaps, by area

**Daemon**

- Storage errors drop the event with a line on stderr; apps are not told.
- SQLite writes happen on the daemon's one task.
- Stored chunks are not merged; retention is by age only.
- Stopping an adapter is abrupt (closed pipes, then a kill). No process was ever left behind in the live runs.
- The Unix socket path is limited to about 100 bytes by the operating system.
- Agent readiness is coarse: `ready`, `needs_login` (when the adapter says so) or `error`.
- Paging of the session list is not implemented.
- Harness-only sessions appear on the next list, not by notification.
- After a wake-up, whether Claude keeps the mode that was set before is unchecked.
- A cut-off turn leaves its terminal items reading "running" (tool calls are marked cancelled).

**Adapters**

- An adopted session lacks its approvals and questions; a harness replays updates only.
- Claude: todo lists could not be tested (the installed Claude Code has no todo tool). Notices from the adapter were never seen.
- Pi: no approvals, questions or plans exist to show; a subagent is one tool call; a reload loses reasoning; compaction and background work untested.

**Extensions**

- Extensions cannot block or change the flow.
- Observers cannot call extension methods, even reading ones.
- Apps are not told when an extension comes or goes.

**Clients and UI**

- React Native and iOS were not run; only Node, macOS and one browser.
- Markdown is not rendered; long sessions are not virtualised.
- ACP's own types are not generated for TypeScript.
- The SwiftUI demo app was built but not looked at running.

**Remote**

- A real SSH hop, `wss://` and Tailscale were not run.
- No token expiry, no rate limit on failed handshakes, no QR code or discovery.

## What was spent

Paid model calls: about 17 short Haiku sessions for the recordings, plus roughly ten short Haiku turns across the live runs. One early Pi probe ran on a paid model by mistake (reported at the time). All other Pi runs used the local model. The sessions are in your `~/.claude/projects` and Pi's session folder.

Scratch daemon homes are in `target/m2-home` to `target/m7-home` (ignored by git).

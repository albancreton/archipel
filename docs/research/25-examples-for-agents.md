# Examples and AGENTS.md: documentation for the reader the library will have

_2026-11-23._

## The question

While going through "running as a service" (section B of `21-second-plan.md`), the user said: "realistically speaking, the main audience for this are other coding agents directed to use this library. What would be the best deliverable for this audience? A full API doc or actual use example?"

## The answer given

Examples, with a short routing file in front of them. Reasons:

- An agent reads the types itself. Rust docs and the generated TypeScript types are the API reference and cannot drift from the code. A hand-written copy can, and an agent cannot tell that it is stale.
- What an agent cannot derive is the order of things: start the daemon, connect, make a session, follow it, answer an approval. An example shows that.
- An example that a test runs is true by construction.

The user: "ok, let's do the AGENTS.md and the examples".

## What was made

| Piece | What it is |
|---|---|
| `AGENTS.md` | The entry point. A table from task to example with how far each is checked, the three ways to run the daemon, the free test path with the fake agent, the commands, "what the types do not tell you", and the rules for changing the library |
| `examples/node/chat.ts` | One turn from Node: connect, new session, follow the items, answer the approval |
| `examples/node/daemon.ts` | Start the daemon from the app if none answers on the socket |
| `examples/node/extension.ts` | An extension that adds a note item and serves one method |
| `examples/embedded-daemon` | A Rust program hosting the server in-process, with its own in-process connection and a socket for other apps |
| `examples/service/` | A launchd agent and a systemd user unit, with the commands to install, inspect and remove them |

React and SwiftUI have no new example: the two demos (`ui/react/demo/live.tsx`, `ui/swift/Demo`) are 64 lines each and already are the thing to copy. `AGENTS.md` points to them.

## What this settles of section B, and what it does not

| Plan item | State |
|---|---|
| B3 "start if not running" in the SDKs | As an example in TypeScript, not as an SDK function. Not in Swift |
| B4 embedded-daemon example | Done |
| B5 a page on the three ways to run it | Done as a table in `AGENTS.md` plus the three examples |
| B2 `archipeld service install` | Not built. The service files and the commands to load them stand in for it |
| B1 `archipeld setup` | Not built. `config.json` is still written by hand |

Decisions taken in the examples, on the questions that were open:

- **A daemon the app started stops with the app**, unless the app asks to keep it (`keep: true`). An app that leaves a background process behind should say so to its user.
- **Never two daemons on one home folder.** The starter connects to whatever answers on the socket first, so a service and an app do not collide.
- **After an upgrade the service is restarted by hand.** The README says how, and that running turns are interrupted while sessions stay.
- **The service files carry a `PATH`.** A service manager does not have the shell's, and without it every agent fails to start. This is the trap most worth writing down.

## Checked, and not

- The three Node examples run in a test against the real daemon program with the fake agent. The Rust example has its own test, in-process and through its socket.
- All of it: 96 Rust tests, 46 TypeScript, 33 React, 4 for the examples, the Swift suites; lint and type checks clean.
- **The launchd file, by hand**, after the user agreed to have a service loaded on their machine: it starts the daemon, a Claude session was made and deleted through it, launchd restarts it after `kill -9`, `kickstart -k` restarts it, `bootout` removes it. With a bare `PATH` the agent fails to start with "node: No such file or directory", which confirms the trap. Everything was removed afterwards.
- **Not checked:** the systemd file was never loaded. The React and SwiftUI demos are compiled or type-checked, not run by a test. Nothing here was run against a real harness, since none of it depends on one.

## Found on the way

`examples/*` was a glob in the Cargo workspace, so a folder there without a `Cargo.toml` broke the build. The members are now listed by name.

## Later the same day: do we need `archipeld setup` and `service install`?

Asked by the user. Answer given: no. Both are conveniences for a person installing by hand, and the reader is an agent with the example in front of it. A product will also want its own service label, binary and installer, which a built-in command would decide for it. What is left of "setup" that an agent cannot do from `--help` is diagnosis, and the daemon already has the data. So two things were done instead.

**`archipel agents` now checks.** It existed, but it only printed what the daemon knew, and the daemon knows whether an agent can start only once it has tried: an agent that was never used read `ready`. The command now asks each agent for its sessions first, which starts it and costs nothing, then prints the state and the reason, and exits with an error unless every agent is ready. A test has a working agent and one whose program does not exist.

**The fresh-install path was run.** This machine had no adapter on `PATH`; every test so far used a copy inside `spikes/`. What a new user does was done in a throwaway folder (`npm install -g --prefix …`, so the real global folder was not touched), with that folder's `bin` first on `PATH` and a daemon home with no `config.json`:

| Step | Result |
|---|---|
| `npm install -g @agentclientprotocol/claude-agent-acp` | 108 packages, 420 MB, version 0.110.1 (needs Node 22 or later) |
| `archipeld --home <empty folder>` | Started with one agent, `claude` |
| `archipel agents` | `claude  Claude Code  ready  0.110.1` |
| `archipel new`, `archipel set … model haiku`, one prompt | The turn ran and answered; the session was deleted afterwards |

So the claim in `AGENTS.md` holds: one npm command, no config file, and a real session works.

**The install gave a newer adapter than the baselines were recorded with** (0.110.1 against 0.109.1). The whole Claude conformance suite was run against it, with the same Claude Code as before: all 13 scenarios `same`, the two "unaided" ones included, so neither shipped fix can be removed yet. The baselines were not rewritten.

Worth knowing: a session made with no configuration runs on the account's default model. The check above switched to Haiku before prompting. `AGENTS.md` now says so.

Cost of this section: one Haiku turn, plus the suite's dozen.

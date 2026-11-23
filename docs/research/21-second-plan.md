# The second plan: components, running the daemon, parity, watching upstream, tests

_2026-11-23. Written after the user tried the SwiftUI demo and asked what a custom Claude Code client would still need. This note turns that conversation into lists and proposals. Nothing in it is built yet unless it says so._

## The scope rule

From the user, 2026-11-23:

> I'm not planning on shipping anything at this moment, and if something is ever shipped, it will be the library only, no products. Any product we work on that is not just a quick demo for the library will live somewhere else.

What follows from it:

- Components, SDK calls, daemon helpers, update monitoring and tests belong here.
- App choices do not: when to create a session, whether to cache a model list, what a "new chat" screen looks like. The library delivers the data and the calls; the app decides.
- The demos stay small. They exist to show the library, not to become a client.

## A. Components and calls to add

Both kits (React and SwiftUI) unless noted. Each line says what the library lacks today.

| # | Piece | What is missing today | Needs below the UI |
|---|---|---|---|
| A1 | Markdown text | Assistant text and reasoning are shown as written. No formatting, no code highlighting. Must cope with text that is still streaming (an open code fence) | Nothing |
| A2 | Settings bar | Model, effort, permission mode and on/off settings are in the session data; nothing draws or changes them | `session/options` passed through the daemon (settings without a session) |
| A3 | Composer: attachments | Text only. No images, no files | Swift SDK: `prompt` with content blocks (TypeScript has it) |
| A4 | Composer: commands and mentions | The command list is in the data; no menu. No `@file` mention | A file-listing source for mentions is the app's; the kit takes it as a function |
| A5 | Long timelines | Every item is drawn at once | Nothing |
| A6 | Session list | Only the demos have one, hand-made. No grouping by folder, no state, no "open elsewhere" mark as a component | Rename (`session/set_title`) and delete (`session/delete`) passed through the daemon; paging |
| A7 | Diff view | Plain coloured lines. No highlighting, no collapse of long unchanged parts | Nothing |
| A8 | Terminal view | Raw text; colour codes are not interpreted | Nothing |
| A9 | Agent status | No "not signed in", "agent not installed", "starting" state on screen | The daemon reports `ready`, `needs_login`, `error`; make it finer |
| A10 | Connection status | The SDKs reconnect silently; no component says "reconnecting" | Nothing (the SDKs expose it) |
| A11 | Usage | Percent of context only. No cost, no rate-limit information | Depends on what the adapter passes (section C) |
| A12 | Todo list and background tasks | No items | Depends on section C |
| A13 | "Needs attention" signal | An app that wants a notification must watch the turn state itself | A small SDK helper; showing a notification is the app's |

Decisions inside these, to take when each is built:

- **A1 needs a parser.** Rule of the project: maintained third parties first. Proposal: each kit renders text through a replaceable function with a default. SwiftUI's default uses Apple's own Markdown support (no dependency). React's default uses one small maintained parser as a dependency; an app can swap it.
- **A2 is generic.** It draws whatever settings the agent reports, by kind (choice or on/off) and category. It does not know model names.
- **A6 delete** removes the session from the harness's own history, which the user cannot undo. Proposal: require the admin scope.

Order proposed: A1, A2, A5, A6, then A3 and A4, then the rest.

## B. Running the daemon as a service

The user's question: does "daemonizing" belong in the library, or to whoever uses it? Maybe helpers, and an example in Rust.

Proposal: the library ships helpers and one example; the consumer decides the lifecycle.

What already holds:

- `bridged` runs in the foreground and logs to stderr. That is the form launchd and systemd want, so there is nothing to "daemonize" in the old sense.
- The server is already a Rust library (`bridge-server`, `Server::builder()`). A Rust app can host it in its own process with no separate program.

What to add:

| # | Helper | What it does |
|---|---|---|
| B1 | `bridged setup` | Finds the installed harnesses and their adapters (Claude Code, the Claude adapter and the Node it needs, Pi, `pi-acp`), says what is missing, writes `config.json`. Today the config is written by hand |
| B2 | `bridged service install / uninstall / status` | Writes and loads a per-user service: a launchd agent on macOS, a systemd user unit on Linux. Windows with the Windows work |
| B3 | "Start if not running" in the SDKs | For an app that prefers to own the daemon: start `bridged` as a child when its socket does not answer, and connect. Swift and TypeScript (Node) |
| B4 | `examples/embedded-daemon` | A short Rust program that hosts the server in-process with its own agent list and store. The example the user asked for |
| B5 | A page in `docs/` | The three ways to run it (service, child of the app, in-process) and when each fits |

One fact every consumer meets: Claude is driven through an adapter that is an npm package and needs Node. B1 is where that is detected and explained.

## C. What Claude Code shows in its terminal that the adapter does not pass on

The full inventory is in `22-claude-terminal-vs-adapter.md`. It was made by reading the adapter's code and the docs; almost none of it has been run. In short:

- **Comes through fully**: text, tool calls, approvals, questions, todos, compaction, custom commands and skills, resume, fork, delete, rename, model and effort and mode, MCP questions and sign-in.
- **Not available at all**: rewind and checkpoints (the largest gap), the status line, notifications, IDE integration, voice, the `!` shell shortcut.
- **Partly**: reasoning, live command output, background tasks, hooks (they run, their output is dropped), usage and context detail, the built-in slash commands (about 30 of 110 are listed), sign-in, settings and plugin management.
- **Three findings that change earlier notes**: an app can pass Claude Agent SDK options per session through `_meta` (sandbox, plugins, custom agents, reasoning); command output does stream, but only after about three seconds; and the adapter runs a compaction of its own on an idle session, which shows up as a turn nobody started.
- **Ten open questions**, each with the test that settles it. They join the "never tested" list in section E.

## D. Watching upstream

The user's request: recurring tasks that watch ACP v2, Claude Code, the Claude adapter and Pi; a script fetches the latest version of each, makes a diff against the version we last used, and hands any diff to an agent that writes a report to start the update work. It could ship as part of the library.

Proposal: `tools/watch/`, in TypeScript (the user reads it more easily than Rust).

**What it watches.** One file, `tools/watch/sources.json`, lists each source with the version we are on and which of its files matter to us:

| Source | Why we depend on it | What to compare |
|---|---|---|
| ACP SDK (`@agentclientprotocol/sdk`) | The pinned v2 schema is copied from it | The schema file; the changelog |
| Claude adapter (`@agentclientprotocol/claude-agent-acp`) | Everything Claude says passes through it | Its code; its changelog; what it advertises at `initialize` |
| Claude Agent SDK | The adapter is built on it; new message kinds appear here first | Its type definitions |
| Claude Code | The harness itself, and the session registry the "open elsewhere" check reads (undocumented) | Its changelog; the shape of a registry entry |
| Pi and `pi-acp` | The Pi profile translates its messages | Code and changelog |

Where each one publishes its releases and changelog is to be confirmed when the script is written.

**What one run does.**

1. For each source, ask the registry for the newest version. Same as ours: done.
2. Download both versions, keep the files that matter, write the diff to `tools/watch/out/<date>/<source>.diff`.
3. Free check, no model: run our replay tests against the new schema, and ask the new adapter for its `initialize` answer and `session/options`, compared with what we recorded.
4. If anything differs, start an agent with the diffs and a fixed brief. It writes `docs/updates/<date>-<source>.md`: what changed, what touches us (schema types, state rules, behaviours we rely on, recordings to redo), a proposed list of steps, and what it could not judge.
5. Optional, costs a little: replay a handful of live scenarios on the new versions with the cheapest model and compare their structure with the recordings.

**What it does not do.** It never updates anything. The report is the input to a deliberate update, which stays a decision.

**Scheduling.** The script is the component. What runs it is the consumer's choice: cron, a launchd timer, a CI schedule, or a scheduled agent. The agent in step 4 is a command in the config (`claude -p` by default), so another agent can be used.

Open points for the user:

- Where it runs for this project: this Mac on a timer, or CI once the repository has a remote.
- How often: daily is cheap (step 4 only runs when something changed).

## E. Behaviours: what is tested, and what is not

Three levels of proof exist today. The lists are by behaviour, not by test.

**Proven by automated tests, on recorded sessions (free, run every time)**

- Every recorded session (29) gives the same state in Rust, TypeScript and Swift.
- Everything passes through the daemon unchanged, and a replay gives the same state as the live run.
- An app that lost its connection catches up from where it was.
- An unanswered approval waits; the first answer wins; the others are told.
- A restarted daemon serves sessions from its log and wakes them.
- An adapter that dies: its sessions are closed cleanly, and the next request restarts it.
- A read-only connection reads everything and changes nothing.
- Sessions only the harness knows are listed, and adopted when opened.
- Every app is told when a session appears or changes.
- Old sessions are removed after the retention time.
- An extension adds items and serves methods.
- A session open in another process is refused, can be read, can be forked; the attach answer says who has it.
- Pairing with a one-time code; a code expires; a wrong token or origin is refused.
- Pi: the translation of 12 recorded sessions; an idle session loses its process and gets one back.
- The SDKs: reconnecting; the open-elsewhere state and the fork (against a stand-in daemon).
- The kits: every recorded session is drawn; approvals, questions, subagents, unknown items, the open-elsewhere notice.

**Seen working live, once, by hand (not repeatable without doing it again)**

- Claude: a turn with an approval; the daemon killed during an approval and restarted; catch-up; wake-up; adopting sessions from the history; 116 sessions listed.
- Pi on a local model next to Claude in one daemon: a turn with tools, idle stop and wake-up, adoption.
- A browser page answering an approval; a page from a wrong origin refused.
- The block on a real session held by a real Claude process; a fork; the fork button in a browser.
- The SwiftUI demo, by the user.

**Never tested**

_Update, same day: the settings, session, turn and shell rows below were run by the first conformance tests. Results in `23-conformance-first-run.md`._

| Area | Behaviour |
|---|---|
| Settings | Switching the model on a live session, for Claude and for Pi |
| Settings | Whether a reopened session keeps its model, effort and mode, or falls back to the default |
| Settings | Whether the effort choices change when the model changes |
| Settings | `session/options` (settings without a session), for a new session and for an existing one |
| Sessions | `session/delete` and `session/set_title` through the daemon (not passed through yet) |
| Sessions | Whether creating a session and never using it leaves an entry in the harness's own history |
| Sessions | A process that opens a session between the check and the prompt |
| Sessions | The time and cost of the first message after reopening a long session |
| Input | Images and files in a prompt; MCP servers and extra folders given at session creation |
| Turn | Steering and interrupting through the daemon with a real harness (only on recordings) |
| Turn | Two apps answering the same approval at the same moment, live |
| Daemon | A client too slow to keep up is disconnected and catches up, live; a full disk; a damaged log |
| Daemon | Many sessions at once; a session of several thousand items end to end |
| Claude | Todo lists; adapter notices; background tasks as items |
| Pi | Who else has a session open; fork; compaction; background work; delete |
| Remote | A real SSH hop; `wss://`; Tailscale |
| Platforms | iOS; React Native; Windows; Linux |
| UI | Clicking through the SwiftUI views (only drawn off screen); long sessions on screen |

**The test plan**

Four tiers, cheapest first. The aim is that every line in "seen once by hand" and "never tested" moves into a tier that can be run again.

| Tier | What | Cost | When it runs |
|---|---|---|---|
| T1 Replay | What exists: recorded sessions through every layer | Free | Every change |
| T2 Faults | The daemon under failure: kill the adapter or the daemon at each step of a turn, a slow client, a store that fails, a damaged log, two answers at once. Uses the fake agent | Free | Every change |
| T3 Live conformance | A script per harness that runs fixed scenarios against the real thing on the cheapest model and checks structure, not wording: which items appear, in which order, which states, what a reopen restores. One scenario per behaviour in the tables above. It also re-records the transcripts T1 uses | A few cents per run for Claude; free for Pi on a local model | On demand, and by the watch script when a version changes |
| T4 UI | Click tests: the React kit in a headless browser against the fake agent; the SwiftUI kit through macOS UI tests | Free | Every change (React); on demand (SwiftUI) |

T3 is what the user called evals: the wording of a model's answer varies, so each scenario states what must be true of the session whatever the model wrote. A scenario that fails on a new version is exactly the signal section D wants.

## Proposed order

1. **T3 first, with the "never tested" settings and session rows.** They are cheap to settle and several plans depend on the answers (does a reopened session keep its model?).
2. **Pass-through of `session/options`, `session/delete`, `session/set_title`**, then A2 and A6 on top.
3. **A1 and A5**, the two most visible gaps in the kits.
4. **The watch script (D)**, reusing T3 as its live check.
5. **B1 to B5.**
6. **T2 and T4**, then the remaining components.

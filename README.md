# archipel (temporary name)

A library for building apps that render the agent loop of terminal coding harnesses: messages, reasoning, tool calls, approvals, questions, plans, todos, subagents. The model is "what libghostty is to the Ghostty terminal": the hard part done once, with thin clients in Swift, React Native, Electron or Windows on top.

Claude Code and Pi are the main targets. Other harnesses are accessory.

Status: all seven milestones of the first plan are done; `docs/research/19` lists what is still open.

- A daemon (`archipeld`) runs Claude Code and Pi side by side, numbers and logs everything they say, and keeps sessions alive while apps come and go.
- Apps attach, catch up after a disconnect, and answer approvals; the first answer wins. A connection can be limited to reading.
- The daemon lists the sessions each harness already has and loads one when it is opened.
- Other programs add their own items and methods as extensions.
- Client SDKs for TypeScript and Swift, and component kits for React and SwiftUI.
- Remote access through SSH, or a WebSocket with paired tokens; a browser page from a named origin.

Tested against 29 recorded sessions in three languages, and live with both harnesses.

Building something with it, or directing a coding agent to? Start at [`AGENTS.md`](AGENTS.md): it says which example to copy for which task, how to check an integration at no cost, and what the types do not tell you.

## Try it

No cost: a fake agent replays a recorded session.

```
cargo build
target/debug/archipeld --home /tmp/archipel-demo --fake testdata/transcripts/claude-v2/approval_command.ndjson &
alias b='target/debug/archipel --home /tmp/archipel-demo'
b new --mode default            # prints the session id; its first characters are enough afterwards
b prompt <id> hello --watch     # stops when the agent asks for approval
b answer <id> allow-once
b watch <id> --until-idle
b show <id>
```

With the real Claude adapter: put a `config.json` in the daemon's home (see `archipeld --help`) and run `archipeld` without `--fake`. `archipel --help` lists the client's commands.

## Decisions so far

- **Greenfield.** T3 Code is a source of specs, recorded transcripts and edge cases. Its code is not adapted or vendored.
- **Scope is the chat loop only.** Not T3's surroundings (projects, git checkpoints, worktrees, scheduling, source control).
- **A daemon, not an in-process library.** Harnesses are processes that live next to the code; apps may be remote. The daemon sits next to the harness and apps talk to it.
- **ACP v2 is the vocabulary.** Apps speak ACP v2 plus a few extensions; v2 shapes are the daemon's canonical form. Claude's adapter is driven over v2; Pi's v1 adapter is translated up. The daemon adds what ACP lacks: several apps per session, catching up after a disconnect, a remote transport (tested live, see docs/research/09 to 11).
- **The server is written in Rust.** One static binary per host, embeddable later; the data model is a schema that server and client types are generated from. UIs are separate packages (Swift and React component kits) and optional.
- **Extensible from the start.** Other products add namespaced content and extension processes without forking (design, section 11).
- **Maintained third parties first.** Official adapters and SDKs on the harness side; platform TLS/WebSocket, SSH or Tailscale for transport. No home-made crypto.

## Where things are

| Path | What |
|---|---|
| `docs/design/daemon.md` | The daemon design (current work) |
| `docs/design/addons.md` | Addons: where fixes for upstream faults and optional features live, and how to write one |
| `schema/`, `crates/` | The data model and the Rust workspace (daemon, command-line client). Run `just` for the commands |
| `clients/ts`, `clients/swift` | The client SDKs |
| `ui/react`, `ui/swift` | The component kits |
| `docs/research/01` to `03` | T3 Code provider report, activity analysis, Claude changes over 30 days |
| `docs/research/04` | First recommendation on the library shape (partly superseded) |
| `docs/research/05`, `06` | T3's Pi and Claude adapters run outside the app (historical) |
| `docs/research/07` | The content types to render; protocol and prior-art summary |
| `docs/research/08` | Raw web research on protocols, prior art, transport (with an unverified list) |
| `docs/research/09`, `10` | Claude and Pi driven live through their ACP adapters |
| `docs/research/11` | Milestone 0: the unknowns closed, ACP v2 tried, the Rust crate checked |
| `docs/research/12` | Milestone 1: schema and core, and how the v2 schema drives the reducer |
| `docs/research/13` | Milestone 2: the first running daemon, what was tested, what is missing |
| `docs/research/14` | Milestone 3: scopes, listing and adopting the harness's own sessions |
| `docs/research/15` | Milestone 4: the Pi translator, one process per session |
| `docs/research/16` | Milestone 5: extensions, and items that carry their own state rule |
| `docs/research/17` | Milestone 6: the TypeScript and Swift SDKs, the React and SwiftUI kits, and how they are kept identical |
| `docs/research/18` | Milestone 7: remote access |
| `docs/research/19` | Where things stand: what exists, what was checked, everything still open |
| `docs/research/20` | One session open in two processes: what Claude Code does, the block, the fork, the fork button |
| `docs/research/21` | The second plan: components to add, running the daemon, watching upstream, what is tested and what is not |
| `docs/research/22` | What Claude Code offers in its terminal and what comes through the adapter |
| `docs/research/23` | The live conformance tests: the first run and what it found |
| `docs/research/24` | Addons: the mechanism, the first two, and which fixes went into the core instead |
| `docs/research/25` | Who reads the docs: examples and `AGENTS.md` for coding agents, and what that settles of "running as a service" |
| `AGENTS.md` | The entry point for whoever builds on the library: which example for which task, the free test path, the traps |
| `examples/node` | A chat, starting the daemon from an app, and an extension, in TypeScript |
| `examples/embedded-daemon` | A Rust program that hosts the daemon in its own process |
| `examples/service` | The daemon as a launchd or systemd service |
| `examples/own-daemon` | A daemon with an addon of one's own, without forking |
| `conformance/` | Live checks of the real harnesses against a recorded baseline |
| `tools/watch/` | Looks for new upstream versions and writes down what changed; its reports go to `docs/updates/` |
| `docs/remote.md` | How to reach a daemon from another machine or a browser |
| `testdata/` | Recorded sessions and readable renderings of what the core makes of them |
| `spikes/` | The scripts and raw logs behind 05, 06, 09, 10. See `spikes/README.md` |
| `data/t3-claude-commits-30d.txt` | The 230 Claude-related T3 commits analysed in 03 |

## What was tested, in one table

Live runs on 2026-11-22. Claude: `claude` 2.1.330, Haiku, adapter `@agentclientprotocol/claude-agent-acp` 0.109.1. Pi: 1.6.0, `local/Qwen3.8-27B-BF16-DFLASH2`, adapter `pi-acp` 0.0.35.

| Content | Claude over ACP | Pi over ACP |
|---|---|---|
| Assistant text, streamed | yes | yes |
| Reasoning, streamed | not seen (adapter has code for it) | yes |
| Shell command | yes | yes |
| Command output while running | no, only at the end | yes, under a `terminal_output` meta key |
| Read, search, edit with diff | yes | yes |
| Approval request | yes | none by default in Pi |
| Question to the user | yes, as a form elicitation | no such tool |
| Plan mode | yes | no |
| Todo list | not reproduced (adapter maps it to plan entries) | no such tool |
| Subagent | yes, as a child session | one opaque tool call |
| Background work | wake-up reply arrives after the turn; no task item | not tested |
| Interrupt | yes | yes |
| Message while busy | yes (steering, advertised under `_meta`) | yes (queued; announced as assistant text) |
| Compaction | yes | not shown (session too small) |
| Usage, model, mode, commands | yes | yes |

## Reference projects

Paseo (daemon + WebSocket API + client SDK), acpx (headless ACP client), Rivet Sandbox Agent (one HTTP API over several harnesses), Happy (relay + native apps), T3 Code (adapters and recorded transcripts). herdr is a terminal workspace manager and does not carry structured content. Details and links in `docs/research/08`.

## Licence

Licensed under either of Apache License, Version 2.0 (`LICENSE-APACHE`) or MIT licence (`LICENSE-MIT`), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work, as defined in the Apache-2.0 licence, shall be dual licensed as above, without any additional terms or conditions.

Files derived from other projects keep their own notices. Scenario prompts in `spikes/acp-probe/scenarios.json` are taken from or modelled on T3 Code's replay fixtures (MIT, Copyright (c) 2026 T3 Tools Inc.).

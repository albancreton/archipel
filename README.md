# harness-bridge (temporary name)

A library for building apps that render the agent loop of terminal coding harnesses: messages, reasoning, tool calls, approvals, questions, plans, todos, subagents. The model is "what libghostty is to the Ghostty terminal": the hard part done once, with thin clients in Swift, React Native, Electron or Windows on top.

Claude Code and Pi are the main targets. Other harnesses are accessory.

Status: research and design. The repository layout is scaffolded; the crates are empty.

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
| `schema/`, `crates/`, `clients/`, `ui/`, `testdata/` | The scaffolded layout (design, section 13). Run `just` for the commands |
| `docs/research/01` to `03` | T3 Code provider report, activity analysis, Claude changes over 30 days |
| `docs/research/04` | First recommendation on the library shape (partly superseded) |
| `docs/research/05`, `06` | T3's Pi and Claude adapters run outside the app (historical) |
| `docs/research/07` | The content types to render; protocol and prior-art summary |
| `docs/research/08` | Raw web research on protocols, prior art, transport (with an unverified list) |
| `docs/research/09`, `10` | Claude and Pi driven live through their ACP adapters |
| `docs/research/11` | Milestone 0: the unknowns closed, ACP v2 tried, the Rust crate checked |
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

# Content types to render, protocols, prior art

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

**The question asked:**

> I want to confirm we are not looking into this as an adaptation project of the t3 code, but as a greenfield project that uses T3 as a source of data for specs.
> 
> For example, my list of features was not exhaustive (tool calls, questions, todo, sub-agent views) 
> We need a full list the actual real list of "features" type of content that needs to be rendered.
> 
> The communication protocol, is ACP a standard of sort or just something T3 created/re-used from a specific harness?
> What other protocol exist we can rely on? do we need a protocol? can we rely on existing multiplexer(s)? 
> How can we keep this secured by using maintained 3rd parties as much as possible?
> 
> Are there other similar project in the wild we can look at and use as reference? Could a project like herdr help?

---

Confirmed: greenfield, with T3 as a source of specs, recorded transcripts and edge cases. The research also changes my earlier advice: ACP now covers enough that you probably should not design your own protocol from scratch.

The protocol and prior-art facts below come from web pages fetched today through a summarising agent, so check field names against the specs before coding.

## What needs rendering

This is the item list from T3's current contracts, minus the items that are T3 app features (checkpoints, handoffs, forks, secret and link requests).

| Group | Content types |
|---|---|
| Conversation | user message (with attachments, and intent: new turn, queued, steer), assistant message (commentary, partial or final answer), reasoning |
| Tools | command execution (with streamed output, exit code, monitor flag), file change (diff, additions and deletions), file search, web search, generic tool (any other tool or MCP tool, including image views) |
| Asking the user | approval request (command, file read, file change, MCP elicitation, permission; with options such as accept for session), question (multi-option, multi-select, custom answer, previews) |
| Planning | proposed plan (markdown, streamed), todo list (steps with pending, running, completed) |
| Delegation | subagent (prompt, progress, result, its own child thread of items), background work (tasks that outlive the turn and wake the agent) |
| Session events | compaction (summary, token counts before and after), error (failure class such as usage limit, with retry), system notice (including "MCP server needs sign-in"), interrupt request and result |
| Session state, outside the item list | turn status, model and mode, context and token usage, available slash commands and skills, prompt suggestions, queued and steering messages |

Every item carries a status from one shared set: idle, pending, running, waiting, completed, failed, cancelled, interrupted.

## Is ACP a standard?

Yes. T3 did not create it; T3 uses it for Grok, Kiro, fx and Antigravity.

- **Origin and governance:** started by Zed, now jointly governed by Zed and JetBrains, Apache-2.0, with a stated plan to move to a foundation.
- **Adoption:** about 46 agents in its registry and clients in Zed, JetBrains, Emacs, Neovim and VS Code.
- **Your two harnesses:** Claude has an adapter under the ACP organisation, built on the Agent SDK. Pi has a community adapter. Codex has one too.
- **Coverage:** message and thought chunks, tool calls with diffs and terminals, plans, permission requests, questions (elicitation), usage, slash commands, modes, and since this year compaction and subagents.
- **Gaps:**
  - No remote transport has shipped; stdio is the only defined one, and HTTP/WebSocket is a draft.
  - It is one client to one agent, with no replay and no way for several clients to attach.
  - Queued and steering messages and background work exist only in v2, which went to preview on 2026-11-20.
  - Fork is a draft.

## Do you need your own protocol?

Not for the vocabulary. You need a daemon that fills ACP's gaps.

- **Other protocols don't fit:**
  - MCP is a different layer (agent to tools).
  - A2A is too coarse.
  - AG-UI and the Vercel stream format have no coding concepts.
  - Codex app-server, OpenCode's API and Pi RPC are rich but tied to one harness each.
- **Terminal multiplexers don't fit either:** they own the terminal and give you a status and a text blob, not tool calls or approvals. The main screen-scraping project, coder/agentapi, was archived in September.
- **What I'd build:** a daemon that speaks ACP to the harness adapters and re-exposes ACP-shaped messages to apps over WebSocket. It adds what ACP lacks: session persistence, replay on reconnect, and multiple clients attaching.

## Security through maintained third parties

- **Harness side:** use the official adapters and SDKs, so auth stays with each vendor's own CLI login.
- **Transport:** platform WebSocket with TLS, or SSH stdio, or the system Tailscale client. None of the comparable projects use anything more exotic.
- **Pairing:** a one-time token exchanged for a session token, as T3 and OpenCode do. Avoid writing your own encrypted relay.

## Reference projects

- **Paseo:** closest to your goal — a daemon with a WebSocket API and a client SDK, with Electron and Expo apps. Apache-2.0. No standalone protocol spec was found.
- **acpx:** a headless ACP client with 30 agent profiles and an embeddable runtime. MIT.
- **Rivet Sandbox Agent:** one HTTP API over Claude Code, Codex, OpenCode and Pi, with ACP endpoints. Apache-2.0.
- **Happy:** a CLI wrapper, encrypted relay and native apps. MIT.
- **herdr:** not for the chat part. It is a Rust terminal workspace manager that keeps terminals alive and marks panes working, blocked or idle, without parsing agent output. It is useful as a reference for session persistence and SSH stdio transport only.

## Next step

Run the Claude and Pi ACP adapters against the scenarios T3 recorded and score them against the table above. Subagents, background work and questions are the likely weak spots, and how well the adapters surface them is unverified. That result decides between "ACP plus a daemon" and "own adapters".

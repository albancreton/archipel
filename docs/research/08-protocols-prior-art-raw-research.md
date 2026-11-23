# Protocols, prior art, transport: raw web research

_Report from a research subagent, 2026-11-22. Pages were fetched through a summarising model: check field names against the linked specs before coding. See its own "Could not verify" list._

Research report (2026-11-22). Everything below comes from pages fetched today unless marked "(unverified)". Fetches went through a summarising model, so check exact field names against the linked specs before coding against them.

## 1. Agent Client Protocol (ACP)

- **Governance and licence:** jointly governed by Zed and JetBrains as an "interim model", "working toward transitioning to an independent foundation". Lead maintainers are Ben Brandt (Zed) and Sergey Ignatov (JetBrains). Apache-2.0, 4.7k stars. https://agentclientprotocol.com/community/governance , https://github.com/agentclientprotocol/agent-client-protocol
- **Version:** v1 is stable. The v2 proposal and its fifteen RFDs moved to Preview on 2026-11-20. https://agentclientprotocol.com/rfds/updates.md , https://agentclientprotocol.com/protocol/v2/migration.md
- **Transport:** stdio newline-delimited JSON-RPC is the only defined transport. The Streamable HTTP and WebSocket RFD is still Draft (since 2026-04-22): a single `/acp` endpoint, `Acp-Connection-Id` and `Acp-Session-Id` headers, no replay of missed messages in v1, and a reference implementation in Goose in progress. Custom transports are allowed. https://agentclientprotocol.com/protocol/transports , https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md
- **Agents via adapter:**
  - Claude: `claude-agent-acp`, now under the agentclientprotocol org, built on the Claude Agent SDK. It serves v2 clients "directly on Claude Code". https://github.com/zed-industries/claude-agent-acp
  - Codex: https://github.com/agentclientprotocol/codex-acp
  - Pi: community adapter https://github.com/svkozak/pi-acp
- **Agents listed with no adapter:** Gemini CLI, OpenCode, Cursor, Kiro CLI, Goose (`goose acp`), GitHub Copilot, Cline, Junie, Qwen Code, Factory Droid, OpenHands, Mistral Vibe and about 30 more. https://agentclientprotocol.com/get-started/agents
- **Clients:** Zed, JetBrains, Emacs (agent-shell), Neovim (CodeCompanion, avante, others), VS Code extensions, Obsidian, Sublime Text, Qt Creator, Visual Studio, Chrome extensions. https://agentclientprotocol.com/get-started/clients
- **SDKs:** official TypeScript, Rust, Python, Kotlin (JVM) and Java. Swift (three), .NET (three), Go (seven) and Dart are community only. https://agentclientprotocol.com/libraries/community
- **Registry:** yes, completed 2026-03-09, with 46 agents and one `agent.json` per agent. https://github.com/agentclientprotocol/registry
- **What v1 can express** (https://agentclientprotocol.com/protocol/schema):
  - Session updates: `user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`, `plan_update`, `plan_removed`, `available_commands_update`, `current_mode_update`, `config_option_update`, `session_info_update`, `usage_update` (context used, size, optional cost), `notice`, `compaction_update`, `compaction_summary_chunk`, `subagent_update`, `session_message`, `session_message_chunk`.
  - Tool calls: kinds are read, edit, delete, move, search, execute, think, fetch, switch_mode, other. Content is content, diff, terminal or session. Status is pending, in_progress, completed or failed.
  - Client-side requests: `session/request_permission`, `elicitation/create` (JSON Schema form or URL), `fs/*`, `terminal/*`.
  - Session methods: new, load, resume, list, close, delete, cancel, set_mode, set_config_option.
- **Recently closed gaps:** subagents (completed 2026-11-20, capability opt-in in v1), compaction (2026-10-06), elicitation (2026-07-24), usage (2026-06-05), and session list, resume, close and delete (March to June 2026). Expect uneven agent and SDK support for these.
- **Still weak:**
  - Session fork is Draft only, even in v2.
  - v1 has no queued or steering messages, because `session/prompt` blocks until the turn ends. v2 Preview lets clients prompt during active work and signals completion via `state_update: idle`.
  - Background work exists only in v2 Preview (a `background` list on `state_update`).
  - There is no separate todo concept. Plans are a checklist of entries with priority and status; v2 adds `items` and `markdown` variants.
  - End-turn token usage, session status, archive and set-title are still open RFDs.
  - No remote transport has shipped, and there is no replay or resumption.
  - There is no multi-client attach or daemon concept. ACP is one client to one agent process.

## 2. Other protocols

- **MCP:** connects an agent to tools and data, which is a different layer. It is not a format for rendering an agent loop. ACP reuses its content blocks.
- **A2A:** v1.0.0, with JSON-RPC, gRPC and REST bindings and a Task/Message/Artifact/AgentCard model. Linux Foundation; the spec page says it joined the Agentic AI Foundation. It is opaque agent-to-agent delegation and too coarse for tool calls, diffs and approvals. https://a2a-protocol.org/latest/specification/
- **IBM/BeeAI Agent Communication Protocol:** merged into A2A in August 2025 and the repo is archived. Dead. https://research.ibm.com/projects/agent-communication-protocol
- **AG-UI:** CopilotKit-originated event protocol between agent backends and web UIs, over HTTP/SSE or WebSocket, with community SDKs in Kotlin, Go, Dart, Java, Rust and .NET. Plausible for generic chat with tools, but it has no coding concepts such as diffs, terminals, permission modes or sessions. https://docs.ag-ui.com/introduction
- **Vercel AI SDK UI message stream:** SSE with the header `x-vercel-ai-ui-message-stream: v1`. Parts cover text, reasoning, tool input and output, `tool-approval-request`, `data-*`, and an experimental `steering-message`. It is single-response scoped, with no session or daemon model. https://ai-sdk.dev/docs/ai-sdk-ui/stream-protocol
- **Codex app-server:** OpenAI's JSON-RPC over stdio (WebSocket is also mentioned), built on thread, turn and item primitives, with `thread/fork` and approval requests. Many features sit behind `experimentalApi`. It is the richest single-harness protocol but Codex-specific. https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md
- **Claude Agent SDK:** Python and TypeScript libraries that run the Claude Code binary. Other languages use `claude -p` with JSON output. The docs say third parties may not offer claude.ai login without approval. https://code.claude.com/docs/en/agent-sdk/overview
- **OpenCode server:** HTTP with OpenAPI 3.1 (178 operations), SSE at `GET /api/event` (volatile), an experimental durable per-session log, and pairing-code auth. Client-facing and rich, but OpenCode-specific. https://opencode.ai/docs/server/
- **Pi RPC:** `pi --mode rpc`, LF-delimited JSON on stdio. Commands include prompt, steer, follow_up, abort and bash. Events include `message_update`, `agent_end`, `agent_settled` and compaction, plus `extension_ui_request` and response. https://docs.pi.dev/latest/rpc
- **Agent Protocol (agentprotocol.ai):** a REST "community spec" of runs, tasks, steps and artifacts. Not suitable for a streaming coding UI, and it looks dormant.
- **LSP:** an analogy only. ACP copies its model of JSON-RPC over stdio with capability negotiation.

## 3. Prior art

**Closest to a reusable daemon with a normalised schema:**

- **ACP adapters plus acpx:** acpx is a headless ACP client with 30 agent profiles, persistent sessions and NDJSON output, and it exports an embeddable `acpx/runtime`. MIT, TypeScript, 3.5k stars. https://github.com/openclaw/acpx
- **Rivet Sandbox Agent:** one HTTP API for Claude Code, Codex, OpenCode, Cursor, Amp and Pi, with `/v1/acp` endpoints and a TypeScript SDK plus React components. Apache-2.0, 1.7k stars; the repo now shows as rivet-dev/agents. https://rivet.dev/sandbox-agent/docs
- **Paseo:** a daemon with a WebSocket API, a `@getpaseo/client` TypeScript SDK, an optional end-to-end encrypted relay written in Elixir, and Electron, Expo and CLI clients. It covers Claude Code, Codex, Copilot, OpenCode, Pi and more. Apache-2.0, 24.7k stars. No standalone protocol spec was found. https://github.com/getpaseo/paseo
- **T3 Code:** one WebSocket server with per-provider adapters and typed contracts, serving web, desktop and mobile. MIT, 71.5k stars. Its protocol is app-internal TypeScript and Effect Schema, not language-neutral. https://github.com/pingdotgg/t3code
- **Happy:** CLI wrapper, encrypted relay server and native apps for Claude, Codex, Grok, Gemini and any ACP agent. MIT, 24.8k stars. https://github.com/slopus/happy
- **herdr** (answering the user's question): a Rust terminal workspace manager that "owns their terminals" and does not parse agent output. A background server keeps PTYs alive, each pane is marked working, blocked or idle, and it has a CLI and socket API. Apache-2.0 per the README; one search result reported an AGPL badge. A secondary source says a desktop app on a new runtime, herdr-core (`hc`), was announced on 2026-10-29, with an OpenRPC schema via `hc schema`, `hc connect` over stdio and SSH, and status from the program-reported "OSC 7501" escape. It is useful prior art for PTY persistence, attention state and SSH stdio transport, but it carries no structured messages, tool calls or approvals. https://github.com/ogulcancelik/herdr , https://flaviocopes.com/herdr/

**Terminal or PTY based (no structured events):**

- **coder/agentapi:** HTTP and SSE over a terminal emulator with screen diffing. MIT, Go. Deprecated and archived on 2026-09-13. https://github.com/coder/agentapi
- **Claude Squad:** tmux plus worktrees. AGPL, Go, 8.8k stars. https://github.com/smtg-ai/claude-squad
- **Agent of Empires:** tmux, with an HTTP API and web dashboard. MIT, Rust, 4.7k stars. https://github.com/njbrake/agent-of-empires
- **ccmanager:** PTY with per-agent state detection. MIT, TypeScript. https://github.com/kbwo/ccmanager
- **cmux:** Swift app on libghostty with a socket API. GPL-3.0, 28.9k stars. https://github.com/manaflow-ai/cmux
- **VibeTunnel:** PTY forwarded to the browser. MIT. https://github.com/amantus-ai/vibetunnel

**Apps (not libraries):**

- **Vibe Kanban:** Apache-2.0, Rust, 10 agents, sunsetting. https://github.com/BloopAI/vibe-kanban
- **Superset:** MIT, 16.5k stars. The fetched README calls it "Paradev", which may be a rename. https://github.com/superset-sh/superset
- **Crystal:** MIT, deprecated in February 2026 in favour of Nimbalyst. https://github.com/stravu/crystal
- **Opcode:** AGPL, Tauri, Claude only. https://github.com/winfunc/opcode
- **CodexMonitor:** MIT, Tauri, one `codex app-server` per workspace, with a remote daemon for iOS over Tailscale. https://github.com/Dimillian/CodexMonitor
- **claudecodeui / CloudCLI:** AGPL. https://github.com/siteboon/claudecodeui
- **Sculptor:** MIT, Python, Pi and Claude Code. https://github.com/imbue-ai/sculptor
- **Toad:** AGPL, Python and Textual, ACP client. https://github.com/batrachianai/toad
- **agent-shell:** GPL-3.0, Emacs, ACP, 24 agents. https://github.com/xenodium/agent-shell
- **Goose:** Apache-2.0, Rust, part of AAIF, speaks ACP as an agent. https://github.com/block/goose
- **Conductor:** proprietary Mac app. https://www.conductor.build

**Not what the names suggest:**

- **Omnara** is now a self-hosted managed-agent platform running its own loop, not a CLI wrapper. https://github.com/omnara-ai/omnara
- **Rivet agentOS** is a VM runtime, not an agent API. https://github.com/rivet-dev/agent-os

## 4. Transport, multiplexing and security

**What comparable projects use (verified):**

- **Paseo:** WebSocket to the daemon; optional end-to-end encrypted relay for pairing, otherwise direct TCP or Tailscale.
- **Happy:** HTTP and WebSocket relay. Per-session data keys are wrapped with tweetnacl box, and payloads use AES-256-GCM (legacy XSalsa20-Poly1305). https://github.com/slopus/happy/blob/main/docs/encryption.md
- **OpenCode:** single-use pairing code exchanged for a session token.
- **T3 Code:** one-time pairing URL token, Tailscale sharing via `--share`, and its own tunnel, T3 Connect (from the repo's AGENTS.md).
- **CodexMonitor:** daemon over Tailscale.
- **herdr:** SSH stdio (`hc connect`).
- **VibeTunnel:** Tailscale, ngrok or Cloudflare Quick Tunnel, with SSH-key or password auth.
- **ACP remote RFD:** WebSocket or HTTP/2 with SSE, auth layered through headers.

**Multiplexing:** every structured project above multiplexes with JSON-RPC ids plus a session id over one stream (ACP, Codex app-server, Pi) or WebSocket (Paseo, T3 Code). None of those reviewed use yamux, gRPC, Cap'n Proto or WebTransport.

**Client library availability (unverified; from general knowledge, not checked today):**

- WebSocket with TLS is native on every target (URLSessionWebSocketTask, OkHttp, React Native, .NET ClientWebSocket).
- gRPC and Connect have official Swift, Kotlin, JS and .NET clients, but React Native streaming is weak.
- WebRTC data channels have libraries on all targets but are heavy.
- iroh is Rust-first with FFI bindings.
- WebTransport and raw QUIC are absent or immature on React Native.
- tsnet is Go only, so apps would rely on the system Tailscale client.

**Terminal multiplexing versus structured protocols:** screen-scraping gives only a status and a text blob. Its main example, agentapi, was archived, and its README warns that parsing breaks as TUIs change. herdr reportedly moved from guessing to program-reported status. PTY ownership helps with persistence and "needs attention" signals but cannot produce tool calls, diffs or approvals.

## Could not verify

- herdr's licence (Apache-2.0 versus an AGPL badge) and the herdr-core details, which come from one blog and not the README.
- Whether superset-sh/superset was renamed "Paradev" or the fetch was wrong.
- Happy's QR pairing flow (not in the encryption doc) and whether it has a daemon or wire package.
- Sandbox Agent's universal event schema and SSE; only "stream events" and `/v1/acp` were seen.
- Codex app-server's full item list, WebSocket framing and schema-generation commands.
- The Claude Agent SDK stream-json message shapes (system, assistant, result, `parent_tool_use_id`), which were not fetched.
- Whether OpenCode has an official SDK or `opencode acp` (the ACP site lists OpenCode as an agent).
- Zed remote over SSH, VS Code tunnels and Mosh details, which were not researched.
- Conductor's agent list and funding, which come from secondary sources.
- Agent Protocol's maintainer and last update.
- Whether individual agents (Cursor, Kiro, Gemini) speak ACP natively; this is inferred from the absence of a named adapter.
- Actual agent and SDK support for the newest ACP features (subagents, compaction, plan operations).


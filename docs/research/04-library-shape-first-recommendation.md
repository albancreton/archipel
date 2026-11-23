# First recommendation on the library shape

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

> Partly superseded: at this point the advice was "own UI protocol, ACP as an input adapter". After the ACP tests (09, 10) the direction became "ACP as the vocabulary, plus a daemon". See docs/design/daemon.md.

**The question asked:**

> I'm still in the quest to write tools that do a lot of what t3-code does with these servers, but not what it does around them.
> What I mean is, I'm only interested in the loop rendered, the "chat" part, with the agent loop, the sub-agents... basically, a custom way to render what the terminal based harnesses (claude code, codex, pi, opencode...) renders, including tool calls, questions, todo, sub-agent views...
> Claude and Pi being my main need and the other being accessory.
> 
> I wish the code of this project was clean enough for simple reuse, keeping the upstream flowing when I focus on builing my app. But it seems unlikely. this is not built to be a Library, it's an app...
> 
> So let's have a research spike to create an actual library out of this. a new project, heavily inspired by this one (they have found a lot of things already we can rely on) but that will have it's onw path, parallele to this project.
> 
> I want a real library. As libghostty is to the ghostty terminal. Something that allows to build apps that have server(s) to talk to the local or remote coding harnesses.
> 
> Something agnostic enough to allow one to build a swift app as well as a react native one, electron, or whatever they use on windows.

---

My recommendation: build it as a small host daemon plus a language-neutral wire protocol, with Claude and Pi as native adapters. Take T3's adapter contract and test method as the design, not its code.

## What "library" has to mean here

libghostty works in-process because a terminal runs where the UI runs. A coding harness does not: it runs where the repo, the CLI and the credentials are, and your UI may be an iPhone. So the reusable unit is three things:

- **A host core** that runs next to the harnesses, spawns them and normalises their output.
- **A wire protocol** between core and UI. This is your ABI, and it is what makes Swift, React Native, Electron and Windows equal citizens.
- **Thin client SDKs** generated from the protocol schema.

A C-ABI library embedded in a Swift app would only serve the local desktop case.

## What T3 gives you, and what it doesn't

- **Licence:** MIT, so lifting code is allowed.
- **Reusable as a design:** the session runtime interface in `packages/provider-core/src/server/ProviderAdapter.ts` is almost exactly your scope: open session, start/steer/interrupt turn, answer a request, and an event stream of item updates. The item vocabulary (messages, reasoning, commands, file changes, todo list, proposed plan, questions, approvals, subagents) is your render model.
- **Reusable as code, partly:** `provider-pi` depends only on `contracts`, `provider-core`, `shared` and Effect, and the test host that satisfies it is about 90 lines.
- **Not reusable as code:**
  - The packages are private and unpublished.
  - `contracts` is 39,700 lines of app-wide schemas.
  - Everything is written in Effect.
  - Claude, your main need, is a 15,194-line file inside the server.
- **Not needed:** the event-sourced orchestrator, SQLite projections, runs and attempts, checkpoints, provider switching and handoffs, usage and limits, installers, sign-in flows, text generation, and T3's own MCP server.

## Proposed shape

1. **Adapters** (one per harness) translate a native protocol into normalised events. Each declares only the capabilities a UI branches on; T3 has about 20 that are actually read.
2. **Session engine** owns processes, turn lifecycle, pending approvals and questions, and the subagent tree. It keeps no database. The harness already stores the transcript (Claude's JSONL, Pi's session file), so resume is a native session id the app keeps.
3. **View model on the host side.** The core sends "upsert item by stable id" updates, as T3 does. Clients then need no reducer logic, which is what keeps a Swift or Kotlin client small.
4. **Protocol:** JSON-RPC over stdio for local embedding and WebSocket for remote, defined in JSON Schema so types are generated per language.

## Decisions for you

- **Core language.** I'd start in TypeScript, shipped as a single executable.
  - Why TypeScript: Claude is driven through the TypeScript Agent SDK, Pi is itself TypeScript, and every T3 workaround ports almost verbatim.
  - The cost: apps embed a sidecar process, not a linked library.
  - The alternative: a Rust or Zig core is possible because every harness is a subprocess protocol, but it means re-deriving the Claude control protocol without the SDK. I have not verified how stable that is.
- **Own protocol or ACP.** ACP already is an agnostic agent protocol, and adapters exist for Claude and OpenCode. But T3's shared ACP adapter has no standard subagent lineage, plan cards, per-turn usage or background work, and needs about 80 flavor hooks. I'd define your own protocol for the UI side and use ACP as one input adapter, which gets you the accessory harnesses cheaply.
- **Pi approvals.** Pi has no native permission prompts. T3 gets them by injecting its own extension, so your library must ship one too or offer Pi without approvals.

## Keeping upstream flowing

You cannot merge from a 15,000-line file you have restructured. What does transfer:

- **Replay fixtures.** T3 records real provider transcripts and replays them through the adapter. Adopt the same format and you can reuse their recordings as your test corpus.
- **The fix stream as a backlog.** Of the 230 Claude commits last month, roughly 100 concern the core loop (liveness, wakes, subagents, steering, resume). That is the maintenance rate you inherit.

## Spike plan

1. Run `provider-pi` standalone with a minimal host and print its events. This shows how much upstream code survives outside the app.
2. Write the item and event schema from T3's turn items, trimmed to what a chat view renders.
3. Build a Claude adapter against the Agent SDK for one path: turn, tool call, approval, question, subagent.
4. Drive both from a throwaway Swift client and a web client over the same protocol.

Step 1 is the cheapest and decides whether Pi is a wrap or a rewrite. Say if you want me to run it.

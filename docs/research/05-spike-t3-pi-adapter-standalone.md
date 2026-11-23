# Spike: T3 Pi adapter run outside the app

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

> Historical: this explored wrapping T3 code. The project is now greenfield and uses T3 only as a source of specs and recorded transcripts.

**The question asked:**

> yes, run step 1

---

Step 1 worked: T3's Pi adapter runs outside the app with no orchestrator, server or database. Pi is a wrap, not a rewrite. This was a replay of T3's recorded Pi transcripts, not a live turn, because Pi isn't installed on this machine.

## What I ran

- **Harness:** about 100 lines of driver code plus T3's replay spawner copied from its testkit. The adapter needed five services: a process spawner, the filesystem, the ID allocator, MCP sessions and the 87-line test host.
- **Code under test:** your `t3code-minitel` worktree (Nov 14), because the main checkout has no `node_modules`. I did not install anything.

| Transcript | Result |
|---|---|
| `simple` | One turn, reasoning and reply items, completed. Transcript fully consumed. |
| `multi_turn` | Two turns on one session. Transcript fully consumed. |
| `turn_interrupt_mid_tool` | Command item went running, Stop turned it interrupted, turn ended interrupted. The adapter then sent one extra `get_session_stats` the recording doesn't contain. |

A turn looks like this on the event stream:

```
provider_turn.updated   turn #1 [running]
turn_item.updated       item reasoning [completed] The user wants me to run a specific command…
turn_item.updated       item command_execution [running] bash
> user: STOP
turn_item.updated       item command_execution [interrupted] bash
turn.terminal           TERMINAL interrupted disposition=reusable
```

## What it costs to wrap

- **Dependency weight:** the adapter loads 78 files of `contracts` (29,000 lines), 24 of `shared` and 29 of `provider-core`, plus Effect. Nothing from `apps/server`.
- **Runtime:** 0.32 seconds for the whole replay and about 200 MB resident memory.
- **The interface leaks the orchestrator:**
  - Starting a turn needs a full app-thread record (about 20 fields), a run id, an attempt id, a root node id and two ordinals.
  - Events are entity upserts with long derived ids; one provider turn id was about 190 characters.
- **Distribution:** the packages are private, so you would vendor them (subtree or submodule) and follow upstream by merging.

## What this does not show

- **Live behaviour:** no real Pi process ran, so the injected extension, approvals and questions are untested.
- **Streaming:** items arrived already completed. A replay delivers frames instantly, so I can't say what live deltas look like.
- **Current main:** the code is a week behind.

## What I'd do with this

Wrap the adapters behind your own protocol: a thin session engine that fabricates the orchestrator-shaped inputs and translates the event stream into your item model. The same interface is shared by OpenCode, Cursor, Muse and the ACP flavors, so they come along for the same work.

The open question is Claude, your main need. It implements the same interface but lives in the server with 26 app-local imports. Repeating this experiment on the Claude adapter is the next step and will show whether it can be lifted or has to be rebuilt on the Agent SDK.

The runner is at `/private/tmp/claude-501/-work-t3code/dcaa0ec8-03b9-47a6-bdbe-90e81d7a6b80/scratchpad/spike/run-pi.mts`; that directory is temporary, so tell me if you want it kept somewhere durable.

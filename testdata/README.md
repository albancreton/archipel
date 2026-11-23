# testdata

Recorded sessions and what the core makes of them. Shared by the Rust core and, later, every client SDK.

## transcripts/

One file per recorded session: every JSON-RPC message between a client and an ACP adapter.

- Line 1 is a header: `{"transcript":1,"scenario":…,"agent":…,"harness":…,"adapter":…,"protocol":2,"model":…,"prompts":[…],"recordedAt":…}`.
- Every other line is a frame: `{"dir":"in"|"out","t":<ms since start>,"m":<the JSON-RPC message>}`. `in` is from the agent.
- Email addresses are replaced with `user@example.com`, and account status messages are left out.

| Set | Recorded from | With |
|---|---|---|
| `claude-v2/` | Claude Code 2.1.330 through `@agentclientprotocol/claude-agent-acp` 0.109.1, ACP v2, Haiku | `spikes/acp-probe/v2rec.mjs` |

The scenario names and several prompts are taken from or modelled on T3 Code's replay fixtures (MIT, Copyright (c) 2026 T3 Tools Inc.).

To add a scenario, add it to `v2rec.mjs`, run `node v2rec.mjs <name>` (a live, paid call), then `just vectors` and read the new file in `vectors/`.

A transcript can also stand in for an adapter: `bridged --fake <transcript>` replays it, and the daemon's tests do the same in-process (`bridge-agents`, `fake`).

## vectors/

For each transcript, a plain-text rendering of the state the core ends with: the items in order, tool output, approvals and their answers, subagent sessions indented, and the final session state. These files are the expected results of the test `rendering_matches_the_vectors`.

They are meant to be read. To check the core, open a vector next to what the harness showed for the same prompt.

After a deliberate change to the core or to the rendering, run `just vectors` and review the diff before committing.

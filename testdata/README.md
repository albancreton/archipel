# testdata

Recorded sessions and what the core makes of them. Shared by the Rust core and, later, every client SDK.

## transcripts/

One file per recorded session: every JSON-RPC message between a client and an ACP adapter.

- Line 1 is a header: `{"transcript":1,"scenario":…,"agent":…,"harness":…,"adapter":…,"protocol":2,"model":…,"prompts":[…],"recordedAt":…}`.
- Every other line is a frame: `{"dir":"in"|"out","t":<ms since start>,"m":<the JSON-RPC message>}`. `in` is from the agent.
- Email addresses are replaced with `user@example.com`, and account status messages are left out.
- Before a recording is committed, `scripts/scrub-recordings.py` takes out what is personal: the home folder and project paths (now `/work/…`), the account's plan, and commands that come from plugins. Then `just vectors` remakes what is derived from it.

| Set | Recorded from | With |
|---|---|---|
| `claude-v2/` | Claude Code 2.1.330 through `@agentclientprotocol/claude-agent-acp` 0.109.1, ACP v2, Haiku | `spikes/acp-probe/v2rec.mjs` |
| `pi-v2/` | The Pi recordings in `adapters/pi-v1`, as the Pi profile translates them to ACP v2 | the test `recorded_pi_sessions_come_out_as_acp_v2` |

The scenario names and several prompts are taken from or modelled on T3 Code's replay fixtures (MIT, Copyright (c) 2026 T3 Tools Inc.).

To add a scenario, add it to `v2rec.mjs`, run `node v2rec.mjs <name>` (a live, paid call), then `just vectors` and read the new file in `vectors/`.

A transcript can also stand in for an adapter: `archipeld --fake <transcript>` replays it, and the daemon's tests do the same in-process (`archipel-agents`, `fake`).

## adapters/

Recordings of adapters that do not speak ACP v2, in the same file format. `pi-v1/` holds twelve sessions of Pi 1.6.0 through `pi-acp` 0.0.35 (ACP v1). They are the input of the Pi profile's tests; its output is `transcripts/pi-v2/`.

## vectors/

For each transcript, a plain-text rendering of the state the core ends with: the items in order, tool output, approvals and their answers, subagent sessions indented, and the final session state. These files are the expected results of the test `rendering_matches_the_vectors`.

They are meant to be read. To check the core, open a vector next to what the harness showed for the same prompt.

After a deliberate change to the core or to the rendering, run `just vectors` and review the diff before committing.

## states/

For each transcript, the exact state the core ends with, as JSON. The client SDKs in other languages apply the same transcript and must get exactly this.

## daemon/

For seven sessions, what an app sees through the daemon: `live.ndjson` (the session as it happened), `replay.ndjson` (the same session read back by an app that attached later), and `state.json` (what both must end in). Written by the daemon's tests.

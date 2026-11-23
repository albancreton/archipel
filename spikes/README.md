# Spikes

Throwaway scripts kept as a record. They were written in a scratch directory and contain absolute paths from that session; expect to edit paths before re-running.

## acp-probe

A raw ACP client (newline-delimited JSON-RPC over stdio, no SDK) that drives an ACP adapter through named scenarios and logs every message.

- `client.mjs`: `node client.mjs <claude|pi> <scenario> [--caps] [--air]`
- `scenarios.json`: the prompts. Most are T3 Code's replay-fixture prompts or close variants.
- `runpi.sh`: runs Pi scenarios with the shell profile loaded and a chosen model (`PROBE_MODEL`).
- `logs/`: one `.ndjson` per run, every inbound and outbound message with a timestamp. Email addresses were replaced with `user@example.com`. These are the first recorded transcripts for a replay test suite.

Setup: `npm install` here (installs `@agentclientprotocol/claude-agent-acp` and `pi-acp`), create a `work/` directory with a small `package.json` and `tsconfig.json`. Claude runs use `ANTHROPIC_MODEL=haiku` and `CLAUDE_CODE_EXECUTABLE` pointing at the installed `claude`. Live runs cost money or tokens.

Known quirk: without `PROBE_MODEL`, a Pi session uses Pi's default model.

## t3-pi-replay and t3-claude (historical)

Run T3 Code's Pi and Claude adapters outside the T3 app, against T3's recorded transcripts, and (Claude only) live.

- They import T3 source by absolute path from a T3 checkout with installed dependencies (`/work/t3code-minitel` at the time) and need a `node_modules` symlink to that checkout's `packages/provider-pi/node_modules` or `apps/server/node_modules`.
- `hook*.mjs` are Node module hooks that measure which files get loaded.
- These belong to the abandoned "wrap T3's adapters" direction. They remain useful to see how T3's adapters behave on a given transcript.

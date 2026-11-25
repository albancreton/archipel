# Spikes

Throwaway scripts from the first experiments, kept as a record of how the findings in notes 05, 06, 09, 10 and 11 were made. They were written in a scratch directory and contain paths from that session; expect to edit paths before running one again.

Their raw logs are not kept in this repository. The recordings the library is tested with are in `testdata/`, and are made by `tools/record`.

## acp-probe

A raw ACP client (newline-delimited JSON-RPC over stdio, no SDK) that drives an ACP adapter through named scenarios and logs every message.

- `client.mjs`: `node client.mjs <claude|pi> <scenario> [--caps] [--air]`
- `scenarios.json`: the prompts. Most are T3 Code's replay-fixture prompts or close variants.
- `runpi.sh`: runs Pi scenarios with the shell profile loaded and a chosen model (`PROBE_MODEL`).
- `m0.mjs`: the milestone 0 probes (`multi`, `load`, `permwait`, `bg`, `think`, `todo`, `picompact`, `v2`, `v2b`); `m0pi.sh` runs one for Pi.
- `fork.mjs`, `list.mjs`, `twoproc.mjs`: one question each (forking, listing, one session in two processes).

They need the adapters that `tools/record` installs.

## t3-claude, t3-pi-replay

The runners of notes 05 and 06: T3 Code's Claude and Pi adapters driven outside T3.

# Watching upstream

2026-11-24. Step 6 of the second plan (section D of note 21): a script that looks for new versions of what the library depends on, writes down what changed, and can hand that to an agent that writes the report an update starts from. It is in `tools/watch`, in TypeScript; how to use it is in its README.

## What was confirmed while writing it

Note 21 left "where each one publishes" open. It is:

| Source | Versions | Changelog |
|---|---|---|
| ACP SDK `@agentclientprotocol/sdk` | npm | `CHANGELOG.md` in its GitHub repository |
| ACP schema crate `agent-client-protocol-schema` | crates.io | none found; the source is compared |
| Claude adapter `@agentclientprotocol/claude-agent-acp` | npm | `CHANGELOG.md` in its GitHub repository |
| Claude Agent SDK `@anthropic-ai/claude-agent-sdk` | npm | `CHANGELOG.md` in its GitHub repository |
| Claude Code `@anthropic-ai/claude-code` | npm | `CHANGELOG.md` in its GitHub repository |
| Pi `@earendil-works/pi-coding-agent` | npm | `CHANGELOG.md` inside the package |
| Pi adapter `pi-acp` | npm | none |

The Claude Code package on npm is a launcher around a binary, so only its tool types can be compared; the changelog carries the rest. The registry of running sessions that the open-elsewhere check reads is in nobody's package: **a change to it would not be seen by this script.** Only the conformance scenario that depends on it would notice, and there is none yet that runs a second real Claude process.

## Decisions

- **The versions we are on are written in `sources.json`**, by hand, and that is the script's only state. Deriving them (from `Cargo.toml`, a transcript's header, a lock file) would be six different readers for seven sources.
- **Free by default.** Versions, differences, the schema comparison and asking the adapter what it can do cost nothing. The agent (`--report`) and the paid scenarios (`--live=all`) are flags.
- **The schema is compared by what we use of it**, not as text: methods, types, and the `x-state` rules. Most of a schema diff is wording.
- **A new adapter is asked directly** what it says at `initialize`, next to the old one. It is installed without running install scripts.
- **The conformance run is the real check.** `conformance/run.ts` takes `--config=FILE`, and the script points it at the new adapter.
- **The agent only reads**, and what it prints is the report. It cannot change the repository.

## The first run

Five of seven sources were ahead of us: ACP SDK 2.9.1 to 2.12.0, the schema crate 1.21.2 to 1.22.0, the Claude adapter 0.109.1 to 0.112.0, the Claude Agent SDK 0.3.330 to 0.3.332, Claude Code 2.1.330 to 2.1.332. Pi and its adapter are current.

What the free checks said:

- **The ACP v2 schema**: one type changed (`MessageMcpRequest` gains `sessionId` and `toolCallId`), no method added or removed, no state rule changed. The rest of a 42 KB diff is wording.
- **The Claude adapter at `initialize`**: it no longer says `_meta.claudeCode.promptQueueing`. Nothing of ours reads it.
- **The seven conformance scenarios that spend no model turn**, against adapter 0.112.0: all the same as the baseline.

The reports in `docs/updates/` were written by an agent on Haiku from this output. They are kept as the first example of what the script produces. Their main claims, not yet checked by a live run:

- A message sent while a turn runs is now steered into the turn instead of queued. `turn.message_while_busy` would show it.
- `requires_action` is now reported only when nothing else in the turn can go on. The "needs you" badge and `onAttention` key off that state.
- The adapter has a new `session-rewind` module, reached only through an extension we do not declare. This is where a rewind addon would start.

## What the first report caught in the script itself

The agent's report on the ACP SDK said the script's "state rules changed" was a false alarm: the script counted a changed `x-scope` as a state rule, and the rule generator reads `x-state` only. It was right; fixed, with a test.

## Not done

- **No schedule is installed.** Where it runs (this Mac on a timer, CI once there is a remote) and how often are the user's to decide, and a timer on the user's machine needs their say.
- The "replay our tests against the new schema" step of note 21 is not built. The schema comparison says whether a state rule changed, which is what that step was for; running the Rust tests on a swapped schema needs the crate bumped too and is the first step of the update itself.
- `session/options` of the new adapter is not compared separately; the free conformance scenarios cover it (`session.options`, `settings.listed`).
- The paid scenarios were not run against the new adapter (as agreed earlier).

## Later the same day: made to run elsewhere

The user wants it run by an agent platform on another machine that reports to them. Three changes for that:

- **It remembers what it told** (`target/watch/seen.json`): a newer version is news once, then `known`. Reports are written for news only.
- **`--json`** prints one object with `news`, and per source the versions, a headline per check and the files to read.
- **The adapter check installs both versions itself**, so it no longer needs this machine's conformance setup. `--live` still does, and says "not run" without it.

Checked on Linux the same day (x86-64, Node 24): the whole free run, including installing and starting both adapter versions, takes about 15 seconds once the downloads are cached.

`tools/watch/hermes` holds what the user's Hermes Agent needs to run it daily: a monitor script whose unchanged output keeps the agent asleep, and a skill for the report and the message.

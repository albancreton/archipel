# T3 Code provider report

_Session notes, 2026-11-22. Written by Claude during the research spike; kept verbatim._

**The question asked:**

> I'd like to get a full report on each server this project have: claude, codex, opencode, pi... 
> I know the project started to refactor some of them and some older ones are still living in the core project ? 
> what'd like to have is a run down of the data model and what each server do, what specificities they have, edge cases, known limitations, etc

---

All seven research passes are back. The short answer to your refactor question: 8 of the 11 drivers now live in `packages/provider-*`, and the 3 still in the core server are Claude, Codex and Antigravity.

## Where each provider lives

| Provider | Location | Transport | Process ownership |
|---|---|---|---|
| Codex | `apps/server` | `codex app-server`, JSON-RPC over stdio | One app-server per instance, shared by all its threads |
| Claude | `apps/server` | Claude Agent SDK in-process, spawning the user's `claude` | One CLI per thread |
| Antigravity | `apps/server` (ACP flavor) | `agy acp` over stdio | One per thread, private profile per instance |
| Cursor | `packages/provider-cursor` | `@cursor/sdk` (patched) in a worker subprocess | One worker for every Cursor session and instance |
| OpenCode | `packages/provider-opencode` | HTTP plus SSE | 1.x: a server per thread. 2.x: a server per instance |
| Pi | `packages/provider-pi` | `pi --mode rpc`, JSON lines over stdio | One per thread |
| Muse | `packages/provider-muse` | `muse serve` (MSP protocol) via its SDK | One per thread |
| Grok, Kiro, fx | `packages/provider-{grok,kiro,fx}` | ACP over stdio, flavors of the shared adapter | One per thread |
| ACP Registry | `packages/provider-acp-registry` | Generic ACP | One per thread |

- **Extraction history:** the framework (`provider-core`, `provider-testing`) and Pi, Muse, OpenCode, Cursor, Grok and the ACP packages were extracted on 2026-10-08. fx and Kiro were created as packages afterwards.
- **What is left:** nothing has moved since, and no doc or comment announces an extraction of Claude or Codex. Their adapters are single files of 15,194 and 9,367 lines.
- **The boundary:** packages reach the server only through `ProviderHost` (paths, settings, credentials, attachments, version policy). Claude and Codex still import a dozen app-local modules each, which is the work an extraction would have to cut.

## The shared data model

There are two halves: configuration, which is not event-sourced, and orchestration, which is.

**Configuration**
- **Driver kind** identifies an integration; there are 11.
- **Instance** is one configuration and account of a driver, stored in settings as `providerInstances`. Everything routes by instance, so two accounts on one driver share no state.
- **Provider snapshot** (`ServerProvider`) is the status card: installed, version, auth, models, skills, usage limits, supported permission modes. It is cached as one JSON file per instance.

**Orchestration** (events plus projection tables)
- **AppThread** is the user's thread. It can own several provider threads, one per provider it has used.
- **Run** is one user turn. A run has attempts (initial, steering restart, retry, recovery), and an attempt can span several provider turns.
- **ProviderSession** is the live process or connection. It is shared across threads only when the adapter declares it (Codex, OpenCode 2.x).
- **ProviderThread** is the handle on a native conversation: native id, head ref, pending background tasks, context usage, goal.
- **ProviderTurn** is one native turn, with token usage and model.
- **TurnItem** is a timeline row (message, reasoning, command, file change, plan, subagent, and so on).
- **RuntimeRequest** is an approval or question. Its `responseCapability` says whether it needs the live session, can be answered by a message, or is no longer answerable.

Native ids are treated as evidence with a strength (strong, weak, none); app ids are derived from them deterministically.

**Adapter contract** (`packages/provider-core/src/server/ProviderAdapter.ts`)
- An adapter exposes capabilities, a selection-transition plan, and `openSession`.
- The session runtime has required methods (ensure/resume thread, start/steer/interrupt turn, respond to a request, snapshot, rollback, fork) and optional ones that orchestration detects by presence (compact, resume turn, stop task, MCP sign-in).
- It reports back through an event stream that ends each turn with a `turn.terminal` event.

**Capabilities are mostly declarative.** Of the twelve capability groups, only about twenty flags are read outside adapters. They drive real fallbacks:
- **No native fork:** the fork becomes a text-summary handoff.
- **No native rollback:** the next turn opens a fresh native session and replays kept history.
- **No active steering:** interrupt and restart, or queue.
- **No in-session mode switch:** the session is detached and reopened.

All of `streaming.*`, `approvals.*`, `planning.*` and `subagents.*` have no consumer today.

**Lifecycle rules worth knowing**
- Sessions idle out after 30 minutes; pending background work pins them for up to 4 hours.
- Restart recovery assumes no provider process survives the server. Live approvals are cancelled; message-type questions stay answerable.
- Switching provider creates a lossy text handoff (16,000-token default cap). No tool state or approvals carry over.
- A usage limit is a failed turn with class `usage_limit` and an optional reset time; a sweep resumes the thread after the reset.

## Codex

- **Modes:** all four, mapped to Codex approval policy plus sandbox. Full access uses a granular policy instead of `never`, because `never` also rejects browser and app prompts.
- **Managed mode** is a second personality of the same driver: a T3-installed pinned binary (0.166.0), T3's own ChatGPT OAuth, and a token helper talking to a broker over a private socket. It has no native subagents, speed tiers, usage limits or `/feedback`.
- **Homes:** a shadow `CODEX_HOME` symlinks everything from the shared home except auth and model cache, so several accounts can share threads.
- **Questions:** async questions arrive as notifications and are answered with a normal user message, so they survive restarts.
- **`/goal`:** Codex starts the next goal turn itself, so the adapter holds the run open for 5 seconds to absorb it. One run can own several native turns.
- **Session approvals:** Codex's "accept for session" covers only the exact command, so the adapter keeps its own per-thread allowance.
- **Edge cases:**
  - A hung app-server is retired after 30 seconds on initialize or 2 minutes on a request; requests are never retried.
  - Overload is retried up to 5 times by starting an empty turn.
  - Instructions are split because Codex silently truncates context values over about 4,000 bytes.
  - Compaction drops injected context, so the adapter re-injects it.
- **Limits:**
  - Versions below 0.149 are refused; 0.156 or later is supported.
  - Legacy-history threads cannot be rolled back.
  - Subagent tokens are not in turn totals.
  - Background work dies with the process.

## Claude

- **Transport:** the SDK is in-process but always runs the user's own `claude` binary. Fork and history reads move to a child worker when the instance uses a custom config dir, because the SDK reads `CLAUDE_CONFIG_DIR` from the process environment.
- **Modes:** all four plus plan. If Claude's own settings disable bypass, Full access silently downgrades to auto or default. A mode change needs a new CLI process, which is refused while background work is running.
- **Rollback** rewrites nothing: it pins the conversation head to the target turn's last assistant message and resumes from there. Turns with no recorded cursor cannot be rolled back or forked.
- **Plan mode:** `ExitPlanMode` is captured as the proposed-plan card and answered with a deny, then the mode is restored on the next turn.
- **Background work:** turns Claude starts on its own (task notifications, monitors, scheduled wakeups) are buffered and replayed as continuation turns. Live output is tailed from Claude's private temp files.
- **`/goal`:** SDK mode emits no goal events, so state is parsed from synthetic transcript text.
- **Edge cases:**
  - The SDK has no boot deadline, so T3 adds a 5-minute one.
  - API failures can arrive marked as success.
  - Dangerous `rm` checks under bypass are auto-denied with guidance.
- **Limits:**
  - No thread snapshot.
  - Approvals from subagents are not surfaced.
  - Only the last skill in a message is expanded natively.
  - Each model has its own minimum CLI version; there is no global one.
  - The adapter rejects a fork during an active turn although the capability flag says it can.

## Cursor

- **Transport:** not ACP and not the `cursor-agent` binary. One shared worker hosts everything because the SDK costs about 250 MB per process.
- **Modes:** all four are offered, but there are no approvals or questions at all; Cursor's auto-review is internal. On Windows there is no sandbox, and Auto-accept edits cannot start.
- **Patched SDK:** the patch adds turn resume, a background-shell hook and shell output streaming.
- **Edge cases:**
  - Cursor discards a prompt cancelled before its first checkpoint, so the adapter replays it with an "interrupted" preamble.
  - A non-zero shell exit is reported as success, so the adapter checks the exit code.
  - Transport errors arrive as assistant text and are reclassified.
- **Limits:**
  - No native fork.
  - Subagent threads are not resumable.
  - No interactive MCP OAuth.
  - A paid plan is required.
  - Updates are manual.

## OpenCode

One driver, two runtimes chosen by probing the version.

| | 1.x | 2.x |
|---|---|---|
| Server | One per thread (or the external URL) | One per instance, random password |
| T3 MCP entry | Fixed name `t3-code`, safe only because each thread has its own server | Per-thread `t3-code-<thread>`, isolated by permission rules |
| Lost event stream | Turns fail, thread marked broken | Reconnects and reconciles |
| "Always" approval | Stored by OpenCode for the whole project | Replies `once`; T3 keeps the session rule itself |
| Rollback | Forks at the boundary and swaps sessions | Native revert, files left to T3 checkpoints |
| Mode switch in session | No | Yes |

- **Both:**
  - Auto behaves as Supervised.
  - No proposed-plan card.
  - External servers get no T3 MCP tools.
  - An orphan ledger reaps leftover servers on Linux and macOS only.
- **1.x edge case:** the user message is published before a cancellable runner exists and idle can arrive before busy, hence an admission state machine.
- **2.x edge case:** 2.2.5 returns 503 on model reads for about a minute at cold start, so the loader retries for 2 minutes.
- **Versions:** below 1.14.19 is refused; 1.x is "limited support"; 2.0.18 or later is supported. Updates never cross from 1.x to 2.x.

## Pi

- **Reference package.** T3 runs the user's own install and injects one generated extension, which is both the MCP bridge and the permission gate.
- **Modes:** Supervised, Auto-accept edits, Full access. Enforcement is the extension's tool-call hook, so it is not an OS sandbox, and extension code outside a tool call is ungoverned.
- **Sessions:** the native thread id is the session file path. Rollback is an in-place RPC fork; a thread fork runs a second short-lived `pi --fork` in the destination directory.
- **Edge cases:**
  - There is no reliable idle event, so the adapter probes state.
  - Prompts are fire-and-forget because the acknowledgement can block on dialogs.
  - Untrusted folders silently skip project resources in RPC mode.
- **Limits:**
  - No Auto or plan mode.
  - No usage limits.
  - Grants never survive a restart.
  - Changing mode restarts Pi.
  - Minimum version is 0.84.3.

## Muse Code

- **Modes:** Supervised and Full access only. Plan mode is rejected outright, and a mode change needs a fresh host.
- **Approvals and questions** are native. In Full access T3 auto-accepts workflow-child approvals, because Muse does not propagate "allow all" to them.
- **Wake handling:** a turn Muse starts unprompted is held, buffered and offered as a continuation.
- **Edge cases:** T3's MCP server is injected as optional, because Muse's default would fail runs when it is unreachable. Interrupting a compaction leaves the thread broken.
- **Limits:**
  - No rollback, no fork, no turn resume.
  - No in-app sign-in, and auth is always reported as unknown.
  - Child-session transcripts cannot be read.
  - Off by default.

## The shared ACP adapter and its flavors

`packages/provider-acp/src/server/adapter.ts` (10,861 lines) is the common base, built on `effect-acp`.

- **Protocol:** it speaks ACP v1 and the v2 draft, deciding by response shape.
- **Defaults are conservative:** no fork, no subagents, no active steering, no proposed-plan card, no per-turn token usage, no usage-limit classification. Rollback means a fresh session with replayed history.
- **T3 MCP tools** are injected by MCP-over-ACP if the agent advertises it, then HTTP if the flavor opts in, then a stdio bridge.
- **Flavors** customise through roughly 80 hooks rather than branches.

| | Grok | Kiro | fx | Antigravity |
|---|---|---|---|---|
| Modes | Supervised, Auto, Full | Supervised, Full | Supervised, Auto, Full (0.0.14+) | All four (Auto if advertised) |
| Mode mechanism | Launch arguments, relaunch on change | `autopilot` option | Session mode, re-written after every load | Session mode; two modes emulated by T3 |
| Steering | Native interject | Native, text only | Interrupt and restart | Second prompt |
| Fork | No | From a turn | No | No |
| In-app sign-in | No | No | No | Yes |
| Text generation | Yes | No | No | Only with no hooks or MCP config |
| Minimum version | 1.0.13 | 2.27.0 | 0.0.13 | 1.5.0 |

- **Grok:** prompt completion can beat the RPC response, so the flavor races the two. Plans and questions arrive as reverse requests. The status probe never opens a session.
- **Kiro:** requires the v3 engine. Stop is held up to 2 seconds because Kiro drops a cancel sent right after the prompt. No usage reader.
- **fx:** all instances share one machine login. The probe is CLI-only because every ACP session is saved and cannot be deleted, which is also why text generation is off. Subagent results are clipped to 199 bytes and repaired.
- **Antigravity:**
  - Each instance gets a private profile with ambient Google credentials stripped.
  - The status probe never spawns the agent, because each launch unpacks about 1 GB.
  - A managed installer keeps immutable releases with per-process leases.
  - Three hooks read the agent's native session database directly to detect failures and outliving subagents.

## ACP Registry

- **What it is:** one driver for any agent in the public ACP registry, or a local executable.
- **Install:** T3 installs binaries or exact-version npm/uv packages into its own prefixes.
- **Auth:** agent, terminal and environment-variable methods are supported.
- **Per-agent fragments:** Claude ACP, Copilot, Devin, Droid, Kimi, OMP and OpenCode ACP each have a workaround file here.
- **Limits:** no text generation, updates are manual.

## Inconsistencies I found

- **Agent-id checks in the registry adapter.** The internal doc says never to add them to the generic registry adapter, but that is exactly where the per-agent fragments are selected.
- **Registry probe opens a session.** Its background probe calls `session/new`, against the "status checks must not open a session" rule.
- **Driver-name branches in orchestration.** For example `session.driver !== "codex"` in `ProviderTurnStartService.ts`, despite the "capabilities, never driver kind" rule.
- **Capability flags that look wrong:**
  - OpenCode 2.x declares `supportsMcpTools: false` while injecting T3's MCP server.
  - Grok captures plans with `emitsProposedPlan: false`.
  - Neither flag has a consumer, so these are cosmetic today.
- **Doc drift.**
  - `docs/orchestration-v2/*` describes an identity-binding store that does not exist.
  - `providers.md` says automatic full-access replies use `once`, which matches OpenCode 2.x but not 1.x.
- **Stale comment.** The Cursor SDK code names a 1.2.0 patch; the actual patch is 1.4.2.

## Coverage caveats

This was read, not run.
- **Claude adapter:** lines 6500–12600 were covered by comments and excerpts, not every statement.
- **Shared ACP adapter:** the subagent and background-work region was covered by outline.
- **OpenCode and Pi adapters:** read by a delegated reader and spot-checked.
- **Antigravity rollback** is the one point I would verify before relying on it: `providers.md` says revert is rejected before touching files, while the adapter research found a fresh-session replay.

I can turn this into a shareable page with the full per-provider capability tables if you want it outside the terminal.

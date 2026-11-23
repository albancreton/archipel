# Milestone 2: the first running daemon

_2026-11-22. Checked by `cargo test --workspace` (34 tests) and by one live run with the real Claude adapter, described below._

## What exists now

Two programs and five more crates with real code.

- **`bridged`**, the daemon. It listens on a local socket (and on a WebSocket when asked), starts adapters, keeps the session log in SQLite.
- **`bridge`**, a command-line client: `agents`, `sessions`, `new`, `prompt`, `watch`, `show`, `answer`, `cancel`, `set`.
- **`bridge-transport`**: one codec for lines over a byte stream, a Unix socket and a WebSocket.
- **`bridge-store-sqlite`**: sessions and log entries in one file.
- **`bridge-agents`**: starting adapters, the Claude profile, and a fake adapter that replays recorded sessions.
- **`bridge-server`**: the daemon as a library.
- **`bridge-client`**: the connection a client needs, and the `bridge` program.
- **`bridge-core`** gained log records for requests, a store interface, and the client-side "mirror".

Release builds: the daemon is 4.8 MB, the client 2.2 MB, with no runtime to install.

## What it does

| Behaviour | How it was checked |
|---|---|
| Everything the adapter sends reaches the app unchanged and in order, with a sequence number | Test over all 17 recorded sessions |
| What an app holds at the end equals `testdata/vectors` | Same test: live, after a full replay, and through plain ACP `session/resume` |
| An app that lost its connection continues from its last sequence number | Test over all 17 sessions; live run (WebSocket watcher) |
| An unanswered approval waits, is offered to whoever attaches, and the first answer wins | Test; live run |
| A restarted daemon lists and replays its sessions with no adapter running | Test with a SQLite file; live run |
| The next prompt for such a session starts the adapter and resumes the session there | Test; live run, context intact |
| When the adapter or the daemon stops mid-turn, the open approval is cancelled and the turn is reported as stopped | Test; live run |
| Update kinds, fields and methods the daemon does not know pass through | Test with invented `x.acme.*` content |
| The WebSocket refuses a wrong token and any browser origin | Test |
| The local socket is owner-only and a second daemon on it is refused | Test |

## How to try it without spending anything

The fake adapter replays a recorded session at the recorded pace:

```
cargo build
target/debug/bridged --home /tmp/bridge-demo --fake testdata/transcripts/claude-v2/approval_command.ndjson &
alias b='target/debug/bridge --home /tmp/bridge-demo'
b new --mode default            # prints the session id; the first characters are enough afterwards
b prompt <id> hello --watch     # stops when the agent asks for approval
b answer <id> allow-once
b watch <id> --until-idle
b show <id>                     # compare with testdata/vectors/claude-v2/approval_command.txt
```

The fake follows the recording, so the commands have to come in the recorded order (here: new, set mode, prompt, answer).

## The live run

Claude Code 2.1.330, Haiku, `claude-agent-acp` 0.109.1, through `bridged` with a `config.json` pointing at the adapter. Three short turns.

1. `bridge new --mode default`, then a WebSocket `bridge watch` left running in the background.
2. A prompt that runs a shell command. `prompt --watch` stopped at the approval; `bridge answer … allow-once` from a second connection; the turn finished and the file was written.
3. A second such prompt. While its approval was open, the daemon was stopped with Ctrl-C.
4. The daemon was started again. The watcher reconnected by itself and received exactly what it had missed.
5. A third prompt. The daemon started the adapter, resumed the session, and Claude answered with the earlier turns in context.

What the watcher printed around the restart (numbers are sequence numbers):

```
   35 [state_update] requires_action
   36 [session/request_permission] printf 'again' > m2-probe2.txt
        option `allow-once`: Yes
        option `reject`: No
   37 [usage_update]
-- connection lost at 37; retrying
   38 [answered] {"outcome":{"outcome":"cancelled"}} by daemon
   39 [notice] The turn was interrupted
   40 [state_update] idle · cancelled
-- reconnected: delta from 37 to 40
   41 [session_info_update] M2-probe.txt file write
   42 [state_update] idle
   …
   50 [agent_message] back
   53 [state_update] idle · end_turn
```

Also seen in that run:

- No adapter process was left behind after the daemon stopped.
- The log held 53 entries (49 updates, 2 requests, 2 answers) in 172 KB, and no account details: the adapter's `auth/status_update` messages, which carry the user's email address, are neither stored nor passed on.

## Decisions taken while building

**The JSON-RPC layer is ours, not the `agent-client-protocol` crate's.** The design had "to verify: the connection layer must preserve unknown kinds". The daemon forwards messages it must not interpret; a typed connection would drop kinds and fields its types do not know. A line reader and writer over JSON values is about 60 lines and preserves everything. The official types are still used to check shapes in tests.

**One task owns all state.** Messages from apps, messages from adapters and closed connections arrive as events on one queue and are handled one at a time. There are no locks. The file `crates/bridge-server/src/daemon.rs` starts with a table of what happens for each kind of message.

**Requests are in the log.** The ACP state rules let a permission request create its tool call, so a replay without requests could show different items than the live session. The log therefore has three kinds of entry: update, request, answer.

- A request that can still be answered is sent as a real JSON-RPC request, with a stable id (`<session id>#<sequence number>`).
- In a replay, an answered request cannot be asked again. It is sent as one `_bridge/request/resolved` notification carrying the question and the answer, at the request's place in the order.
- This pulled "first answer wins" forward from milestone 3: it falls out of the same code.

**Setup values are written into the log.** `configOptions` and `availableCommands` arrive in the response to `session/new`, not as updates. The daemon writes them as `config_option_update` and `available_commands_update` entries, marked `_meta.bridge.origin = "daemon"`, so the log alone rebuilds a session.

**Attach answers first, then sends the catch-up.** The app has to know whether a replay or a delta follows before it applies anything. `session/resume`, which the daemon also accepts so that a plain ACP v2 client works, follows ACP's order instead: replay, current state, then the response.

**`_meta.bridge.rootSessionId`.** Sequence numbers count per conversation, and a subagent's messages carry the child session's id. This field says which log a message belongs to.

**SQLite for the log**, as recommended in the design. Write-ahead log with `synchronous = NORMAL`: a crash of the daemon loses nothing; a power cut may lose the last moments, which the harness still has. The daemon only sees a `Store` trait, so this stays easy to change.

**A slow app is disconnected, not buffered.** Each connection has a queue of 4096 messages. An app that falls further behind is dropped and catches up when it attaches again, which is what the log is for.

**The WebSocket is off by default and closed when on.** It needs `Authorization: Bearer <token>` (the token is in a file only the owner can read) and refuses any request with an `Origin` header, because any web page may try to open a WebSocket to a local port.

**Chunks are merged at replay time, not in storage.** The design said "merged in storage". Merging on the way out is simpler and loses no information; the cost is a log that grows with the number of chunks. Revisit with retention.

## What is not done, or not right yet

Left for the milestone that owns it:

- **Scopes** (observe, interact, admin). Today every connected app may do everything. Milestone 3.
- **Pi**, and everything adapter-specific. Milestone 4.
- **Pairing**, SSH stdio mode, TLS notes. Milestone 7. Browser clients cannot connect yet: they cannot set the token header, and origins are refused.
- **Windows**: the local transport is a Unix socket only.

Gaps found while building:

- **Sessions created outside the daemon** (in the harness's own UI) are not listed and cannot be adopted.
- **`mcpServers` are not remembered.** A session woken after a restart is resumed with an empty list.
- **Settings after a wake-up were not checked.** Whether Claude resumes in the mode that was set before, or in the user's default, is unknown.
- **A tool call that was waiting when a turn was cut off stays `pending`** in the item list. The turn state says `idle`, so a client can tell, but the item itself is stale.
- **Agent readiness is coarse.** `_bridge/agents/list` says `ready` unless the last start failed. Login state is not read.
- **Storage errors drop the event** with a line on stderr. There is no retry and no signal to apps.
- **SQLite writes happen on the daemon's task.** Fine at this scale; move to a writer thread if a slow disk ever shows.
- **Stopping an adapter is abrupt** (closed pipes and a kill). No process was left behind in the live run; a gentler shutdown is not implemented.
- **The Unix socket path is limited to about 100 bytes** by the operating system. A deep `--home` fails at start with the system's error.
- **Retention** is not implemented.
- **Notices from an adapter** have still not been seen. The daemon now writes one itself when a turn is cut off.
- **A third-party ACP client was not tried** against the daemon's `session/resume`. Only the test client was.

## Side effects of the live run

- One more short Haiku session in `~/.claude/projects`, three turns.
- The daemon's home for the run is `target/m2-home` (ignored by git): config, log, token.

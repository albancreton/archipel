# One session, two Claude processes

_2026-11-22. Probe: `spikes/acp-probe/twoproc.mjs` (three short Haiku turns), plus a read of Claude Code's files on disk. Claude Code 2.1.330._

## The question

The daemon holds a session in one harness process. Nothing stops the user from resuming the same session in Claude's own terminal or the VS Code extension. What happens then, and what does Claude Code itself do about it?

## What Claude Code does

**It does not block.** A second process resumed a session the first still held, and was accepted.

**Each process has its own memory.** Process A was told ALPHA. Process B resumed the session and was told BRAVO. Asked afterwards for every codeword, A answered `ALPHA`. It never saw B's turn.

**The transcript forks instead of breaking.** Messages in the transcript file point at the message before them. After the probe, one message had two follow-ups: A's next turn and B's turn. The file stays valid; it holds two branches.

**A later reader gets one branch.** A third process that replayed the session got A's branch (the last one written). B's turn is in the file and shown to nobody.

**It keeps a registry of running processes.** Each Claude Code process writes `~/.claude/sessions/<pid>.json` with its pid, the session id, the working directory, how it was started, and whether it is `busy` or `idle`. Processes started by the ACP adapter are listed too. During the probe the same session id appeared under two pids.

So the answer to "how did they solve it" is: they did not prevent it. They made it harmless to the file, and they record who has what open.

## What this gives the daemon

For Claude, the registry answers the question the daemon could not answer before: is this session open in another process right now, and is that process in the middle of a turn?

- Before adopting a session, or sending a prompt to one, the daemon can look for another live process with that session id.
- "Live" can be checked: the file has the pid and the process's start time.

Limits:

- The registry is Claude Code's own file layout, not a documented interface. It can change with any release, so the daemon must treat a missing or unreadable registry as "unknown", not as "free".
- It is a check, not a lock. Two processes can still start at the same moment.
- Pi was not probed. Whether it has anything similar is unknown.

---
name: archipel-upstream-watch
description: Report what changed upstream of the archipel library (ACP, the Claude adapter, Claude Code, Pi) when its watch script finds a newer version. Used by the daily cron job of the same name.
version: 1.0.0
---

# archipel upstream watch

archipel is a library that lets apps drive coding agents (Claude Code, Pi) through a daemon. It depends on things that move: the ACP schema, the adapters, the harnesses. A monitor script (`archipel_watch.py`) runs the repository's watch script every day. You are woken only when its output changed, with the difference in your prompt. Each line is `<source> <version in use> <newest version>`.

The repository is at `~/Dev/archipel` (your working directory). **Never change, commit or push anything in it.** Write only under `target/watch/`, which git ignores.

## What to do

1. Read the difference in the prompt.
   - Only lines removed, or `everything is at the newest version`: the library caught up. Answer with exactly `[SILENT]`.
   - A line added or changed: that source has a version nobody was told about. Go on.
2. Read `target/watch/out/<newest date>/result.json`. For each source in the difference it gives `headlines` (one line per check) and `files` (what was written for it: the diff, the changelog, schema changes, what the adapter says at `initialize`). `summary.md` next to it has the same in prose.
3. For each such source, write a report by following `tools/watch/brief.md` exactly: it says what to read, which sections to write and what to look for in this repository. Search the diff for what matters instead of reading all of it. Save the report as `target/watch/reports/<date>-<source>.md`.
4. Answer with the message for the user (below). That answer is what gets delivered.

If something fails (a file is missing, the diff cannot be read), say so in the message rather than guessing.

## The message

Short, for a phone. No tables.

```
archipel: <n> upstream update(s)

<source>: <from> → <to>
<the two or three lines of "In short" that matter most>
Work: <none | small | medium | large>

(next source…)

Reports: lilb:~/Dev/archipel/target/watch/reports/
```

Put first whatever needs a decision: a state rule that changed, a behaviour the library relies on that changed, a fix that makes one of its addons unnecessary. If a source changed nothing that touches the library, give it one line.

## Rules

- Say only what the diff, the changelog or the repository shows. What you could not check goes under "What I could not judge" in the report, and is not stated as fact in the message.
- The watch script and the monitor cost nothing; you are the only paid step. Do not run the conformance tests (`--live`) or any model-backed command.
- Do not create, edit or remove cron jobs.

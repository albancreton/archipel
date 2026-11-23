# Conformance tests: the first run, and what it found

_2026-11-23. The user: "start with the conformance test"._

## What was built

`conformance/`: a runner in TypeScript and twelve scenarios. Each scenario drives a real harness through a daemon of its own and records what it saw; a baseline per agent says what to expect next time. How to run it is in `conformance/README.md`.

| Scenario | Question | Claude | Pi |
|---|---|---|---|
| `settings.listed` | Which settings does a new session report before any message? | yes | yes |
| `settings.switch` | Does a change show? Do the effort choices depend on the model? | yes | yes |
| `settings.rejects_unknown` | What happens with a value the agent does not offer? | yes | yes |
| `reopen.keeps_settings` | After a daemon restart, does a session keep its model, effort and mode? | yes | yes |
| `session.empty` | Does a session with no message leave an entry in the harness's list? | yes | |
| `session.options` | Can settings be read without a session, and for an existing one? | yes | |
| `session.rename_delete` | Rename and delete, at the adapter and through the daemon | yes | |
| `turn.interrupt` | Does stopping a turn end it? | yes | yes |
| `turn.message_while_busy` | What happens to a message sent during a turn? | yes | yes |
| `shell.output_streams` | Does command output arrive before the command ends? Is approval asked? | yes | yes |
| `reasoning.arrives` | Does reasoning arrive as items? | yes | yes |
| `memory.project_file` | Is the project's instruction file loaded? | yes | yes |

Versions: Claude Code 2.1.330 with adapter 0.109.1; Pi 1.6.0 with `pi-acp` 0.0.35 on the local model. Both baselines are committed. A second run of the free scenarios matched them.

## What it found

### Problems

1. **A reopened Claude session loses its model.** A session put on another model, used, and woken after a daemon restart ran its next turn on the configured default. The cause is in the adapter (`session-model.js`, read in code): the starting model is taken from, in this order, the `ANTHROPIC_MODEL` variable, the user's `settings.json` model, and only then the session's own transcript. Anyone with a default model set, which is the normal case, gets that default back on every reopen.
2. **A reopened Claude session loses its effort.** `low` came back as `default`. Cause not looked into.
3. **Until the wake-up, the app is shown the old choice.** The daemon shows what its log says, so after a restart the session still displays the model and effort the user picked; the first message then runs on something else and the display changes. Nothing warns the user.
4. **A delete through the daemon leaves the session in the daemon's list.** The call reaches the adapter and succeeds there; the daemon's log is not touched.
5. **Pi accepts an effort value it does not offer.** `no-such-value` was answered with success. Either the translator or `pi-acp` does not check.

### Answers to open questions

- **The settings arrive with a new session, before any message**: model, effort, mode and one on/off setting for Claude; model and effort for Pi.
- **The effort choices do depend on the model.** An older Claude model has no effort setting at all: selecting it removes the setting from the list. Selecting it also changed the permission mode from `auto` to `acceptEdits`. A settings bar must redraw from the list after every change and assume nothing.
- **A session that never got a message is not in Claude's list.** Creating one to read the settings leaves no trace there.
- **`session/options` works**, in about 270 ms, with no session and for an existing one. For an existing session it reported the mode the user chose but the default effort, which matches finding 2. It also passes through the daemon today.
- **Unknown values are refused by Claude** with error `-32602`, including the terminal's `dontAsk` mode, and the setting stays as it was.
- **The mode is kept across a reopen** (Claude).
- **Stopping a turn** ends it with `cancelled` on both harnesses, and the next turn works.
- **A message sent during a turn** is accepted, shown and answered on both.
- **Command output streams.** With an eight-second command, output arrived before the command ended on both harnesses. Claude asked for approval first; Pi did not.
- **Reasoning arrives** on both (Claude with the summarised display the config asks for).
- **The project's instruction file is loaded** on both.

### Seen once, not in the baseline

While isolating finding 1: with Haiku selected and plan mode on, Claude Code ran the turn on Sonnet, and the model setting showed `sonnet` after the turn. After a reopen the setting showed `haiku` while the turn still ran on Sonnet. The reopen scenario avoids plan mode for this reason. It deserves a scenario of its own.

### Not conclusive

- **Rename.** The call succeeds, but the scenario renames a session that has no message, and such a session is not listed, so the new title could not be read back. It needs a session with a turn.
- **Pi keeping its effort across a reopen.** The setting shows `low` after the wake-up, but that is what the translator reports; Pi gives no independent sign of the effort it used.
- **The time of the first message after a reopen** was about a second, on sessions of two messages. It says nothing about long sessions.

## What follows for the library

Not built; proposed.

1. **The daemon re-applies the user's choices after a wake-up.** It already logs every setting. After `session/resume` it compares what the agent reports with what the log says and sets what differs, before forwarding the message. This fixes findings 1 to 3 for every harness, whatever the adapter does.
2. **`session/delete` removes the session from the daemon's log too**, with the admin scope.
3. **The Pi translator refuses a value that is not in the list.**
4. **More scenarios**, from `22-claude-terminal-vs-adapter.md`: hooks, the built-in commands sent as text, plan mode and its model, rename on a used session, images, the idle compaction.

## What it cost

About 35 short Claude turns over the runs it took to write the scenarios. Five of them ran on Sonnet instead of Haiku, because of the plan-mode behaviour above, before the cause was understood. Pi ran on the local model. Every Claude session was deleted from Claude's history by the runner; Pi's were removed by hand from Pi's folder for the work directory.

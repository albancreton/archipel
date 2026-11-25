# Components: agent and connection status, usage, background work, attention

2026-11-24. A9 to A13 of the second plan, the last of section A, in React and SwiftUI.

## Agent status (A9)

The daemon's report was too coarse to draw: an agent it had never started was `ready`, and a missing program was a plain `error`. `not_installed` was in the schema and never sent. Now:

| Readiness | When |
|---|---|
| `not_started` | The daemon has not started it. Nothing is known; not a fault |
| `starting` | Its process runs and has not answered `initialize` |
| `ready` | Started and answering |
| `needs_login` | The adapter says the user is not signed in |
| `not_installed` | The last start failed because the program was not found. `detail` names it |
| `error` | It could not be started, or stopped. `detail` says why |

This is a change to the daemon's core, and it is ours to make: it is our report, not a harness fault. `AgentStatus` draws a line per agent that cannot be used and nothing otherwise. Seen in a browser with a deliberately missing adapter.

Not covered: Claude Code itself missing while its adapter is installed (the adapter starts, then fails later), and a wrong Node version. Both arrive as `error` with whatever the adapter printed. `bridged setup` (B1 of the plan) is where those would be detected properly.

## Connection status (A10)

The TypeScript client gained `connected` and `onConnectionChange`; the Swift client already had an observable `connected`. `ConnectionStatus` says "Reconnecting…" and that what is shown may be out of date.

## Usage (A11)

What the harnesses pass: the context's size and use (both), a running cost in dollars (Claude only), the last turn's token counts (Claude). **No rate-limit or subscription-quota information comes through** from either; that stays a gap of the adapters (`22-claude-terminal-vs-adapter.md`). The status bar now shows a small meter, the percentage, and the cost when there is one; the tooltip has the token counts.

## Todo list and background work (A12)

Checked live (new scenario `todos.listed`): **neither harness has a todo tool in the sessions the daemon starts**. Asked to make a task list, Claude (through its adapter) and Pi both searched for the tool and said they have none. So no todo list reaches an app today. The Claude adapter does map todos to ACP plan entries in its code, as note 22 found; nothing triggers it.

`Activity` therefore does two things, one of them only on paper so far:

- **Background work**: real. After a turn that leaves a command running, the session lists it (`state.background`), and the panel shows the command. Seen live with a `sleep` started in the background.
- **Todo list**: the newest plan that has entries, with "n of m done" and the entry in progress. Tested on written data only. It will show as soon as a harness sends entries.

## Needs attention (A13)

`onAttention` on the client: called once when a session starts to wait for an answer, and once when a turn ends. It works from the session summaries the daemon sends to every app, so it covers sessions the app has not opened. A repeat of the same state (a title change meanwhile) does not fire again. Whether the user is looking, and what a notification looks like, is the app's.

## Also

The command menu now lists commands whose name starts with what was typed before those that only contain it (`/re` gave `deep-research` first; seen in the browser).

## Numbers

106 Rust tests, 51 TypeScript, 53 React, 12 Swift in the kit and 10 in the client, all passing.

## Section A is done. What it leaves

- No test clicks anything: menus, pickers, dialogs, folds and "Show earlier" are tested as functions and as drawn output only. Step 5.
- SwiftUI was never looked at on screen in this step, only as pictures drawn off screen, where system controls (text fields, some buttons) do not draw.
- No syntax highlighting anywhere, no virtual list, no terminal emulator: each is a dependency an app can add at the swap points.
- A picture a user attaches is stored in the daemon's log in full.

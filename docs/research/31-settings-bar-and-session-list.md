# Components: settings bar and session list

2026-11-24. Start of the fourth step of the second plan (`21-second-plan.md`, section A): A2 and A6, in React and SwiftUI. They came first because the pass-through step had just made them possible.

## Settings bar (A2)

- **Generic.** It reads the agent's `configOptions`: a `select` becomes a menu (with headings when the agent groups the values), a `boolean` a checkbox or toggle. A kind it does not know is left out. The category the agent gives (`model`, `thought_level`, `mode`) is kept on the setting and as a CSS class, for an app that wants to place or style one of them.
- **In a session:** the chat views draw it above the composer and call `session/set_config_option`. While the request runs the control shows the new value; a refusal is shown under the bar and the control goes back. Seen in a browser against the fake agent, which refuses every change.
- **Before a session:** `useDraftSettings` (React) holds the choices; they go into `session/new` as `configOptions`. The SDKs' `newSession` takes them, and `setConfigOption` now takes a switch as well as a choice.

Checked live, free (new scenario `session.initial_settings`, in both baselines):

| | Claude | Pi |
|---|---|---|
| Settings can be read before a session (`session/options`) | yes | no (error) |
| A new session starts with the effort chosen beforehand | yes | **no: `configOptions` on `session/new` is ignored** |

On Pi there is nothing to choose before a session, so the kit's own flow never sends initial settings there. An app that sends them itself gets Pi's defaults without an error. ACP says a client must only send them to an agent that advertises `session.options`, and Pi does not, so this is Pi's adapter within its rights. An addon could apply them after `session/new`; not written, since nothing needs it yet.

## Session list (A6)

- Sessions under their folder's name (or flat). A row shows the title, `needs you` when an approval or question waits, `running`, `open elsewhere`, `from <agent>` for a session only the harness knows, and how long ago it changed. An idle session has no badge.
- **Rename and delete only where the agent can.** Given the agent list, a row of an agent without `session.title` or `session.delete` in its capabilities gets no such control (Pi has neither).
- **Delete asks first**, in the row (React) or a confirmation dialog (SwiftUI), and says that the agent's own history loses the session too.
- Paging: `onMore` / `more` draws "Show more"; the app calls `sessionsPage` with its cursor.
- Keeping the list current stays the app's (`onSessionUpdated`, `onSessionRemoved`); React has `withSession` and `withoutSession` for it, the Swift client does it itself.
- Both demos now use the component.

## Tests

React: 5 new tests (38 in all), drawing to HTML. Swift: 2 new, including pictures of the bar and of the rows in `.build/archipel-ui-renders`. A SwiftUI `List` draws nothing off screen, so its rows are drawn stacked; the dialogs are not exercised. Neither kit has tests that click yet: that is step 5.

## Left of section A

A1 Markdown, A3 attachments, A4 commands and mentions, A5 long timelines, A7 diff, A8 terminal colours, A9 agent status, A10 connection status, A11 usage, A12 todos and background tasks, A13 needs-attention signal.

# Components of a session UI

What a component set for archipel has to draw, what each part takes and gives back, and the rules every set keeps. The React kit (`ui/react`), the SwiftUI kit (`ui/swift`) and the GPUI kit (`ui/gpui`) are three sets built to this. Start here to make another, or to redesign one.

A set may also take a theme: a look for the same components, kept in `ui/themes/<name>` with a version per kit (`react`, `swift`, `gpui`) and its own DESIGN.md. Nothing here is tied to a platform. Names are the React ones; the Swift and GPUI names are in [the last table](#names-in-the-kits). To see every component of the React kit in each of its states, run `pnpm catalog` in `ui/react` and open `demo/out/catalog.html`. The SwiftUI kit has the same page, with the same data and sections: `swift run ArchipelCatalog` from the repository's root writes `ui/swift/Catalog/out/catalog.html`, and `--live` opens it in a window. The GPUI kit has it as a window only: `cargo run -p archipel-gpui-catalog` in `ui/gpui`.

## Where the data comes from

A component draws what the client SDK holds. It never talks to the daemon itself.

- A **conversation** is a session and the sessions of its subagents (`conversation.sessions`, the first is the one the user talks to).
- A **session's state** has
  - `timeline`: references to items, in the order they first appeared, each `{ target, id }`.
  - `collections`: the items, by kind (`target`) and id. An item changes while it streams.
  - `values`: single values: `state` (the turn), `sessionInfo`, `usage`, `configOptions`, `availableCommands`.
  - `requests`: approvals and questions, by id. A request is also in the timeline, as target `request`.
- The **client** gives the list of sessions, the agents, whether it is connected, and calls `onAttention`.

What the user does goes out through **actions**: callbacks the app gives the components. A component that is not given an action does not draw its control.

## Rules every set keeps

1. **Draw from the current state.** After each change, draw the items as they are now. Do not append what an update says: an item's text grows, a tool call's status changes, a request gets its answer.
2. **Never drop a kind you do not know.** Draw it with the generic [Fallback](#fallback). Extensions and addons add kinds named `x.<namespace>.<name>`, and an app can give its own component for one.
3. **The agent's text is Markdown, and never HTML.** Build it as the platform's own elements. Markup the agent writes is shown as text. Links go only to `http`, `https` and `mailto`; anything else is plain text. Images in the agent's text are named, not fetched: fetching tells their server someone is reading. Text that is still arriving must draw: an open code fence is code up to where the text ends.
4. **The user's own text is shown as written**, not as Markdown.
5. **A picture is shown only from bytes the user sent.** Nothing is loaded from an address.
6. **Acting can fail.** Every control that acts says why when it fails, and puts back what the user had (a refused message goes back into the field, a refused setting goes back to the value in force).
7. **Ask before what cannot be undone.** Deleting a session removes it from the agent's own history too.
8. **Offer only what the agent can do.** Rename, delete and settings before a session depend on the agent's `capabilities` (`client.agents()`). A call the agent lacks fails with code -32601.
9. **The first answer counts.** Every attached app gets the same approvals and questions. When another app answers first, the request shows that answer.
10. **Long sessions stay quick.** Draw only what is on screen, or the newest items first with a way to see earlier ones. A session of 5000 items should open as fast as one of 200.
11. **Two themes.** Every component works in light and dark.

## How the parts fit

```
App
├── SessionList ─── SessionRow
├── AgentStatus, ConnectionStatus              (the app's own chrome)
└── ChatView                                   (one session)
    ├── StatusBar ─── Usage
    ├── Timeline                               (one row per timeline entry)
    │   ├── UserMessage
    │   ├── AssistantMessage ─── Markdown
    │   ├── Thought ─── Markdown
    │   ├── ToolGroup ─── ToolCall, Thought, Approval   (finished calls that followed one another)
    │   ├── ToolCall ─┬─ Terminal ─── AnsiText
    │   │             └─ Diff
    │   ├── Terminal                           (only when no tool call shows it)
    │   ├── Plan
    │   ├── Subagent ─── Timeline              (the subagent's own session, nested)
    │   ├── SessionMessage
    │   ├── Compaction
    │   ├── Approval, Question                 (requests)
    │   └── Fallback, or the app's own component for that kind
    ├── Activity                               (todos, background work)
    ├── OpenElsewhere
    ├── Session error
    ├── SettingsBar
    └── Composer ─┬─ command menu, mention menu
                  └─ attachments
```

## Shared parts

Small pieces most components use. Keep them consistent; they are most of a set's look.

| Part | What it is | Used by |
|---|---|---|
| **Badge** | A short word in a pill, coloured by tone: *ok* (`completed`, `idle`, `exit 0`), *bad* (`failed`, `cancelled`, other exits), *attention* (`needs you`), *running* (anything else: `pending`, `in progress`, `running`, `requires action`). `_` is shown as a space. | ToolCall (unless it completed), Terminal, Subagent, StatusBar, SessionRow |
| **Label** | A small uppercase word naming a part: `Plan`, `Permission`, `Question`, `Subagent`, `Between agents`, `Tasks`, a tool's kind. | Most cards, ToolCall |
| **Card** | A bordered, softly filled box. Highlighted (attention colour) while it waits for the user. | Plan, SessionMessage, Approval (while it waits), Question. A ToolCall is not a card: it is one quiet line (see [the weight of a turn's parts](#the-weight-of-a-turns-parts)). Nor is a Subagent: its timeline hangs from its row on a guide line, so its cards are never inside another card. |
| **Fold** | A one-line summary that opens to show more. Closed by default. | Thought, ToolGroup, Compaction, Fallback, Activity's tasks. Subagent is open by default. A ToolCall folds its output the same way, but opens by itself while it runs. |
| **Code block** | Monospaced text in a bordered box, wrapping long lines. | ToolCall output, Terminal, Fallback, Markdown code |
| **Chip** | A small rounded name. | Attachments, background work |
| **Meter** | A thin bar, full to a percentage. Attention colour from 80%. | Usage |
| **Notice** | A line across the view with a tone (*wait*, *act*, *bad*) and maybe one button. | AgentStatus, ConnectionStatus, OpenElsewhere |
| **Problem** | A line in the error colour, under the control that failed. | SettingsBar, Composer, SessionRow, OpenElsewhere |

### The weight of a turn's parts

What the agent **said** is what the user came for; what it **did and thought** on the way is there to be looked into. A set draws the two so that they cannot be mistaken:

- AssistantMessage is the strongest text of a turn: the primary colour, the largest size.
- ToolCall, ToolGroup, Thought and an Approval that has its answer are quiet: one line each, secondary colour, smaller, no card and no fill, with the same mark on those that open.
- A message's column, the user's or the agent's, is at most about 70 characters wide: 38.5 times the text's size, which counts a character at 0.55 em. Lines longer than that are hard to read, and a wide window otherwise stretches the two sides apart. The column holds prose only: a code block or a table in the agent's text may take the whole width of the chat, since code that wraps and a table squeezed into columns of a few words are harder to read than long lines of them.
- The chat itself (ChatView, every part of it) is at most about 150 characters wide, counted the same way (82.5 times the text's size), in the middle of a wider window.
- What a finished turn leaves on screen is the user's message, a few quiet lines, and the answer. Output is on screen only while it is being produced, or when the user opens it.

Colours a set needs, at least: text, secondary text, background, soft background (cards), raised and sunken surfaces (the user's messages and menus; code, diffs and fields), line (borders), accent (primary buttons, links, focus), ok, bad, running, and attention (waiting for the user, warnings). Terminal output needs 16 more (ANSI 0 to 15), in both themes.

## Item kinds

How each `target` of the timeline is drawn.

| Target | Component |
|---|---|
| `userMessage` | [UserMessage](#usermessage) |
| `agentMessage` | [AssistantMessage](#assistantmessage) |
| `agentThought` | [Thought](#thought) |
| `toolCall` | [ToolCall](#toolcall), inside a [ToolGroup](#toolgroup) when it is one of several finished calls in a row |
| `terminal` | [Terminal](#terminal), unless a tool call already shows it |
| `plan` | [Plan](#plan) |
| `subagent` | [Subagent](#subagent) |
| `sessionMessage` | [SessionMessage](#sessionmessage) |
| `compaction` | [Compaction](#compaction) |
| `request` | [Approval](#approval) when its method is `session/request_permission`, otherwise [Question](#question) |
| `cursor` | Nothing: it marks a place in the history |
| anything else | The app's component for that target if it gave one, otherwise [Fallback](#fallback) |

---

## The chat

### ChatView

**For:** one session, assembled: everything below in one view. An app that wants another layout assembles its own from the parts.

| In | |
|---|---|
| `conversation` | The session and its subagents' sessions |
| `sessionId` | Which session to draw; the default is the first |
| `actions` | `approve`, `answer`, `prompt`, `cancel`, `setOption`, `fork` (see [Actions](#actions)) |
| `components` | The app's components for kinds it knows, by target |
| `openElsewhere` | Set while another process has the session open: `{ pid, status }` |
| `error` | Something the daemon reported outside any request, such as a log it cannot write |
| `renderText` | Replaces the Markdown renderer for the agent's text |
| `mentions`, `accepts` | Passed to the Composer |

**Out:** nothing of its own; its parts call the actions.

**States:** no session (says so). Turn running: the Composer offers Stop.

**Width:** at most about 150 characters, centred (see [the weight of a turn's parts](#the-weight-of-a-turns-parts)).

**Order, top to bottom:** StatusBar, Timeline, Activity, OpenElsewhere, session error, SettingsBar (when `setOption` is given), Composer (when `prompt` is given). The timeline scrolls; the rest stays in place. A chat opens at its newest item.

### StatusBar

**For:** what the session is and where it stands.

| In | From |
|---|---|
| Title | `values.sessionInfo.title`; "New session" without one |
| Turn state | `values.state.state`: `idle`, `running`, `requires_action`, as a Badge |
| Usage | the session state, drawn by [Usage](#usage) |

### Timeline

**For:** the items of one session, in order. Also draws a subagent's session inside its item.

| In | |
|---|---|
| `conversation`, `state` | Where the items are; the conversation finds subagents' sessions |
| `actions`, `components` | Passed down to approvals, questions and custom kinds |
| `pageSize` | How many of the newest items to draw at first (React: 200) |

**Out:** "Show earlier" draws `pageSize` more. Counted from the end, so items that arrive do not push older ones out.

**Rules:** one row per timeline entry, each found in `collections` (or `requests`), except that finished tool calls in a row share one, a [ToolGroup](#toolgroup). Rows off screen should cost nothing to draw. A terminal that a tool call points at is drawn in the tool call, not again on its own.

**Running:** the Timeline tells each ToolCall whether it runs. A call runs when its status is `pending` or `in_progress` and either the turn is not `idle` or the call is in `values.state.background`. A call left `in_progress` by a turn that was cut short is over.

---

## Items

Each item component takes the item (JSON as the SDK holds it) and, where the user can act, a callback. Nothing else.

### UserMessage

**For:** what the user sent.

| In | |
|---|---|
| `item.content` | A string, or content blocks: `text`; `image` (`mimeType`, base64 `data`); `resource_link` (`uri`, `name`), a mention; `resource` (`resource.uri`, and `text` or `blob`), an attached file |

**Draws:** the text as written, aligned to the user's side, set apart from the agent's text by its fill, in a column of at most about 70 characters. A turn starts here: the Timeline leaves four times the rows' spacing above and below it (nothing more above the first row). Below it, a picture for each `image` (from its own bytes, only for an `image/*` type) and a chip with the name of each file or mention.

### AssistantMessage

**For:** what the agent said.

| In | |
|---|---|
| `item.content` | Text blocks; their text joined is Markdown |

**Draws:** the text through [Markdown](#markdown), or through the app's renderer if it gave one. On the agent's side, its prose in a column of at most about 70 characters, its code blocks and tables as wide as the chat lets them. The strongest text of the turn (see [the weight of a turn's parts](#the-weight-of-a-turns-parts)).

### Thought

**For:** the agent's reasoning.

| In | `item.content`, Markdown |
|---|---|

**Draws:** a Fold titled "Thinking", closed. Secondary colour, and the same quiet line as a ToolCall.

### ToolCall

**For:** something the agent did with a tool: read, edit, run, search, fetch.

| In | |
|---|---|
| `item.kind` | `read`, `edit`, `delete`, `move`, `search`, `execute`, `think`, `fetch`, `switch_mode`, `other` (as a Label; falls back to `item.name`) |
| `item.title` | What it did, such as a path or a command; monospaced. One line while the call is closed (the first, when a command is written on several); all of it, on as many lines as it takes, when open |
| `item.status` | `pending`, `in_progress`, `completed`, `failed` (also `cancelled`). A Badge, except `completed`: a call that went well says nothing more |
| `item.content` | A list of: `{ type: "content", content }` text output; `{ type: "diff", … }` a [Diff](#diff); `{ type: "terminal", terminalId }` a [Terminal](#terminal) |
| `terminal(id)` | Finds a terminal item by id, for the last kind |
| `running` | Whether the call still runs. From the Timeline; on its own, a ToolCall goes by its status |
| `chosen`, `onToggle` | What the user chose, open or closed, and the call that changes it. The Timeline keeps it by the call's id, because a call changes rows when it ends and joins a group; on its own, a ToolCall keeps it itself |

**Draws:** one quiet line with kind, title and status (not a Card), and under it, when open, each content block.

**States:**
- **Running:** open. The output is in view while it is produced, and so is the diff an approval asks about.
- **Over** (completed, failed, cancelled): closed. Only the line stays.
- **Toggled:** the line opens and closes the output, and says which it is to assistive technology. Once the user has pressed it, their choice stays: a call they opened does not close when it ends, and one they closed does not open again.
- **Open:** the title is shown whole, and the mark and the kind stay at its top.
- **Nothing to show** (no content, or only a terminal that is not found, and a title of one line): the line alone, and it is not a control.

**Related:** contains Terminal and Diff. Approvals often ask about a tool call just before it runs. Finished calls in a row are drawn in a [ToolGroup](#toolgroup).

### ToolGroup

**For:** several tool calls that are over and followed one another, as one line instead of one each.

| In | |
|---|---|
| `calls` | The tool call items, in order |
| children | The rows of the group: each call, and what came between two of them |

**Draws:** a Fold, closed: "N tool calls", and " · M failed" in the bad tone when some failed or were cancelled. It opens to its rows on a guide line, each call still closed and opened on its own.

**What is grouped** (the Timeline decides, from the current state, in order):
- A call that is over joins the group being built. A group needs two calls; one alone is drawn as a plain ToolCall.
- An Approval that has its answer goes into the group of the call it follows, whether or not another call comes after. A Thought goes in only when it lies **between** two calls; one that follows the last call stays outside, on its own row, and so does an answered approval behind it.
- Anything else ends the group: a message, a plan, a subagent, a question, an approval still waiting, an item of an unknown kind. So does a call that still runs, which is drawn on its own so that its output is seen. When it ends, it joins the calls before it.
- What draws nothing (a cursor, a terminal its tool call shows) is passed over and ends nothing.

A group is known by its first row, so one the user opened stays open while calls are added to it.

### Terminal

**For:** a command and its output.

| In | |
|---|---|
| `item.command` | Shown after `$`, one line |
| `item.output.data` | The output, base64 |
| `item.exitStatus` | Absent while running; `exitCode` once it ended |

**States:** `running`, `exit 0` (ok), `exit N` (bad), `ended` (no code). As a Badge.

**Draws:** the output through [AnsiText](#ansitext).

### Diff

**For:** a change to one or more files.

| In | |
|---|---|
| `block.patch.text` | A unified patch, as `git diff` prints it |
| `block.changes` | `[{ path, operation }]`: `add`, `modify`, `delete`, … |

**Draws, per file:** the operation and path, the count of added and removed lines, then the rows: hunk headers, unchanged lines, added and removed lines (tinted ok and bad), each with its old and new line numbers. More than three unchanged lines in a row, beyond three next to each change, fold behind "Show N unchanged lines", which opens in place. A patch that cannot be read is shown as text; with no patch, only the list of changes.

**Not done:** syntax highlighting. A set may add it.

### Plan

**For:** the agent's plan, or its todo list (they arrive the same way).

| In | |
|---|---|
| `item.entries` | `[{ content, status, priority }]`; status `pending`, `in_progress`, `completed` |
| `item.content` | A plan written as a document, when there are no entries |

**Draws:** a Card labelled Plan. Entries with a mark: ✓ done (struck through, secondary), … in progress, ○ pending.

**Related:** [Activity](#activity) shows the newest list with entries above the composer.

### Subagent

**For:** a subagent the agent started, and everything it did.

| In | |
|---|---|
| `item.title` | What it was asked to do |
| `item.state.state` | `running`, `idle` (done), as a Badge |
| children | Its own session, drawn by a nested [Timeline](#timeline) |

**Draws:** a Card, open by default, with the nested timeline indented under a line.

**Related:** its session usually starts and ends with a [SessionMessage](#sessionmessage).

### SessionMessage

**For:** a message between a session and its subagent: the task given, or the result returned.

| In | `item.content`; `item.senderSessionId`, `item.recipientSessionId` |
|---|---|

**Draws:** a Card labelled "Between agents".

### Compaction

**For:** the agent summarised the conversation to free context.

| In | `item.status`, `item.summary` |
|---|---|

**Draws:** a Fold: "Conversation compacted (status)", opening to the summary.

### Approval

**For:** the agent asks permission to do something. The turn waits.

| In | |
|---|---|
| `request.params.title` | What it wants to do |
| `request.params.options` | `[{ optionId, name, kind }]`; kinds `allow_once`, `allow_always`, `reject_once`, `reject_always` |
| `request.outcome` | `null` while waiting; then `{ outcome: { outcome: "selected", optionId } }` or `{ outcome: { outcome: "cancelled" } }` |

| Out | |
|---|---|
| `onChoose(optionId)` | → `actions.approve(requestId, optionId)` |

**States:** waiting (highlighted Card, one button per option, in the agent's order); answered (the chosen option's name); cancelled ("Cancelled"). The answer may come from another app.

**Draws, once it has its answer:** no longer a Card but one quiet line, like the [ToolCall](#toolcall) it was about: the label, the title on one line, the answer. It lines up with the tool calls' titles, and goes into a [ToolGroup](#toolgroup) with the call it follows.

### Question

**For:** the agent asks the user something. The turn waits.

| In | |
|---|---|
| `request.params.message` | The question |
| `request.params.requestedSchema.properties` | The fields, in order: `{ key: { title, type, oneOf: [{ const, title }] } }` |
| `request.outcome` | `null` while waiting; then `{ action: "accept", content }` or `{ action: "decline" }` |

| Out | |
|---|---|
| `onAnswer(content)` | The form's values by key → `actions.answer(requestId, content)` |
| `onAnswer(null)` | Skip → the SDK sends `decline` |

**Draws:** a form: a menu for a field with `oneOf`, a text field otherwise, each with its title. Answer (primary) and Skip. Once answered, the values given, or "No answer".

### Fallback

**For:** any kind there is no component for.

| In | `kind` (the target), `item` |
|---|---|

**Draws:** a Fold titled with the kind, opening to the item's data as formatted JSON.

### An app's own component

**For:** a kind an extension or addon adds, such as `x.acme.checkpoint`. The app gives a component per target; it receives `item` and draws it in the timeline row.

---

## Text

### Markdown

**For:** the agent's text, in AssistantMessage and Thought.

**In:** a string, possibly incomplete. **Draws:** paragraphs; headings (kept close to the text's size: this is a chat, not a document); bold, italic, strikethrough, inline code; ordered and unordered lists, nested; quotes; code blocks (no highlighting); tables; rules; links (safe schemes only); images as `[image: alt]`. See rule 3.

**Replaceable:** an app can give its own renderer for the agent's text, or show it as written.

### AnsiText

**For:** terminal output with the colours and styles a command prints.

**In:** a string with ANSI codes. **Draws:** the 16 colours (foreground and background), 256-colour and RGB codes, bold, dim, italic, underline, inverse. Other control codes are dropped; a line redrawn with carriage returns (a progress bar) shows its last state. It is not a terminal emulator.

---

## Around the timeline

### Usage

**For:** how full the context is, and what the session cost.

| In | From |
|---|---|
| `used`, `size` | `values.usage`: tokens in the context, and the window's size |
| `cost` | `values.usage.cost`: `{ amount, currency }`. Claude gives it; Pi does not |
| last turn's tokens | `values.state.usage.totalTokens` |

**Draws:** a Meter and "N% of context", then the cost (`$0.04`; below a cent, four decimals). The exact tokens on hover. Nothing when neither is known.

### Activity

**For:** what goes on beside the conversation, kept in view above the composer.

| In | From |
|---|---|
| Todos | The newest `plan` item with entries |
| Background work | `values.state.background`: `[{ terminalId } | { toolCallId } | { sessionId }]`, named by the command, the tool's title or the subagent's title |

**Draws:** a Fold "Tasks: N of M done · the one in progress", opening to the list (marks as in [Plan](#plan)); and "Running in the background" with a chip per piece of work. Nothing without either.

### OpenElsewhere

**For:** another process of the harness (its own terminal UI, say) has this session open, so the daemon refuses messages from here.

| In | |
|---|---|
| `holder` | `{ pid, status }`, both optional |
| `onFork` | Optional → `actions.fork()`; the app shows the copy once it resolves |

**States:** without fork (explains, nothing to press); with fork ("Fork and continue here"); forking ("Forking…", disabled); fork failed ("Could not fork: reason").

**Related:** the Composer does not repeat this when a message is refused for this reason. SessionRow shows "open elsewhere".

### Session error

**For:** something the daemon reported outside any request, such as a log it cannot write (code -32005). Set until updates arrive again.

**In:** the message. **Draws:** a Problem line above the composer.

### SettingsBar

**For:** the agent's settings: model, effort, permission mode, switches. Drawn from what the agent reports; a set knows no model or mode by name.

| In | |
|---|---|
| `settings` | From `values.configOptions` (or, before a session exists, `client.sessionOptions()`). Each `{ id, name, description, category, kind: choice or switch, current, groups: [{ name?, choices: [{ value, name, description }] }] }`. `category` (`model`, `thought_level`, `mode`) is a hint for styling |
| `disabled` | |

| Out | |
|---|---|
| `onChange(id, value)` | → `actions.setOption(id, value)`, or kept for the new session |

**Draws:** a menu per choice (with group headings when the agent groups them), a checkbox per switch, the description on hover.

**States:** read only (no `onChange`); changing (shows the new value, controls locked until the agent answers); refused ("Not changed: name: reason", and the control goes back). A kind of setting the set does not know is left out.

### Composer

**For:** where the user writes to the agent.

| In | |
|---|---|
| `running` | A turn is running: the placeholder says "Add to the running turn…" and Stop is offered |
| `commands` | From `values.availableCommands`: `[{ name, description, input.hint }]` |
| `mentions(query)` | The app's function that finds files to mention; the kit has no file list of its own |
| `accepts` | `{ image, files }` from the agent's capabilities: what it takes besides text |
| `maxBytes` | Largest file to attach (5 MB) |

| Out | |
|---|---|
| `onSend(content)` | → `actions.prompt`. The text alone when there is nothing else; otherwise content blocks: `text`, a `resource_link` per mention still in the text, an `image` per picture, a `resource` per other file (with its text, or base64 `blob`) |
| `onCancel()` | Stop → `actions.cancel()` |

**Parts and states:**
- **Field.** Enter sends, Shift+Enter breaks the line. Sending clears it.
- **Command menu.** Typing `/` at the start lists the commands, those starting with what was typed first, then those containing it. Arrows move, Enter or Tab picks (writes `/name `), Escape closes.
- **Mention menu.** Typing `@` at the start of a word lists what `mentions` returns. Picking writes `@name `.
- **Attach.** With `accepts`: a button, and files can be pasted and dropped. Pictures only if the agent takes pictures, other files only if it takes files.
- **Attachments.** A chip per file (a thumbnail for a picture), each removable.
- **Problem.** A file refused (wrong kind, too large, unreadable), or "Not sent: reason". A refused message goes back into the field, unless the user has typed since.

**Related:** `prompt` resolves when the agent has taken the message, not when the turn is over; the turn is over when the turn state is `idle`.

---

## The app's chrome

These are not part of a chat. An app places them where it wants.

### SessionList

**For:** the sessions to choose from.

| In | |
|---|---|
| `sessions` | `client.listSessions()`, kept current as the daemon says sessions appear, change and go. Each `{ sessionId, agentId, title, cwd, live, logged, openElsewhere, state, updatedAt }` |
| `selected` | The session that is open |
| `agents` | `client.agents()`: hides rename and delete where the agent cannot |
| `groupBy` | Under each folder's name (the default), or one flat list |
| `now` | The time "5m ago" counts from |

| Out | |
|---|---|
| `onOpen(id)` | |
| `onRename(id, title)` | → `client.setTitle` |
| `onDelete(id)` | → `client.deleteSession` (needs the `admin` scope) |
| `onMore()` | With `client.sessionsPage`: "Show more" |

**States:** empty ("No sessions"); has more. Rows as below.

### SessionRow

**For:** one session in the list.

**Draws:** the title ("Untitled" without one); then what needs the user first: `needs you` (attention; an approval or question waits), `running`, `open elsewhere`; the agent, or `from <agent>` for a session started outside the app (opening it loads its history); how long ago it changed (`now`, `5m`, `3h`, `2d`, then the date).

**States:** shown; selected; renaming (a field with Save and Cancel; Escape cancels); confirming delete ("Delete “title”? It is removed from agent's own history too. This cannot be undone." with Delete and Cancel); busy; failed ("Not renamed: …", "Not deleted: …").

How rename and delete are reached is the platform's: buttons on hover in React, the row's context menu and dialogs in SwiftUI.

### AgentStatus

**For:** which agents cannot be used, and why.

| In | `agents`: `client.agents()`, each `{ agentId, name, readiness, detail }` |
|---|---|
| **Out** | `onRetry()`: ask again |

| readiness | Tone | Says |
|---|---|---|
| `starting` | wait | "Name is starting…" |
| `needs_login` | act | "Name is not signed in. Sign in with Name's own program, then try again." |
| `not_installed` | bad | "Name is not installed on the daemon's machine (detail)." |
| `error` | bad | "Name could not be started (detail)." |
| `ready`, `not_started` | | Nothing: nothing is wrong as far as anyone knows |

A Notice per agent with a problem, with "Try again" except while starting. Nothing when all is well.

### ConnectionStatus

**For:** the connection to the daemon is lost. **In:** `connected`. **Draws:** a wait Notice, "Connection lost. Reconnecting… What you see may be out of date." Nothing while connected. The SDK reconnects and catches up on its own.

### Attention

Not a component: the client calls `onAttention(session, why)` once when any session, open or not, starts to wait for an answer (`needs_answer`) or its turn ends (`finished`). A notification, a badge or a sound is the app's.

---

## Actions

What a chat can do, as the app gives it. Each is optional; without it, its control is not drawn.

| Action | Called by | Does |
|---|---|---|
| `approve(requestId, optionId)` | Approval | Answers a permission request |
| `answer(requestId, content \| null)` | Question | Answers a question, or declines it |
| `prompt(content)` | Composer | Sends a message; may fail (refused, open elsewhere, storage) |
| `cancel()` | Composer | Stops the running turn |
| `setOption(id, value)` | SettingsBar | Changes a setting; may fail |
| `fork()` | OpenElsewhere | Continues on a copy of the session |

## Names in the kits

| Here | React | SwiftUI | GPUI |
|---|---|---|---|
| ChatView | `ChatView` (+ `useSession`) | `ChatView`, `LiveChatView` | `ChatView` (an entity), `live_chat` |
| StatusBar | `StatusBar` | `StatusBar` | `status::status_bar` |
| Timeline | `Timeline` | `SessionTimeline` | `timeline`, `timeline_row` |
| UserMessage … Fallback | `UserMessage`, `AssistantMessage`, `Thought`, `ToolCall`, `ToolGroup`, `Terminal`, `Diff`, `Plan`, `Subagent`, `SessionMessage`, `Compaction`, `Approval`, `Question`, `Fallback` | the same with `View` after (`DiffView` for Diff) | functions of `items`, in snake case (`items::tool_call`); `diff::diff` for Diff |
| Markdown | `Markdown` | `MarkdownText` | `markdown::markdown` |
| AnsiText | `AnsiText` | `AnsiText` | `parts::ansi_text` |
| Usage | `Usage` | `UsageView` | `status::usage` |
| Activity | `Activity` | `ActivityView` | `status::activity` |
| OpenElsewhere | `OpenElsewhere` | `OpenElsewhereNotice` | `status::open_elsewhere` |
| SettingsBar | `SettingsBar` | `SettingsBar` | `settings_bar` |
| Composer | `Composer` | `Composer` | `Composer` (an entity) |
| SessionList, SessionRow | `SessionList`, `SessionRow` | `SessionList`, `SessionRow` | `SessionList` (an entity), `session_row` |
| AgentStatus, ConnectionStatus | `AgentStatus`, `ConnectionStatus` | `AgentStatus`, `ConnectionStatus` | `status::agent_status`, `status::connection_status` |

The kits have the same components and say the same things, but do not look alike without a theme: React has its own colour variables, SwiftUI uses the system's, GPUI has React's colours and draws every control itself; the session list's actions and the composer's menus are drawn differently. The React kit is the base the others follow.

## Checking a new set

- Draw every file of `testdata/states/`: they are real recorded sessions, the same the SDKs are tested against. `ui/react/demo/render.tsx` does it for React.
- Drive a session end to end against the fake agent, with no model and no account (see `AGENTS.md`, "Check your work without spending money").
- Compare the catalogues (`pnpm catalog` in `ui/react`, `swift run ArchipelCatalog` at the root, `cargo run -p archipel-gpui-catalog` in `ui/gpui`) for the states a recording does not reach: menus open, a row being renamed, a refused setting.

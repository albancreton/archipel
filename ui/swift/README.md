# SwiftUI component kit

`ArchipelUI`: one view per kind of item, and a chat view assembled from them. Optional.

```swift
LiveChatView(session: session)                       // a ArchipelSession; draws and acts on it
LiveChatView(session: session) { self.session = $0 } // also offers the fork, and hands you the copy
ChatView(conversation: conversation, actions: actions) // from plain state, with your own closures
```

`SettingsBar(settings: Setting.all(from: state.values["configOptions"]), change: …)` draws the agent's settings (model, effort, permission mode, switches) from what the agent reports; `LiveChatView` shows it above the composer. A change the agent refuses is shown under the bar. Before a session exists, read them with `client.sessionOptions` and pass the choices to `client.newSession(cwd:agentId:configOptions:)`; this needs an agent with `session.options` (Claude has it, Pi does not).

```swift
SessionList(
    sessions: client.sessions, selection: $selected, agents: agents,   // agents: try await client.agents()
    rename: { id, title in _ = try await client.setTitle(id, to: title) },
    delete: { try await client.deleteSession($0) })
```

`SessionList` groups sessions under their folder's name. A row says `needs you`, `running`, `open elsewhere`, and `from <agent>` for a session started outside the app. Rename and delete are in the row's context menu, only where the agent can do them; delete asks first, because the agent removes the session from its own history too. `SessionRow` and `SessionSummary.byFolder` are public for a list of your own.

- **The agent's text is drawn as Markdown** (`MarkdownText`, on Foundation's own parser, no dependency): headings, lists, tables, quotes, code blocks. Links go only to `http`, `https` and `mailto`. An open code fence in text that is still arriving is code. There is no code highlighting. `.agentText { AnyView(Text($0)) }` on any view above swaps the renderer.
- **Long sessions:** the timeline is a `LazyVStack`, so inside the chat's scroll view only the rows on screen are built.
- **Composer**: typing `/` lists the agent's commands; with `mentions:` (a function of yours that finds files) typing `@` lists them; with `accepts: Accepts(agent:)` it offers "Attach" and takes dropped files. Arrows, Tab, Return and Escape work in the lists. A mention is found only at the end of the text, since a SwiftUI field does not say where the caret is. `ChatActions.prompt` receives content blocks; `ArchipelSession.prompt` takes them.
- **Diffs** (`DiffView`) are drawn per file with line numbers, counts, and long unchanged parts folded. **Terminal output** (`AnsiText`, used by `TerminalView`) shows the colours a command prints. Neither highlights syntax.
- **Status:** `AgentStatus(agents:)` says which agents cannot be used and why; `ConnectionStatus(connected: client.connected)` says when the connection is lost; `UsageView` (in the status bar) shows the context and the cost when the agent gives one; `ActivityView` (above the composer) keeps background work and the agent's todo list in view. `client.onAttention` is called once when a session starts to wait for an answer or its turn ends; showing a notification is yours.
- Each view takes an item and, where the user can act, a closure.
- When another process of the harness has the session open (its own terminal UI, say), the daemon refuses messages for it. The chat says so above the composer; with `onFork` it offers "Fork and continue here". A refused message goes back into the field.
- `custom: ["x.acme.checkpoint": { AnyView(CheckpointView(item: $0)) }]` draws an extension's kind.
- `Demo/` is a small macOS app on both packages: `swift run ArchipelChatDemo`.

`swift test` draws recorded sessions off screen and writes the pictures to `.build/archipel-ui-renders`.

It also uses the views as a person would (`Tests/ArchipelUITests/ClickTests.swift`): buttons are found by their label and pressed, and text is typed into fields. The helper `Screen.swift` does this inside the test process through accessibility, in a window that is never shown, so it needs no permission. It cannot drive menus (pickers, a row's menu), alerts, the file dialog or drags.

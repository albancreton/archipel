# SwiftUI component kit

`BridgeUI`: one view per kind of item, and a chat view assembled from them. Optional.

```swift
LiveChatView(session: session)                       // a BridgeSession; draws and acts on it
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

- Each view takes an item and, where the user can act, a closure.
- When another process of the harness has the session open (its own terminal UI, say), the daemon refuses messages for it. The chat says so above the composer; with `onFork` it offers "Fork and continue here". A refused message goes back into the field.
- `custom: ["x.acme.checkpoint": { AnyView(CheckpointView(item: $0)) }]` draws an extension's kind.
- `Demo/` is a small macOS app on both packages: `swift run BridgeChatDemo`.

`swift test` draws recorded sessions off screen and writes the pictures to `.build/bridge-ui-renders`.

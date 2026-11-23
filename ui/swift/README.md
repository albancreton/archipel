# SwiftUI component kit

`BridgeUI`: one view per kind of item, and a chat view assembled from them. Optional.

```swift
LiveChatView(session: session)                       // a BridgeSession; draws and acts on it
LiveChatView(session: session) { self.session = $0 } // also offers the fork, and hands you the copy
ChatView(conversation: conversation, actions: actions) // from plain state, with your own closures
```

- Each view takes an item and, where the user can act, a closure.
- When another process of the harness has the session open (its own terminal UI, say), the daemon refuses messages for it. The chat says so above the composer; with `onFork` it offers "Fork and continue here". A refused message goes back into the field.
- `custom: ["x.acme.checkpoint": { AnyView(CheckpointView(item: $0)) }]` draws an extension's kind.
- `Demo/` is a small macOS app on both packages: `swift run BridgeChatDemo`.

`swift test` draws recorded sessions off screen and writes the pictures to `.build/bridge-ui-renders`.

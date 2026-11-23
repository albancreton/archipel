# SwiftUI component kit

`BridgeUI`: one view per kind of item, and a chat view assembled from them. Optional.

```swift
LiveChatView(session: session)                       // a BridgeSession; draws and acts on it
ChatView(conversation: conversation, actions: actions) // from plain state, with your own closures
```

- Each view takes an item and, where the user can act, a closure.
- `custom: ["x.acme.checkpoint": { AnyView(CheckpointView(item: $0)) }]` draws an extension's kind.
- `Demo/` is a small macOS app on both packages: `swift run BridgeChatDemo`.

`swift test` draws recorded sessions off screen and writes the pictures to `.build/bridge-ui-renders`.

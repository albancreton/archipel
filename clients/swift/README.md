# Swift client SDK

`ArchipelClient`, a Swift package (the `Package.swift` is at the repository root). For macOS and iOS.

```swift
let client = ArchipelClient(name: "MyApp") {
    try UnixSocketTransport(path: NSHomeDirectory() + "/.archipel/daemon.sock")
    // or: WebSocketTransport(url: URL(string: "ws://host:7890")!, token: token)
}
try await client.connect()
let sessions = try await client.refreshSessions()
let session = try await client.session(sessions[0].id)
try await session.prompt("Run the tests")
```

`ArchipelClient` and `ArchipelSession` are observable, so SwiftUI views that read them update by themselves.

| File | What |
|---|---|
| `State.swift` | The reducer: applies ACP v2 messages by following the state rules. A port of the Rust core |
| `Mirror.swift` | What a client of the daemon holds, and the daemon's additions |
| `Client.swift` | `ArchipelClient` and `ArchipelSession`: attach, reconnect, prompt, answer |
| `Transport.swift` | The daemon's WebSocket and its local socket |
| `Resources/state-rules.json` | The state rules, written by `just gen` |

`swift test` checks the reducer against `testdata/states` and runs the client against the real daemon (`cargo build` first).

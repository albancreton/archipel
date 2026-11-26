# Swift client SDK

`ArchipelClient`, a Swift package (the `Package.swift` is at the repository root). For macOS and iOS.

```swift
let client = ArchipelClient(name: "MyApp") {
    try UnixSocketTransport(path: NSHomeDirectory() + "/.archipel/daemon.sock")
    // on another machine: CommandTransport(command: "ssh", arguments: ["myhost", "archipel", "connect", "--stdio"])
}
try await client.connect()
let sessions = try await client.refreshSessions()
let session = try await client.session(sessions[0].id)
try await session.prompt("Run the tests")
```

A daemon on another machine is reached through SSH, which does the encryption: `CommandTransport` runs `ssh` on macOS; on iOS, `StreamTransport` takes the output of a channel the app opened with its own SSH library, and a way to write to it (`docs/remote.md`).

`ArchipelClient` and `ArchipelSession` are observable, so SwiftUI views that read them update by themselves.

| File | What |
|---|---|
| `State.swift` | The reducer: applies ACP v2 messages by following the state rules. A port of the Rust core |
| `Mirror.swift` | What a client of the daemon holds, and the daemon's additions |
| `Client.swift` | `ArchipelClient` and `ArchipelSession`: attach, reconnect, prompt, answer |
| `Transport.swift` | The daemon's local socket, a command that carries the connection (SSH), bytes the app moves itself (an SSH library on iOS), and the WebSocket |
| `Resources/state-rules.json` | The state rules, written by `just gen` |

`swift test` checks the reducer against `testdata/states` and runs the client against the real daemon (`cargo build` first).

import BridgeClient
import BridgeUI
import SwiftUI

// A small macOS app on the two Swift packages: the session list on the left, a chat on the right.
// It talks to the daemon on this machine through its local socket.
//
//   swift run BridgeChatDemo [path of the daemon's home]     (default: ~/.harness-bridge)

@main
struct BridgeChatDemo: App {
    @State private var client: BridgeClient
    @State private var session: BridgeSession?
    @State private var failure: String?

    init() {
        // Started from a terminal there is no app bundle: without this the window gets no keyboard.
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)
        let home = CommandLine.arguments.dropFirst().first ?? NSHomeDirectory() + "/.harness-bridge"
        _client = State(initialValue: BridgeClient(name: "BridgeChatDemo") {
            try UnixSocketTransport(path: home + "/daemon.sock")
        })
    }

    var body: some Scene {
        WindowGroup("Bridge") {
            NavigationSplitView {
                List(client.sessions, selection: Binding(get: { session?.id }, set: { open($0) })) { summary in
                    VStack(alignment: .leading) {
                        Text(summary.title ?? "Untitled").lineLimit(1)
                        Text("\(summary.agentId) · \(summary.state ?? (summary.logged ? "" : "in the harness only"))\(summary.openElsewhere ? " · open elsewhere" : "")")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                .overlay { if !client.connected { ContentUnavailableView("Not connected", systemImage: "bolt.slash", description: Text(failure ?? "Waiting for the daemon…")) } }
            } detail: {
                if let session {
                    LiveChatView(session: session) { self.session = $0 }.id(session.id)
                } else {
                    Text("Choose a session").foregroundStyle(.secondary)
                }
            }
            .task {
                do {
                    try await client.connect()
                    try await client.refreshSessions()
                } catch {
                    failure = String(describing: error)
                }
            }
        }
    }

    private func open(_ id: String?) {
        guard let id else { return }
        Task {
            do {
                session = try await client.session(id)
            } catch {
                failure = String(describing: error)
            }
        }
    }
}

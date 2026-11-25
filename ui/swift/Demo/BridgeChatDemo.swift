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
    @State private var agents: [JSON] = []
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
                VStack(spacing: 6) {
                    ConnectionStatus(connected: client.connected)
                    AgentStatus(agents: agents) { Task { try? await client.refreshSessions(); agents = (try? await client.agents()) ?? agents } }
                }
                .padding(.horizontal, 8)
                SessionList(
                    sessions: client.sessions, selection: Binding(get: { session?.id }, set: { open($0) }), agents: agents,
                    rename: { id, title in _ = try await client.setTitle(id, to: title) },
                    delete: { try await client.deleteSession($0) })
                .overlay { if !client.connected { ContentUnavailableView("Not connected", systemImage: "bolt.slash", description: Text(failure ?? "Waiting for the daemon…")) } }
            } detail: {
                if let session, !session.removed {
                    let agent = client.sessions.first { $0.id == session.id }.flatMap { summary in agents.first { $0["agentId"]?.string == summary.agentId } }
                    LiveChatView(session: session, accepts: Accepts(agent: agent)) { self.session = $0 }.id(session.id)
                } else {
                    Text("Choose a session").foregroundStyle(.secondary)
                }
            }
            .task {
                do {
                    try await client.connect()
                    try await client.refreshSessions()
                    // Listing started the agents, so they now say what they can do.
                    agents = try await client.agents()
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

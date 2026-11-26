import ArchipelClient
import ArchipelMinitel
import ArchipelUI
import SwiftUI

// A small macOS app on the two Swift packages: the session list on the left, a chat on the right.
// It talks to the daemon on this machine through its local socket.
//
//   swift run ArchipelChatDemo [path of the daemon's home]     (default: ~/.archipel)
//   swift run ArchipelChatDemo --theme minitel [home]           (the Minitel theme, ui/themes/minitel)

@main
struct ArchipelChatDemo: App {
    @State private var client: ArchipelClient
    @State private var session: ArchipelSession?
    @State private var agents: [JSON] = []
    @State private var failure: String?
    private let theme: ArchipelTheme

    init() {
        // Started from a terminal there is no app bundle: without this the window gets no keyboard.
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)
        var arguments = Array(CommandLine.arguments.dropFirst())
        if let at = arguments.firstIndex(of: "--theme"), at + 1 < arguments.count {
            theme = arguments[at + 1] == "minitel" ? .minitel() : .standard
            arguments.removeSubrange(at...(at + 1))
        } else {
            theme = .standard
        }
        let home = arguments.first ?? NSHomeDirectory() + "/.archipel"
        _client = State(initialValue: ArchipelClient(name: "ArchipelChatDemo") {
            try UnixSocketTransport(path: home + "/daemon.sock")
        })
    }

    var body: some Scene {
        WindowGroup("Archipel") {
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
            .archipelTheme(theme)
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

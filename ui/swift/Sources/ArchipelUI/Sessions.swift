import ArchipelClient
import SwiftUI

extension SessionSummary {
    /// The sessions by working folder, in the order the folders first appear.
    public static func byFolder(_ sessions: [SessionSummary]) -> [(cwd: String, sessions: [SessionSummary])] {
        var order: [String] = []
        var folders: [String: [SessionSummary]] = [:]
        for session in sessions {
            if folders[session.cwd] == nil { order.append(session.cwd) }
            folders[session.cwd, default: []].append(session)
        }
        return order.map { ($0, folders[$0] ?? []) }
    }

    /// How long ago the session changed, in a few characters: `now`, `5m`, `3h`, `2d`, then the date.
    public func ago(now: Date = Date()) -> String {
        guard let updatedAt, let then = Self.time(updatedAt) else { return "" }
        let minutes = Int(now.timeIntervalSince(then) / 60)
        switch minutes {
        case ..<1: return "now"
        case ..<60: return "\(minutes)m"
        case ..<(60 * 24): return "\(minutes / 60)h"
        case ..<(60 * 24 * 30): return "\(minutes / (60 * 24))d"
        default: return String(updatedAt.prefix(10))
        }
    }

    private static func time(_ text: String) -> Date? {
        let plain = ISO8601DateFormatter()
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions.insert(.withFractionalSeconds)
        return plain.date(from: text) ?? fractional.date(from: text)
    }

    /// What a row says about the session's turn: only what is worth a look.
    var attention: (text: String, tone: Tone)? {
        switch state {
        case "requires_action": ("needs you", .attention)
        case "running": ("running", .running)
        default: nil
        }
    }
}

/// Sessions to choose from: grouped by folder, with each session's state, and rename and delete
/// in the row's menu. Assemble your own from `SessionRow` and `SessionSummary.byFolder` if it
/// does not fit.
public struct SessionList: View {
    let sessions: [SessionSummary]
    @Binding var selection: String?
    /// `client.agents()`: an agent that does not say it can rename or delete gets no such menu
    /// item. An agent says so once it has been started. With nil, every session gets them.
    var agents: [JSON]?
    /// Rename a session (`client.setTitle`).
    var rename: ((String, String) async throws -> Void)?
    /// Delete a session (`client.deleteSession`). The agent removes it from its own history
    /// too and nobody can undo that, so the list asks first. The daemon wants the `admin` scope.
    var delete: ((String) async throws -> Void)?
    /// There are more sessions (`nextCursor` of `client.sessionsPage`): draws "Show more".
    var more: (() async -> Void)?
    var grouped: Bool
    @State private var renaming: SessionSummary?
    @State private var draft = ""
    @State private var deleting: SessionSummary?
    @State private var problem: String?
    @Environment(\.archipelTheme) private var theme

    public init(
        sessions: [SessionSummary], selection: Binding<String?>, agents: [JSON]? = nil,
        rename: ((String, String) async throws -> Void)? = nil, delete: ((String) async throws -> Void)? = nil,
        more: (() async -> Void)? = nil, grouped: Bool = true
    ) {
        self.sessions = sessions
        _selection = selection
        self.agents = agents
        self.rename = rename
        self.delete = delete
        self.more = more
        self.grouped = grouped
    }

    /// Whether the session's agent said it can do this (`title` or `delete`).
    func can(_ session: SessionSummary, _ what: String) -> Bool {
        guard let agents else { return true }
        let agent = agents.first { $0["agentId"]?.string == session.agentId }
        return agent?.at("capabilities.session.\(what)") != nil
    }

    public var body: some View {
        List(selection: $selection) {
            if grouped {
                ForEach(SessionSummary.byFolder(sessions), id: \.cwd) { folder in
                    Section {
                        ForEach(folder.sessions) { row($0) }
                    } header: {
                        let name = (folder.cwd as NSString).lastPathComponent
                        Group { if theme.folderLabels { Label2(text: name) } else { Text(name) } }.help(folder.cwd)
                    }
                }
            } else {
                ForEach(sessions) { row($0) }
            }
            if let more {
                Button("Show more") { Task { await more() } }
                    .buttonStyle(.plain).foregroundStyle(theme.colors.accent).underline().font(theme.fonts.secondary)
            }
        }
        .scrollContentBackground(theme.colors.background == nil ? .automatic : .hidden)
        .background(theme.colors.background ?? .clear)
        .overlay { if sessions.isEmpty { Text("No sessions").foregroundStyle(theme.colors.secondary) } }
        .alert("Rename session", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } }), presenting: renaming) { session in
            TextField("Title", text: $draft)
            Button("Save") {
                let title = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !title.isEmpty, title != session.title else { return }
                run("Not renamed") { try await rename?(session.id, title) }
            }
            Button("Cancel", role: .cancel) {}
        }
        .confirmationDialog(
            "Delete “\(deleting?.title ?? "Untitled")”?",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), presenting: deleting
        ) { session in
            Button("Delete", role: .destructive) { run("Not deleted") { try await delete?(session.id) } }
            Button("Cancel", role: .cancel) {}
        } message: { session in
            Text("It is removed from \(session.agentId)’s own history too. This cannot be undone.")
        }
        .alert("Something went wrong", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(problem ?? "")
        }
    }

    @ViewBuilder
    private func row(_ session: SessionSummary) -> some View {
        let canRename = rename != nil && can(session, "title")
        let canDelete = delete != nil && can(session, "delete")
        SessionRow(session: session)
            .tag(session.id)
            .contextMenu {
                if canRename {
                    Button("Rename…") {
                        draft = session.title ?? ""
                        renaming = session
                    }
                }
                if canDelete { Button("Delete…", role: .destructive) { deleting = session } }
            }
    }

    private func run(_ what: String, _ work: @escaping () async throws -> Void) {
        Task {
            do { try await work() } catch { problem = "\(what): \(describe(error))" }
        }
    }
}

/// One session: its title, what it is doing, and where else it lives.
public struct SessionRow: View {
    let session: SessionSummary
    var now: Date
    @Environment(\.archipelTheme) private var theme

    public init(session: SessionSummary, now: Date = Date()) {
        self.session = session
        self.now = now
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            // A list sets its own font on macOS: the theme's is set again here.
            Text(session.title ?? "Untitled").font(theme.fonts.body).lineLimit(1)
            HStack(spacing: 6) {
                if let attention = session.attention { Badge(text: attention.text, tone: attention.tone) }
                if session.openElsewhere {
                    Badge(text: "open elsewhere", tone: .attention)
                        .help("Another window of the agent has this session open. Fork it to continue here.")
                }
                if session.logged {
                    Text(session.agentId).lineLimit(1)
                } else {
                    Text("from \(session.agentId)").lineLimit(1).help("Started outside this app. Opening it loads its history from the agent.")
                }
                Spacer(minLength: 0)
                Text(session.ago(now: now))
            }
            .font(theme.fonts.meta)
            .foregroundStyle(theme.colors.secondary)
        }
        .foregroundStyle(theme.colors.text)
        .padding(.vertical, 2)
    }
}

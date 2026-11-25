import ArchipelClient
import SwiftUI

/// What the user can do. Leave one out and its control does nothing.
public struct ChatActions {
    /// Answer an approval with one of its options: request id, option id.
    public var approve: (String, String) -> Void = { _, _ in }
    /// Answer a question: request id, and the form's content or nil to skip it.
    public var answer: (String, JSON?) -> Void = { _, _ in }
    /// Send a message, as content blocks: its text, and what the user attached or mentioned.
    /// If it throws, the composer gives everything back.
    public var prompt: (([JSON]) async throws -> Void)?
    public var cancel: () -> Void = {}
    /// Change a setting of the agent, such as the model. With it the chat draws the settings bar.
    public var setOption: ((String, Setting.Value) async throws -> Void)?
    /// Continue on a copy of the session, when another process has this one open. The app
    /// shows the copy once this returns; if it throws, the notice says why.
    public var fork: (() async throws -> Void)?
    public init() {}
}

/// Says that another process has the session open, so messages from here are refused, and
/// offers to continue on a copy. Without `fork` it only says so.
public struct OpenElsewhereNotice: View {
    let holder: OpenElsewhere
    var fork: (() async throws -> Void)?
    @State private var forking = false
    @State private var problem: String?

    public init(holder: OpenElsewhere, fork: (() async throws -> Void)? = nil) {
        self.holder = holder
        self.fork = fork
    }

    public var body: some View {
        let who = [holder.pid > 0 ? "pid \(holder.pid)" : nil, holder.status].compactMap { $0 }.joined(separator: ", ")
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text("This session is open in another window of the agent\(who.isEmpty ? "" : " (\(who))").").fontWeight(.semibold)
                Text("Messages from here are refused so the two do not overwrite each other. Close it there and send again\(fork == nil ? "." : ", or continue here on a copy.")")
                    .foregroundStyle(.secondary)
                if let problem { Text("Could not fork: \(problem)").foregroundStyle(.red) }
            }
            .font(.callout)
            .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            if let fork {
                Button(forking ? "Forking…" : "Fork and continue here") {
                    forking = true
                    problem = nil
                    Task {
                        do { try await fork() } catch { problem = String(describing: error) }
                        forking = false
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(forking)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.orange.opacity(0.12)))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.orange.opacity(0.6)))
    }
}

/// Views for kinds of item that this package does not know, by target, e.g. `x.acme.checkpoint`.
public typealias ItemViews = [String: (JSON) -> AnyView]

/// The items of one session, in order. A subagent's session is drawn inside its item.
public struct SessionTimeline: View {
    let conversation: Conversation
    let state: SessionState
    var actions = ChatActions()
    var custom: ItemViews = [:]

    public init(conversation: Conversation, state: SessionState, actions: ChatActions = ChatActions(), custom: ItemViews = [:]) {
        self.conversation = conversation
        self.state = state
        self.actions = actions
        self.custom = custom
    }

    public var body: some View {
        // A terminal that a tool call shows is not drawn a second time.
        let inline = Set((state.collections["toolCall"] ?? [:]).values.flatMap { tool in
            (tool["content"]?.array ?? []).compactMap { $0["terminalId"]?.string }
        })
        // Lazy: inside a scroll view only the rows on screen are built, so a long session
        // opens as quickly as a short one.
        LazyVStack(alignment: .leading, spacing: 10) {
            ForEach(state.timeline, id: \.self) { ref in
                row(ref, inline: inline)
            }
        }
    }

    @ViewBuilder
    private func row(_ ref: ItemRef, inline: Set<String>) -> some View {
        if ref.target == SessionState.requestTarget {
            if let request = state.requests[ref.id] {
                if request.method == "session/request_permission" {
                    ApprovalView(request: request) { actions.approve(ref.id, $0) }
                } else {
                    QuestionView(request: request) { actions.answer(ref.id, $0) }
                }
            }
        } else if let item = state.item(ref) {
            if let view = custom[ref.target] {
                view(item)
            } else {
                switch ref.target {
                case "userMessage": UserMessageView(item: item)
                case "agentMessage": AssistantMessageView(item: item)
                case "agentThought": ThoughtView(item: item)
                case "toolCall": ToolCallView(item: item) { state.collections["terminal"]?[$0] }
                case "terminal": if !inline.contains(ref.id) { TerminalView(item: item) }
                case "plan": PlanView(item: item)
                case "sessionMessage": SessionMessageView(item: item)
                case "compaction": CompactionView(item: item)
                case "subagent":
                    SubagentView(item: item) {
                        if let child = conversation.sessions[ref.id] {
                            // AnyView: the view contains itself, which Swift cannot express as one type.
                            AnyView(SessionTimeline(conversation: conversation, state: child, actions: actions, custom: custom))
                        }
                    }
                // A cursor marks a place in the history; it is not content.
                case "cursor": EmptyView()
                default: FallbackView(kind: ref.target, item: item)
                }
            }
        }
    }
}

/// The session's title, turn state and context use.
public struct StatusBar: View {
    let state: SessionState
    public init(state: SessionState) { self.state = state }
    public var body: some View {
        HStack {
            Text(state.values["sessionInfo"]?["title"]?.string ?? "New session").font(.headline).lineLimit(1)
            Spacer()
            UsageView(state: state)
            if let turn = state.values["state"]?["state"]?.string { Badge(text: turn) }
        }
    }
}

/// A simple chat: status, the items, and a composer. Assemble your own from the parts if it does not fit.
public struct ChatView: View {
    let conversation: Conversation
    var sessionId: String?
    var actions = ChatActions()
    var custom: ItemViews = [:]
    /// Set while another process has the session open: the chat says so and offers `actions.fork`.
    var openElsewhere: OpenElsewhere?
    /// Something the daemon reported outside any request, such as a log it cannot write.
    var error: String?
    /// Finds files to mention for what the user typed after `@`. See `Composer`.
    var mentions: ((String) async -> [Mention])?
    /// What the agent takes besides text (`Accepts(agent:)` of its entry in `client.agents()`).
    var accepts = Accepts()

    public init(
        conversation: Conversation, sessionId: String? = nil, actions: ChatActions = ChatActions(), custom: ItemViews = [:],
        openElsewhere: OpenElsewhere? = nil, error: String? = nil, mentions: ((String) async -> [Mention])? = nil,
        accepts: Accepts = Accepts()
    ) {
        self.conversation = conversation
        self.sessionId = sessionId
        self.actions = actions
        self.custom = custom
        self.openElsewhere = openElsewhere
        self.error = error
        self.mentions = mentions
        self.accepts = accepts
    }

    public var body: some View {
        if let state = (sessionId ?? conversation.rootId).flatMap({ conversation.sessions[$0] }) {
            let running = (state.values["state"]?["state"]?.string).map { $0 != "idle" } ?? false
            VStack(spacing: 0) {
                StatusBar(state: state).padding(12)
                Divider()
                ScrollView {
                    SessionTimeline(conversation: conversation, state: state, actions: actions, custom: custom).padding(12)
                }
                .defaultScrollAnchor(.bottom)
                ActivityView(state: state).padding([.horizontal, .bottom], 12)
                if let openElsewhere {
                    OpenElsewhereNotice(holder: openElsewhere, fork: actions.fork).padding([.horizontal, .bottom], 12)
                }
                if let error {
                    Text(error).font(.callout).foregroundStyle(.red)
                        .frame(maxWidth: .infinity, alignment: .leading).padding([.horizontal, .bottom], 12)
                }
                if let setOption = actions.setOption {
                    SettingsBar(settings: Setting.all(from: state.values["configOptions"]), change: setOption)
                        .padding([.horizontal, .bottom], 12)
                }
                if let prompt = actions.prompt {
                    Divider()
                    Composer(
                        running: running, send: prompt, cancel: actions.cancel,
                        commands: Command.all(from: state.values["availableCommands"]), mentions: mentions, accepts: accepts)
                        .padding(12)
                }
            }
        } else {
            Text("No session").foregroundStyle(.secondary)
        }
    }
}

/// A chat on a session of the client SDK: it draws again after every change, and its controls
/// act on the session. Give `onFork` to offer the fork when another process has the session
/// open: it receives the copy to show.
public struct LiveChatView: View {
    let session: ArchipelSession
    var custom: ItemViews = [:]
    var onFork: ((ArchipelSession) -> Void)?
    var mentions: ((String) async -> [Mention])?
    var accepts: Accepts

    public init(
        session: ArchipelSession, custom: ItemViews = [:], mentions: ((String) async -> [Mention])? = nil, accepts: Accepts = Accepts(),
        onFork: ((ArchipelSession) -> Void)? = nil
    ) {
        self.session = session
        self.custom = custom
        self.mentions = mentions
        self.accepts = accepts
        self.onFork = onFork
    }

    public var body: some View {
        var actions = ChatActions()
        actions.approve = { session.approve($0, option: $1) }
        actions.answer = { id, content in
            session.answer(id, result: content.map { ["action": "accept", "content": $0] } ?? ["action": "decline"])
        }
        actions.prompt = { try await session.prompt($0) }
        actions.cancel = { session.cancel() }
        actions.setOption = { id, value in
            switch value {
            case .choice(let choice): try await session.setConfigOption(id, value: choice)
            case .on(let on): try await session.setConfigOption(id, on: on)
            }
        }
        if let onFork { actions.fork = { onFork(try await session.fork()) } }
        return ChatView(
            conversation: session.conversation, sessionId: session.id, actions: actions, custom: custom,
            openElsewhere: session.openElsewhere, error: session.error?.message, mentions: mentions, accepts: accepts)
    }
}

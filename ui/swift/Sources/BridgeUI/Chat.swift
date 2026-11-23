import BridgeClient
import SwiftUI

/// What the user can do. Leave one out and its control does nothing.
public struct ChatActions {
    /// Answer an approval with one of its options: request id, option id.
    public var approve: (String, String) -> Void = { _, _ in }
    /// Answer a question: request id, and the form's content or nil to skip it.
    public var answer: (String, JSON?) -> Void = { _, _ in }
    public var prompt: ((String) -> Void)?
    public var cancel: () -> Void = {}
    public init() {}
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
        VStack(alignment: .leading, spacing: 10) {
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
            if let used = state.values["usage"]?["used"]?.number, let size = state.values["usage"]?["size"]?.number, size > 0 {
                Text("\(Int((used / size * 100).rounded()))% of context").font(.caption).foregroundStyle(.secondary)
            }
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
    @State private var draft = ""

    public init(conversation: Conversation, sessionId: String? = nil, actions: ChatActions = ChatActions(), custom: ItemViews = [:]) {
        self.conversation = conversation
        self.sessionId = sessionId
        self.actions = actions
        self.custom = custom
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
                if let prompt = actions.prompt {
                    Divider()
                    HStack(alignment: .bottom) {
                        TextField(running ? "Add to the running turn…" : "Message the agent…", text: $draft, axis: .vertical)
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(1...6)
                            .onSubmit { send(prompt) }
                        Button("Send") { send(prompt) }.buttonStyle(.borderedProminent)
                        if running { Button("Stop", action: actions.cancel) }
                    }
                    .padding(12)
                }
            }
        } else {
            Text("No session").foregroundStyle(.secondary)
        }
    }

    private func send(_ prompt: (String) -> Void) {
        guard !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        prompt(draft)
        draft = ""
    }
}

/// A chat on a session of the client SDK: it draws again after every change, and its controls
/// act on the session.
public struct LiveChatView: View {
    let session: BridgeSession
    var custom: ItemViews = [:]

    public init(session: BridgeSession, custom: ItemViews = [:]) {
        self.session = session
        self.custom = custom
    }

    public var body: some View {
        var actions = ChatActions()
        actions.approve = { session.approve($0, option: $1) }
        actions.answer = { id, content in
            session.answer(id, result: content.map { ["action": "accept", "content": $0] } ?? ["action": "decline"])
        }
        actions.prompt = { text in Task { try? await session.prompt(text) } }
        actions.cancel = { session.cancel() }
        return ChatView(conversation: session.conversation, sessionId: session.id, actions: actions, custom: custom)
    }
}

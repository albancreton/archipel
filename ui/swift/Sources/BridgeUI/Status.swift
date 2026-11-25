import BridgeClient
import SwiftUI

/// What to tell the user about an agent that cannot be used as it is.
public struct AgentProblem: Equatable, Identifiable {
    public enum Tone: Equatable { case wait, act, bad }
    public var id: String
    public var tone: Tone
    public var text: String

    /// From one entry of `client.agents()`. Nil when the agent is ready, or was not started
    /// yet: then nothing is wrong as far as anyone knows.
    public init?(agent: JSON) {
        let name = agent["name"]?.string ?? agent["agentId"]?.string ?? "The agent"
        let detail = agent["detail"]?.string.map { " (\($0))" } ?? ""
        switch agent["readiness"]?.string {
        case "starting": (tone, text) = (.wait, "\(name) is starting…")
        case "needs_login": (tone, text) = (.act, "\(name) is not signed in. Sign in with \(name)'s own program, then try again.")
        case "not_installed": (tone, text) = (.bad, "\(name) is not installed on the daemon's machine\(detail).")
        case "error": (tone, text) = (.bad, "\(name) could not be started\(detail).")
        default: return nil
        }
        id = agent["agentId"]?.string ?? name
    }
}

/// A line that says something is not as it should be, with an optional button.
struct Notice: View {
    let text: String
    var tone: AgentProblem.Tone = .wait
    var action: (title: String, run: () -> Void)?

    var body: some View {
        let color: Color = tone == .bad ? .red : tone == .act ? .orange : .secondary
        HStack(spacing: 12) {
            Text(text).font(.callout).foregroundStyle(tone == .wait ? .secondary : .primary)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            if let action { Button(action.title, action: action.run) }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 10).fill(color.opacity(0.1)))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(color.opacity(0.5)))
    }
}

/// Says which agents cannot be used and why: not signed in, not installed, failed to start, or
/// still starting. Draws nothing when all is well. `agents` is `client.agents()`; ask again
/// after a session was created or listed, since an agent is only known once it was started.
public struct AgentStatus: View {
    let agents: [JSON]
    var retry: (() -> Void)?

    public init(agents: [JSON], retry: (() -> Void)? = nil) {
        self.agents = agents
        self.retry = retry
    }

    public var body: some View {
        VStack(spacing: 6) {
            ForEach(agents.compactMap(AgentProblem.init(agent:))) { problem in
                Notice(text: problem.text, tone: problem.tone, action: problem.tone == .wait ? nil : retry.map { ("Try again", $0) })
            }
        }
    }
}

/// Says that the connection to the daemon is lost and being retried. Draws nothing while
/// connected. `connected` is `client.connected`.
public struct ConnectionStatus: View {
    let connected: Bool
    public init(connected: Bool) { self.connected = connected }

    public var body: some View {
        if !connected { Notice(text: "Connection lost. Reconnecting… What you see may be out of date.") }
    }
}

/// What the session has used, as far as the agent says.
public struct UsageSummary: Equatable {
    public var used: Int?
    public var size: Int?
    public var percent: Int?
    /// Such as `$0.0031`. Nil when the agent gives no cost.
    public var cost: String?
    public var turnTokens: Int?

    public init(state: SessionState) {
        let usage = state.values["usage"]
        used = usage?["used"]?.number.map(Int.init)
        size = usage?["size"]?.number.flatMap { $0 > 0 ? Int($0) : nil }
        if let used, let size { percent = Int((Double(used) / Double(size) * 100).rounded()) }
        if let amount = usage?.at("cost.amount")?.number {
            let currency = usage?.at("cost.currency")?.string ?? ""
            // Small amounts keep more digits: a turn often costs less than a cent.
            let number = String(format: amount < 0.01 ? "%.4f" : "%.2f", amount)
            cost = currency == "USD" ? "$" + number : currency.isEmpty ? number : "\(number) \(currency)"
        }
        turnTokens = state.values["state"]?.at("usage.totalTokens")?.number.map(Int.init)
    }
}

/// How full the context is, and what the session has cost when the agent says.
public struct UsageView: View {
    let state: SessionState
    public init(state: SessionState) { self.state = state }

    public var body: some View {
        let usage = UsageSummary(state: state)
        HStack(spacing: 6) {
            if let percent = usage.percent {
                Capsule().fill(Color.secondary.opacity(0.25)).frame(width: 44, height: 5)
                    .overlay(alignment: .leading) {
                        Capsule().fill(percent >= 80 ? Color.orange : Color.secondary).frame(width: 44 * CGFloat(min(percent, 100)) / 100)
                    }
                    .accessibilityLabel("Context used").accessibilityValue("\(percent) percent")
                Text("\(percent)% of context")
            }
            if let cost = usage.cost { Text(cost) }
        }
        .font(.caption).foregroundStyle(.secondary)
        .help([
            usage.used.flatMap { used in usage.size.map { "\(used) of \($0) tokens of context" } },
            usage.turnTokens.map { "\($0) tokens in the last turn" },
        ].compactMap { $0 }.joined(separator: " · "))
    }
}

/// One thing that still runs in the background after a turn ended.
public struct BackgroundWork: Equatable, Identifiable {
    /// `command`, `tool` or `subagent`.
    public var kind: String
    public var id: String
    public var label: String

    /// What the session says is running in the background, with each one's command or title.
    public static func all(in state: SessionState) -> [BackgroundWork] {
        func named(_ target: String, _ id: String, _ key: String) -> String {
            state.collections[target]?[id]?[key]?.string.flatMap { $0.isEmpty ? nil : $0 } ?? id
        }
        return (state.values["state"]?["background"]?.array ?? []).compactMap { work in
            if let id = work["terminalId"]?.string { return BackgroundWork(kind: "command", id: id, label: named("terminal", id, "command")) }
            if let id = work["toolCallId"]?.string { return BackgroundWork(kind: "tool", id: id, label: named("toolCall", id, "title")) }
            if let id = work["sessionId"]?.string { return BackgroundWork(kind: "subagent", id: id, label: named("subagent", id, "title")) }
            return nil
        }
    }
}

/// The newest todo list the agent keeps (a plan with entries), or nil.
public func todos(in state: SessionState) -> [(content: String, status: String)]? {
    for ref in state.timeline.reversed() where ref.target == "plan" {
        if let entries = state.item(ref)?["entries"]?.array, !entries.isEmpty {
            return entries.map { ($0["content"]?.string ?? "", $0["status"]?.string ?? "pending") }
        }
    }
    return nil
}

/// What goes on beside the conversation, kept in view above the composer: the agent's todo
/// list with its progress, and what still runs in the background. Draws nothing without either.
public struct ActivityView: View {
    let state: SessionState
    @State private var open = false
    public init(state: SessionState) { self.state = state }

    public var body: some View {
        let list = todos(in: state)
        let background = BackgroundWork.all(in: state)
        if list != nil || !background.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                if let list {
                    let now = list.first { $0.status == "in_progress" }?.content
                    DisclosureGroup(isExpanded: $open) {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(list.enumerated()), id: \.offset) { _, todo in
                                HStack(alignment: .firstTextBaseline, spacing: 6) {
                                    Text(todo.status == "completed" ? "✓" : todo.status == "in_progress" ? "…" : "○").frame(width: 14)
                                    Text(todo.content).strikethrough(todo.status == "completed")
                                        .foregroundStyle(todo.status == "completed" ? .secondary : .primary)
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    } label: {
                        Text("Tasks: \(list.filter { $0.status == "completed" }.count) of \(list.count) done\(now.map { " · \($0)" } ?? "")").lineLimit(1)
                    }
                }
                if !background.isEmpty {
                    HStack(spacing: 6) {
                        Text("Running in the background").foregroundStyle(.secondary)
                        ForEach(background) { work in
                            Text(work.label).font(.system(.caption, design: .monospaced)).lineLimit(1)
                                .padding(.horizontal, 8).padding(.vertical, 2)
                                .background(Color.secondary.opacity(0.14), in: RoundedRectangle(cornerRadius: 8))
                                .help(work.kind)
                        }
                        Spacer(minLength: 0)
                    }
                }
            }
            .font(.callout)
            .padding(.horizontal, 10).padding(.vertical, 8)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.secondary.opacity(0.08)))
        }
    }
}

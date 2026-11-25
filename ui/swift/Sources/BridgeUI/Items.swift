import BridgeClient
import SwiftUI

// One view per kind of item. Each takes the item and, where the user can act, a closure.
// Nothing here knows about connections; give the views state and they draw it.

/// The text of a content field: a list of blocks, one block, or a string.
public func contentText(_ content: JSON?) -> String {
    switch content {
    case .string(let text): return text
    case .array(let blocks): return blocks.map { contentText($0) }.joined()
    case .object:
        switch content?["type"]?.string {
        case "text": return content?["text"]?.string ?? ""
        case "content": return contentText(content?["content"])
        case let other: return "[\(other ?? "content")]"
        }
    default: return ""
    }
}

struct Badge: View {
    let text: String
    /// Set to choose the colour; otherwise it follows the text.
    var tint: Color?
    var body: some View {
        let color: Color = if let tint { tint } else { switch text {
        case "completed", "idle", "exit 0": .green
        case "failed", "cancelled": .red
        default: text.hasPrefix("exit") ? .red : .orange
        } }
        Text(text.replacingOccurrences(of: "_", with: " "))
            .font(.caption2)
            .padding(.horizontal, 7).padding(.vertical, 1)
            .foregroundStyle(color)
            .overlay(Capsule().stroke(color, lineWidth: 1))
    }
}

struct Card<Content: View>: View {
    var highlighted = false
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 6) { content }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(highlighted ? Color.orange : Color.secondary.opacity(0.25)))
    }
}

struct Label2: View {
    let text: String
    var body: some View {
        Text(text.uppercased()).font(.caption2).foregroundStyle(.secondary)
    }
}

struct CodeBlock: View {
    let text: String
    /// Terminal output, whose colour codes are drawn as colours.
    var terminal = false
    var body: some View {
        Text(terminal ? AnsiText(text).attributed : AttributedString(text))
            .font(.system(.caption, design: .monospaced))
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(8)
            .background(.background, in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(.secondary.opacity(0.25)))
    }
}

public struct UserMessageView: View {
    let item: JSON
    public init(item: JSON) { self.item = item }
    public var body: some View {
        let blocks = item["content"]?.array ?? item["content"].map { [$0] } ?? []
        let extras = blocks.filter { $0["type"]?.string != "text" && $0.string == nil }
        HStack {
            Spacer(minLength: 60)
            VStack(alignment: .leading, spacing: 6) {
                let text = blocks.filter { $0["type"]?.string == "text" || $0.string != nil }.map { contentText($0) }.joined()
                if !text.isEmpty { Text(text) }
                // What was attached or mentioned: a picture from its own bytes, anything else by name.
                ForEach(Array(extras.enumerated()), id: \.offset) { _, block in
                    if block["type"]?.string == "image", let picture = block["data"]?.string.flatMap({ Data(base64Encoded: $0) }).flatMap(Image.init(data:)) {
                        picture.resizable().scaledToFit().frame(maxWidth: 220, maxHeight: 160).clipShape(RoundedRectangle(cornerRadius: 8))
                    } else {
                        let uri = (block["resource"] ?? block)["uri"]?.string
                        Text(block["name"]?.string ?? uri.flatMap { URL(string: $0)?.lastPathComponent } ?? block["type"]?.string ?? "attachment")
                            .font(.caption).padding(.horizontal, 8).padding(.vertical, 2)
                            .background(.white.opacity(0.2), in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .foregroundStyle(.white)
            .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14))
        }
    }
}

public struct AssistantMessageView: View {
    let item: JSON
    @Environment(\.agentText) private var render
    public init(item: JSON) { self.item = item }
    public var body: some View {
        render(contentText(item["content"])).frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Reasoning, folded by default.
public struct ThoughtView: View {
    let item: JSON
    @State private var open = false
    @Environment(\.agentText) private var render
    public init(item: JSON) { self.item = item }
    public var body: some View {
        DisclosureGroup("Thinking", isExpanded: $open) {
            render(contentText(item["content"]).trimmingCharacters(in: .whitespacesAndNewlines))
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(.callout).foregroundStyle(.secondary)
    }
}

/// The output of a command, with the command line and how it ended.
public struct TerminalView: View {
    let item: JSON
    public init(item: JSON) { self.item = item }
    public var body: some View {
        let output = (item.at("output.data")?.string).flatMap { Data(base64Encoded: $0) }.map { String(decoding: $0, as: UTF8.self) } ?? ""
        let ended: String = {
            guard let exit = item["exitStatus"], exit.object != nil else { return "running" }
            return exit["exitCode"]?.number.map { "exit \(Int($0))" } ?? "ended"
        }()
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("$ " + (item["command"]?.string ?? "")).font(.system(.callout, design: .monospaced)).lineLimit(1)
                Spacer()
                Badge(text: ended)
            }
            if !output.isEmpty { CodeBlock(text: output.trimmingCharacters(in: .newlines), terminal: true) }
        }
    }
}

/// A tool call: its title and status, and what it produced. `terminal` finds a terminal by id,
/// for calls whose content points at one.
public struct ToolCallView: View {
    let item: JSON
    var terminal: (String) -> JSON? = { _ in nil }
    public init(item: JSON, terminal: @escaping (String) -> JSON? = { _ in nil }) {
        self.item = item
        self.terminal = terminal
    }
    public var body: some View {
        Card {
            HStack {
                Label2(text: item["kind"]?.string ?? item["name"]?.string ?? "tool")
                Text(item["title"]?.string ?? item["name"]?.string ?? "").font(.system(.callout, design: .monospaced)).lineLimit(1)
                Spacer()
                Badge(text: item["status"]?.string ?? "pending")
            }
            ForEach(Array((item["content"]?.array ?? []).enumerated()), id: \.offset) { _, block in
                switch block["type"]?.string {
                case "terminal":
                    if let found = block["terminalId"]?.string.flatMap(terminal) { TerminalView(item: found) }
                case "diff":
                    DiffView(block: block)
                default:
                    let body = contentText(block)
                    if !body.isEmpty { CodeBlock(text: body) }
                }
            }
        }
    }
}

/// A plan: a document, or a list of entries with their status (which is also how todos arrive).
public struct PlanView: View {
    let item: JSON
    public init(item: JSON) { self.item = item }
    public var body: some View {
        Card {
            Label2(text: "Plan")
            let entries = item["entries"]?.array ?? []
            if entries.isEmpty {
                Text(contentText(item["content"])).font(.callout)
            } else {
                ForEach(Array(entries.enumerated()), id: \.offset) { _, entry in
                    let status = entry["status"]?.string
                    HStack(alignment: .firstTextBaseline) {
                        Text(status == "completed" ? "✓" : status == "in_progress" ? "…" : "○").frame(width: 16)
                        Text(entry["content"]?.string ?? "").strikethrough(status == "completed")
                    }
                    .foregroundStyle(status == "completed" ? .secondary : .primary)
                }
            }
        }
    }
}

/// A subagent: its title and state, and its own session drawn inside.
public struct SubagentView<Inner: View>: View {
    let item: JSON
    @ViewBuilder var inner: Inner
    public init(item: JSON, @ViewBuilder inner: () -> Inner) {
        self.item = item
        self.inner = inner()
    }
    public var body: some View {
        Card {
            HStack {
                Label2(text: "Subagent")
                Text(item["title"]?.string ?? "")
                Spacer()
                Badge(text: item.at("state.state")?.string ?? "")
            }
            inner.padding(.leading, 10).overlay(alignment: .leading) { Rectangle().fill(.secondary.opacity(0.3)).frame(width: 2) }
        }
    }
}

/// A message between a session and its subagent: the task given, or the result returned.
public struct SessionMessageView: View {
    let item: JSON
    public init(item: JSON) { self.item = item }
    public var body: some View {
        Card {
            Label2(text: "Between agents")
            Text(contentText(item["content"])).font(.callout)
        }
    }
}

public struct CompactionView: View {
    let item: JSON
    @State private var open = false
    public init(item: JSON) { self.item = item }
    public var body: some View {
        DisclosureGroup("Conversation compacted (\(item["status"]?.string ?? ""))", isExpanded: $open) {
            Text(contentText(item["summary"])).frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(.callout).foregroundStyle(.secondary)
    }
}

/// An approval: the question and its options. Once answered it shows which option was taken.
public struct ApprovalView: View {
    let request: Request
    var choose: (String) -> Void = { _ in }
    public init(request: Request, choose: @escaping (String) -> Void = { _ in }) {
        self.request = request
        self.choose = choose
    }
    public var body: some View {
        let options = request.params["options"]?.array ?? []
        Card(highlighted: request.outcome == nil) {
            Label2(text: "Permission")
            Text(request.params["title"]?.string ?? "").bold()
            if let outcome = request.outcome {
                let chosen = outcome.at("outcome.optionId")?.string
                let name = options.first { $0["optionId"]?.string == chosen }?["name"]?.string
                Text(outcome.at("outcome.outcome")?.string == "cancelled" ? "Cancelled" : (name ?? chosen ?? ""))
                    .foregroundStyle(.secondary)
            } else {
                HStack {
                    ForEach(Array(options.enumerated()), id: \.offset) { _, option in
                        Button(option["name"]?.string ?? "") { choose(option["optionId"]?.string ?? "") }
                    }
                }
            }
        }
    }
}

/// A question from the agent: a small form built from the schema it sent.
public struct QuestionView: View {
    let request: Request
    /// Called with the form's content, or nil when the user skips the question.
    var answer: (JSON?) -> Void = { _ in }
    @State private var values: [String: String] = [:]
    public init(request: Request, answer: @escaping (JSON?) -> Void = { _ in }) {
        self.request = request
        self.answer = answer
    }
    public var body: some View {
        let fields = (request.params.at("requestedSchema.properties")?.object ?? [:]).sorted { $0.key < $1.key }
        Card(highlighted: request.outcome == nil) {
            Label2(text: "Question")
            Text(request.params["message"]?.string ?? "").bold()
            if let outcome = request.outcome {
                let given = (outcome["content"]?.object ?? [:]).sorted { $0.key < $1.key }.compactMap { $0.value.string }
                Text(outcome["action"]?.string == "accept" ? given.joined(separator: ", ") : "No answer").foregroundStyle(.secondary)
            } else {
                ForEach(fields, id: \.key) { key, field in
                    let choices = field["oneOf"]?.array ?? []
                    let binding = Binding(get: { values[key] ?? "" }, set: { values[key] = $0 })
                    if choices.isEmpty {
                        TextField(field["title"]?.string ?? key, text: binding).textFieldStyle(.roundedBorder)
                    } else {
                        Picker(field["title"]?.string ?? key, selection: binding) {
                            Text("").tag("")
                            ForEach(Array(choices.enumerated()), id: \.offset) { _, choice in
                                Text(choice["title"]?.string ?? choice["const"]?.string ?? "").tag(choice["const"]?.string ?? "")
                            }
                        }
                    }
                }
                HStack {
                    Button("Answer") { answer(.object(values.filter { !$0.value.isEmpty }.mapValues { .string($0) })) }
                        .buttonStyle(.borderedProminent)
                    Button("Skip") { answer(nil) }
                }
            }
        }
    }
}

/// Any item there is no view for, such as an extension's: its kind and its data.
public struct FallbackView: View {
    let kind: String
    let item: JSON
    @State private var open = false
    public init(kind: String, item: JSON) {
        self.kind = kind
        self.item = item
    }
    public var body: some View {
        DisclosureGroup(kind, isExpanded: $open) { CodeBlock(text: item.text) }
            .font(.callout).foregroundStyle(.secondary)
    }
}

extension Image {
    /// A picture from its file's bytes, or nil when they are not a picture.
    init?(data: Data) {
        #if canImport(AppKit)
        guard let image = NSImage(data: data) else { return nil }
        self.init(nsImage: image)
        #else
        guard let image = UIImage(data: data) else { return nil }
        self.init(uiImage: image)
        #endif
    }
}

import ArchipelClient
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
    var tone: Tone?
    @Environment(\.archipelTheme) private var theme
    @Environment(\.self) private var environment
    var body: some View {
        let color = theme.color(tone ?? Tone(status: text))
        let words = Text(text.replacingOccurrences(of: "_", with: " ")).font(theme.fonts.meta).lineLimit(1)
        switch theme.badge {
        case .outlined:
            words
                .padding(.horizontal, 7).padding(.vertical, 1)
                .foregroundStyle(color)
                .overlay(Capsule().stroke(color, lineWidth: 1))
                .fixedSize()
        case .tinted:
            HStack(spacing: 6) {
                Circle().fill(color).frame(width: 7, height: 7)
                // Text between the tone and the text colour, so it stays readable on the pale fill.
                words.foregroundStyle(mix(color, theme.colors.text, 0.7))
            }
            .padding(.horizontal, 10).padding(.vertical, 2)
            .background(color.opacity(0.12), in: Capsule())
            .fixedSize()
        }
    }

    private func mix(_ one: Color, _ other: Color, _ amount: Double) -> Color {
        let a = one.resolve(in: environment), b = other.resolve(in: environment)
        func between(_ x: Float, _ y: Float) -> Double { Double(x) * amount + Double(y) * (1 - amount) }
        return Color(.sRGB, red: between(a.red, b.red), green: between(a.green, b.green), blue: between(a.blue, b.blue))
    }
}

struct Card<Content: View>: View {
    /// Waiting for the user: the border takes the attention colour.
    var highlighted = false
    @ViewBuilder var content: Content
    @Environment(\.archipelTheme) private var theme
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: theme.cardRadius)
        VStack(alignment: .leading, spacing: 6) { content }
            .padding(theme.cardPadding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.colors.card, in: shape)
            .background(highlighted ? theme.colors.attention.opacity(theme.waitingFill) : .clear, in: shape)
            .overlay(shape.stroke(highlighted ? theme.colors.attention : theme.colors.line, lineWidth: highlighted ? theme.waitingBorder : 1))
    }
}

/// The uppercase word that names a part.
struct Label2: View {
    let text: String
    @Environment(\.archipelTheme) private var theme
    var body: some View { Self.text(text, theme) }

    /// The same as text, to lead a line of other text.
    static func text(_ text: String, _ theme: ArchipelTheme) -> Text {
        Text(text.uppercased()).font(theme.fonts.label).tracking(theme.fonts.labelTracking).foregroundColor(theme.colors.secondary)
    }
}

/// A label, then a title on the same line; a long title wraps after the label.
struct LabelledTitle: View {
    let label: String
    let title: String
    var font: Font?
    @Environment(\.archipelTheme) private var theme
    var body: some View {
        (Label2.text(label, theme) + Text("  ") + Text(title).font(font ?? theme.fonts.requestTitle))
            .fixedSize(horizontal: false, vertical: true)
    }
}

struct CodeBlock: View {
    let text: String
    /// Terminal output, whose colour codes are drawn as colours.
    var terminal = false
    @Environment(\.archipelTheme) private var theme
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: theme.controlRadius)
        Text(terminal ? AnsiText(text).attributed(palette: theme.colors.ansi) : AttributedString(text))
            .font(theme.fonts.code)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(8)
            .background(theme.colors.sunken, in: shape)
            .overlay(shape.stroke(theme.colors.line))
    }
}

/// The quiet line of what the agent did or thought: a chevron and a few words, in the secondary
/// colour, that opens what is under it. Without `toggle` it is not a control and has no chevron.
struct FoldLine<Words: View>: View {
    let open: Bool
    /// What assistive technology reads for the line.
    let label: String
    /// The words take several lines: the chevron and what comes first stay at their top.
    var tall = false
    var toggle: (() -> Void)?
    @ViewBuilder var words: Words
    @Environment(\.archipelTheme) private var theme
    var body: some View {
        let line = HStack(alignment: tall ? .firstTextBaseline : .center, spacing: 8) {
            Image(systemName: "chevron.right").font(.system(size: 9, weight: .semibold))
                .rotationEffect(.degrees(open ? 90 : 0))
                .frame(width: 10)
                .opacity(toggle == nil ? 0 : 1)
                .accessibilityHidden(true)
            words
            Spacer(minLength: 0)
        }
        .font(theme.fonts.secondary).foregroundStyle(theme.colors.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        if let toggle {
            Button(action: toggle) { line }
                .buttonStyle(.plain)
                .accessibilityLabel(label)
                .accessibilityValue(open ? "expanded" : "collapsed")
        } else {
            line
        }
    }
}

/// What a fold line opened: under it, hanging from a guide line.
struct FoldBody<Content: View>: View {
    var spacing: CGFloat = 6
    @ViewBuilder var content: Content
    @Environment(\.archipelTheme) private var theme
    var body: some View {
        VStack(alignment: .leading, spacing: spacing) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.leading, 13)
            .overlay(alignment: .leading) { Rectangle().fill(theme.colors.line).frame(width: 1) }
            .padding(.leading, 4)
    }
}

/// The mark before a plan entry or a todo: a tick when done, a dot while in progress, a ring before.
struct PlanMark: View {
    let status: String
    @Environment(\.archipelTheme) private var theme
    var body: some View {
        Group {
            switch status {
            case "completed": Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(theme.colors.ok)
            case "in_progress": Image(systemName: "circle.fill").font(.system(size: 7)).foregroundStyle(theme.colors.running)
            default: Image(systemName: "circle").font(.system(size: 9, weight: .medium)).foregroundStyle(theme.colors.secondary)
            }
        }
        .frame(width: 16)
        .accessibilityLabel(status.replacingOccurrences(of: "_", with: " "))
    }
}

public struct UserMessageView: View {
    let item: JSON
    @Environment(\.archipelTheme) private var theme
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
                            .font(theme.fonts.meta).padding(.horizontal, 8).padding(.vertical, 2)
                            .background(theme.colors.onMessage.opacity(0.2), in: RoundedRectangle(cornerRadius: 8))
                    }
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .foregroundStyle(theme.colors.onMessage)
            .background(theme.colors.message, in: RoundedRectangle(cornerRadius: 14))
        }
    }
}

public struct AssistantMessageView: View {
    let item: JSON
    @Environment(\.agentText) private var render
    public init(item: JSON) { self.item = item }
    @Environment(\.archipelTheme) private var theme
    public var body: some View {
        // The strongest text of a turn: the body font in the text colour. What the agent did
        // and thought is drawn smaller and in the secondary colour.
        render(contentText(item["content"])).frame(maxWidth: .infinity, alignment: .leading)
            .font(theme.fonts.body).foregroundStyle(theme.colors.text)
    }
}

/// Reasoning, folded by default: the same quiet line as a tool call.
public struct ThoughtView: View {
    let item: JSON
    @State private var open = false
    @Environment(\.agentText) private var render
    @Environment(\.archipelTheme) private var theme
    /// `open`: shown unfolded at first.
    public init(item: JSON, open: Bool = false) {
        self.item = item
        _open = State(initialValue: open)
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            FoldLine(open: open, label: "Thinking", toggle: { open.toggle() }) { Text("Thinking") }
            if open {
                FoldBody {
                    render(contentText(item["content"]).trimmingCharacters(in: .whitespacesAndNewlines))
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .font(theme.fonts.secondary).foregroundStyle(theme.colors.secondary)
            }
        }
    }
}

/// The output of a command, with the command line and how it ended.
public struct TerminalView: View {
    let item: JSON
    @Environment(\.archipelTheme) private var theme
    public init(item: JSON) { self.item = item }
    public var body: some View {
        let output = (item.at("output.data")?.string).flatMap { Data(base64Encoded: $0) }.map { String(decoding: $0, as: UTF8.self) } ?? ""
        let ended: String = {
            guard let exit = item["exitStatus"], exit.object != nil else { return "running" }
            return exit["exitCode"]?.number.map { "exit \(Int($0))" } ?? "ended"
        }()
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("$ " + (item["command"]?.string ?? "")).font(theme.fonts.mono).lineLimit(1)
                Spacer()
                Badge(text: ended)
            }
            if !output.isEmpty { CodeBlock(text: output.trimmingCharacters(in: .newlines), terminal: true) }
        }
    }
}

/// Whether a tool call is still going, by its own status.
public func toolRunning(_ item: JSON) -> Bool {
    ["", "pending", "in_progress"].contains(item["status"]?.string ?? "")
}

func toolFailed(_ item: JSON) -> Bool {
    ["failed", "cancelled"].contains(item["status"]?.string ?? "")
}

/// A tool call: one quiet line with its kind, title and status, and under it what it produced.
/// The output shows while the call runs and folds away when it is over; the line opens and
/// closes it, and what the user chose stays. `terminal` finds a terminal by id, for calls whose
/// content points at one. `running` is the call's own status unless the caller knows better
/// (a call left `in_progress` by a turn that was cut short is not running).
public struct ToolCallView: View {
    let item: JSON
    var running: Bool
    var terminal: (String) -> JSON? = { _ in nil }
    /// What the user chose, once they pressed the line. Kept here only when the call is used
    /// alone: a timeline keeps it itself (`choice`), because a call changes rows when it ends.
    @State private var own: Bool?
    var choice: Binding<Bool?>?
    @Environment(\.archipelTheme) private var theme
    /// `open`: the user's choice from the start, for a picture of that state. `choice`: where
    /// the user's choice is kept, when the caller keeps it.
    public init(
        item: JSON, running: Bool? = nil, open: Bool? = nil, choice: Binding<Bool?>? = nil,
        terminal: @escaping (String) -> JSON? = { _ in nil }
    ) {
        self.item = item
        self.running = running ?? toolRunning(item)
        self.choice = choice
        self.terminal = terminal
        _own = State(initialValue: open)
    }

    /// The content blocks that draw something.
    private var blocks: [JSON] {
        (item["content"]?.array ?? []).filter { block in
            switch block["type"]?.string {
            case "terminal": return block["terminalId"]?.string.flatMap(terminal) != nil
            case "diff": return true
            default: return !contentText(block).isEmpty
            }
        }
    }

    public var body: some View {
        let blocks = blocks
        let status = item["status"]?.string ?? "pending"
        let kind = item["kind"]?.string ?? item["name"]?.string ?? "tool"
        // Closed, the title is one line: the first of a command written on several. Open, it
        // is all there, so a call with such a title opens even when it produced nothing.
        let title = (item["title"]?.string ?? item["name"]?.string ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let first = title.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? ""
        let opens = !blocks.isEmpty || first != title
        let chosen = choice ?? $own
        let open = opens && (chosen.wrappedValue ?? running)
        // A call that went well says nothing more: only what runs or went wrong is marked.
        let said = status == "completed" ? "" : ", " + status.replacingOccurrences(of: "_", with: " ")
        VStack(alignment: .leading, spacing: 4) {
            FoldLine(open: open, label: "\(kind), \(title)\(said)", tall: open, toggle: opens ? { chosen.wrappedValue = !open } : nil) {
                Label2(text: kind)
                Text(open ? title : first == title ? first : first + " …").font(theme.fonts.mono).lineLimit(open ? nil : 1)
                    .multilineTextAlignment(.leading)
                    .foregroundStyle(toolRunning(item) ? theme.colors.text : theme.colors.secondary)
                    .layoutPriority(-1)
                if status != "completed" { Badge(text: status, tone: toolFailed(item) ? .bad : .running) }
            }
            if open && !blocks.isEmpty {
                FoldBody {
                    ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                        switch block["type"]?.string {
                        case "terminal":
                            if let found = block["terminalId"]?.string.flatMap(terminal) { TerminalView(item: found) }
                        case "diff":
                            DiffView(block: block)
                        default:
                            CodeBlock(text: contentText(block))
                        }
                    }
                }
                .font(theme.fonts.body).foregroundStyle(theme.colors.text)
            }
        }
    }
}

/// Finished tool calls that followed one another, as one line: how many, and how many failed.
/// It opens to the calls (and what came between them), each still folded.
public struct ToolGroupView<Rows: View>: View {
    let calls: [JSON]
    @ViewBuilder var rows: Rows
    @State private var open: Bool
    @Environment(\.archipelTheme) private var theme
    /// `open`: shown unfolded at first.
    public init(calls: [JSON], open: Bool = false, @ViewBuilder rows: () -> Rows) {
        self.calls = calls
        self.rows = rows()
        _open = State(initialValue: open)
    }
    public var body: some View {
        let failed = calls.filter(toolFailed).count
        let count = "\(calls.count) tool calls"
        let wrong = failed > 0 ? " · \(failed) failed" : ""
        VStack(alignment: .leading, spacing: 6) {
            FoldLine(open: open, label: count + wrong, toggle: { open.toggle() }) {
                Text(count) + Text(wrong).foregroundColor(theme.colors.bad)
            }
            if open { FoldBody { rows } }
        }
    }
}

/// A plan: a document, or a list of entries with their status (which is also how todos arrive).
public struct PlanView: View {
    let item: JSON
    @Environment(\.archipelTheme) private var theme
    public init(item: JSON) { self.item = item }
    public var body: some View {
        Card {
            Label2(text: "Plan")
            let entries = item["entries"]?.array ?? []
            if entries.isEmpty {
                Text(contentText(item["content"]))
            } else {
                ForEach(Array(entries.enumerated()), id: \.offset) { _, entry in
                    let status = entry["status"]?.string ?? "pending"
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        PlanMark(status: status)
                        Text(entry["content"]?.string ?? "").strikethrough(status == "completed")
                            .fontWeight(status == "in_progress" ? .semibold : .regular)
                    }
                    .foregroundStyle(status == "completed" ? theme.colors.secondary : theme.colors.text)
                }
            }
        }
    }
}

/// A subagent: its title and state, and its own session below. Not a card: its timeline hangs
/// from its row on a guide line, so the cards in it are never inside another card.
public struct SubagentView<Inner: View>: View {
    let item: JSON
    @ViewBuilder var inner: Inner
    @Environment(\.archipelTheme) private var theme
    public init(item: JSON, @ViewBuilder inner: () -> Inner) {
        self.item = item
        self.inner = inner()
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Label2(text: "Subagent")
                Text(item["title"]?.string ?? "")
                Badge(text: item.at("state.state")?.string ?? "")
                Spacer(minLength: 0)
            }
            inner
                .padding(.leading, 18)
                .overlay(alignment: .leading) { Rectangle().fill(theme.colors.edge).frame(width: 1) }
                .padding(.leading, 6)
        }
    }
}

/// A message between a session and its subagent: the task given, or the result returned.
public struct SessionMessageView: View {
    let item: JSON
    @Environment(\.archipelTheme) private var theme
    public init(item: JSON) { self.item = item }
    public var body: some View {
        Card {
            (Label2.text("Between agents", theme) + Text("  ") + Text(contentText(item["content"])))
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

public struct CompactionView: View {
    let item: JSON
    @State private var open = false
    @Environment(\.archipelTheme) private var theme
    /// `open`: shown unfolded at first.
    public init(item: JSON, open: Bool = false) {
        self.item = item
        _open = State(initialValue: open)
    }
    public var body: some View {
        DisclosureGroup("Conversation compacted (\(item["status"]?.string ?? ""))", isExpanded: $open) {
            Text(contentText(item["summary"])).frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(theme.fonts.secondary).foregroundStyle(theme.colors.secondary)
    }
}

/// An approval: the question and its options, in a card that waits for the user. Once answered
/// it is one quiet line, like a tool call, that says which option was taken.
public struct ApprovalView: View {
    let request: Request
    var choose: (String) -> Void = { _ in }
    @Environment(\.archipelTheme) private var theme
    public init(request: Request, choose: @escaping (String) -> Void = { _ in }) {
        self.request = request
        self.choose = choose
    }
    public var body: some View {
        let options = request.params["options"]?.array ?? []
        let title = request.params["title"]?.string ?? ""
        if let outcome = request.outcome {
            let chosen = outcome.at("outcome.optionId")?.string
            let name = options.first { $0["optionId"]?.string == chosen }?["name"]?.string
            let answer = outcome.at("outcome.outcome")?.string == "cancelled" ? "Cancelled" : (name ?? chosen ?? "")
            FoldLine(open: false, label: "Permission, \(title), \(answer)") {
                Label2(text: "Permission")
                Text(title).lineLimit(1).layoutPriority(-1)
                Text(answer).lineLimit(1)
            }
        } else {
            Card(highlighted: true) {
                LabelledTitle(label: "Permission", title: title)
                HStack {
                    // The first option is the one most often taken.
                    ForEach(Array(options.enumerated()), id: \.offset) { index, option in
                        Button(option["name"]?.string ?? "") { choose(option["optionId"]?.string ?? "") }
                            .archipelButton(primary: index == 0)
                    }
                }
                .padding(.top, 4)
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
    @Environment(\.archipelTheme) private var theme
    public init(request: Request, answer: @escaping (JSON?) -> Void = { _ in }) {
        self.request = request
        self.answer = answer
    }
    public var body: some View {
        let fields = (request.params.at("requestedSchema.properties")?.object ?? [:]).sorted { $0.key < $1.key }
        Card(highlighted: request.outcome == nil) {
            LabelledTitle(label: "Question", title: request.params["message"]?.string ?? "")
            if let outcome = request.outcome {
                let given = (outcome["content"]?.object ?? [:]).sorted { $0.key < $1.key }.compactMap { $0.value.string }
                Text(outcome["action"]?.string == "accept" ? given.joined(separator: ", ") : "No answer").foregroundStyle(theme.colors.secondary)
            } else {
                ForEach(fields, id: \.key) { key, field in
                    let choices = field["oneOf"]?.array ?? []
                    let binding = Binding(get: { values[key] ?? "" }, set: { values[key] = $0 })
                    if choices.isEmpty {
                        TextField(field["title"]?.string ?? key, text: binding).archipelField()
                    } else {
                        Picker(field["title"]?.string ?? key, selection: binding) {
                            // Not a choice: says that one is still to be made.
                            Text("Choose…").tag("")
                            ForEach(Array(choices.enumerated()), id: \.offset) { _, choice in
                                Text(choice["title"]?.string ?? choice["const"]?.string ?? "").tag(choice["const"]?.string ?? "")
                            }
                        }
                    }
                }
                HStack {
                    Button("Answer") { answer(.object(values.filter { !$0.value.isEmpty }.mapValues { .string($0) })) }
                        .archipelButton(primary: true)
                    Button("Skip") { answer(nil) }.archipelButton()
                }
                .padding(.top, 4)
            }
        }
    }
}

/// Any item there is no view for, such as an extension's: its kind and its data.
public struct FallbackView: View {
    let kind: String
    let item: JSON
    @State private var open = false
    @Environment(\.archipelTheme) private var theme
    /// `open`: shown unfolded at first.
    public init(kind: String, item: JSON, open: Bool = false) {
        self.kind = kind
        self.item = item
        _open = State(initialValue: open)
    }
    public var body: some View {
        DisclosureGroup(kind, isExpanded: $open) { CodeBlock(text: item.text) }
            .font(theme.fonts.secondary).foregroundStyle(theme.colors.secondary)
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

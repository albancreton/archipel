import ArchipelClient
import SwiftUI
import UniformTypeIdentifiers

/// A slash command the agent offers (`state.values["availableCommands"]`).
public struct Command: Identifiable, Equatable {
    public var name: String
    public var description: String?
    /// What to type after the command, when the agent says.
    public var hint: String?
    public var id: String { name }

    /// The commands for what was typed after `/`: those whose name starts with it first, then
    /// those that contain it.
    public static func matching(_ query: String, in commands: [Command]) -> [Command] {
        let wanted = query.lowercased()
        let starts = commands.filter { $0.name.lowercased().hasPrefix(wanted) }
        return starts + commands.filter { !$0.name.lowercased().hasPrefix(wanted) && $0.name.lowercased().contains(wanted) }
    }

    /// Reads the agent's `availableCommands`.
    public static func all(from availableCommands: JSON?) -> [Command] {
        (availableCommands?.array ?? []).compactMap { entry in
            guard let name = entry["name"]?.string else { return nil }
            return Command(name: name, description: entry["description"]?.string, hint: entry.at("input.hint")?.string)
        }
    }
}

/// Something the user can mention with `@`: a file, usually. The app supplies them.
public struct Mention: Identifiable, Equatable, Sendable {
    /// Where it is, such as `file:///work/app/src/main.ts`. Sent to the agent as a link.
    public var uri: String
    /// What is written into the text after `@`, such as `src/main.ts`. No spaces.
    public var name: String
    /// A second line in the menu.
    public var detail: String?
    public var id: String { uri }

    public init(uri: String, name: String, detail: String? = nil) {
        self.uri = uri
        self.name = name
        self.detail = detail
    }
}

/// A file the user attached.
public struct Attachment: Identifiable, Equatable {
    public let id = UUID()
    public var name: String
    public var mimeType: String
    public var data: Data

    public init(name: String, mimeType: String, data: Data) {
        self.name = name
        self.mimeType = mimeType
        self.data = data
    }

    var isImage: Bool { mimeType.hasPrefix("image/") }
}

/// What the agent takes in a message besides text.
public struct Accepts: Equatable {
    /// Pictures.
    public var image = false
    /// Other files, sent with their content.
    public var files = false

    public init(image: Bool = false, files: Bool = false) {
        self.image = image
        self.files = files
    }

    /// From one entry of `client.agents()`. Nothing until the agent has started.
    public init(agent: JSON?) {
        image = agent?.at("capabilities.session.prompt.image") != nil
        files = agent?.at("capabilities.session.prompt.embeddedContext") != nil
    }
}

/// The command the user is typing: the draft is `/` and a name with nothing after it yet.
public func commandQuery(_ draft: String) -> String? {
    guard draft.hasPrefix("/") else { return nil }
    let name = draft.dropFirst()
    return name.contains(where: { $0.isWhitespace || $0 == "/" }) ? nil : String(name)
}

/// The mention the user is typing at the end of the draft: `@` at the start of a word, and
/// what follows it. (A SwiftUI text field does not say where the caret is, so only the end counts.)
public func mentionQuery(_ draft: String) -> (start: String.Index, query: String)? {
    guard let at = draft.lastIndex(of: "@") else { return nil }
    let query = draft[draft.index(after: at)...]
    if query.contains(where: \.isWhitespace) { return nil }
    if at > draft.startIndex, !draft[draft.index(before: at)].isWhitespace { return nil }
    return (at, String(query))
}

/// What goes to the agent, as content blocks: the text, a `resource_link` for each mention
/// whose `@name` is still in the text, an `image` for each picture and a `resource` with its
/// content for each other file.
public func promptContent(_ text: String, attachments: [Attachment] = [], mentions: [Mention] = []) -> [JSON] {
    var blocks: [JSON] = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? [] : [["type": "text", "text": .string(text)]]
    let words = Set(text.split(whereSeparator: \.isWhitespace).map(String.init))
    var linked = Set<String>()
    for mention in mentions where words.contains("@" + mention.name) && linked.insert(mention.uri).inserted {
        blocks.append(["type": "resource_link", "uri": .string(mention.uri), "name": .string(mention.name)])
    }
    for file in attachments {
        if file.isImage {
            blocks.append(["type": "image", "mimeType": .string(file.mimeType), "data": .string(file.data.base64EncodedString())])
        } else {
            let uri = "file:///" + (file.name.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(.init(charactersIn: "/"))) ?? file.name)
            let textual = file.mimeType.hasPrefix("text/") || ["json", "xml", "javascript", "typescript", "yaml", "toml", "x-sh"].contains { file.mimeType.contains($0) }
            let resource: JSON = if textual, let text = String(data: file.data, encoding: .utf8) {
                ["uri": .string(uri), "mimeType": .string(file.mimeType), "text": .string(text)]
            } else {
                ["uri": .string(uri), "mimeType": .string(file.mimeType), "blob": .string(file.data.base64EncodedString())]
            }
            blocks.append(["type": "resource", "resource": resource])
        }
    }
    return blocks
}

/// Where the user types: text, attachments, the agent's commands after `/`, and files after `@`.
public struct Composer: View {
    let running: Bool
    /// Sends the message. If it throws, the composer gives everything back.
    let send: ([JSON]) async throws -> Void
    var cancel: () -> Void
    var commands: [Command]
    /// Finds what the user may mention, for what they typed after `@`. Without it `@` is only a character.
    var mentions: ((String) async -> [Mention])?
    var accepts: Accepts
    /// The largest file that can be attached, in bytes.
    var maxBytes: Int

    @State private var draft = ""
    @State private var attached: [Attachment] = []
    @State private var mentioned: [Mention] = []
    @State private var found: [Mention] = []
    @State private var chosen = 0
    @State private var closed = false
    @State private var picking = false
    @State private var problem: String?
    @Environment(\.archipelTheme) private var theme

    public init(
        running: Bool, send: @escaping ([JSON]) async throws -> Void, cancel: @escaping () -> Void = {}, commands: [Command] = [],
        mentions: ((String) async -> [Mention])? = nil, accepts: Accepts = Accepts(), maxBytes: Int = 5_000_000,
        text: String = ""
    ) {
        // `text`: what the field holds at first, such as a draft the app kept.
        _draft = State(initialValue: text)
        self.running = running
        self.send = send
        self.cancel = cancel
        self.commands = commands
        self.mentions = mentions
        self.accepts = accepts
        self.maxBytes = maxBytes
    }

    private struct Entry: Identifiable {
        let id: String
        let label: String
        let detail: String?
        let pick: () -> Void
    }

    private var menu: [Entry] {
        if closed { return [] }
        if let query = commandQuery(draft) {
            return Command.matching(query, in: commands).prefix(8).map { command in
                Entry(id: command.name, label: "/" + command.name, detail: [command.hint, command.description].compactMap { $0 }.joined(separator: " · ")) {
                    draft = "/\(command.name) "
                }
            }
        }
        if mentions != nil, let typing = mentionQuery(draft) {
            return found.prefix(8).map { mention in
                Entry(id: mention.uri, label: "@" + mention.name, detail: mention.detail) {
                    mentioned.append(mention)
                    draft = draft[..<typing.start] + "@\(mention.name) "
                }
            }
        }
        return []
    }

    public var body: some View {
        let menu = menu
        let at = min(chosen, max(menu.count - 1, 0))
        VStack(alignment: .leading, spacing: 6) {
            if !menu.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(menu.enumerated()), id: \.element.id) { index, entry in
                        Button(action: entry.pick) {
                            HStack(alignment: .firstTextBaseline, spacing: 10) {
                                Text(entry.label).font(theme.fonts.mono)
                                // On the chosen row the detail takes the text colour, to stay readable on its fill.
                                Text(entry.detail ?? "").font(theme.fonts.meta).foregroundStyle(index == at ? theme.colors.text : theme.colors.secondary).lineLimit(1)
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 8).padding(.vertical, 4)
                            .background(index == at ? theme.colors.accent.opacity(0.22) : .clear, in: RoundedRectangle(cornerRadius: theme.controlRadius))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(4)
                .background(RoundedRectangle(cornerRadius: theme.cardRadius).fill(theme.drawsControls ? theme.colors.raised : .clear))
                .overlay(RoundedRectangle(cornerRadius: theme.cardRadius).stroke(theme.drawsControls ? theme.colors.edge : theme.colors.line))
            }
            if !attached.isEmpty {
                HStack(spacing: 6) {
                    ForEach(attached) { file in
                        HStack(spacing: 4) {
                            Image(systemName: file.isImage ? "photo" : "doc")
                            Text(file.name).lineLimit(1)
                            Button { attached.removeAll { $0.id == file.id } } label: { Image(systemName: "xmark") }
                                .buttonStyle(.plain).accessibilityLabel("Remove \(file.name)")
                        }
                        .font(theme.fonts.meta)
                        .padding(.horizontal, 8).padding(.vertical, 3)
                        .background(theme.colors.raised, in: RoundedRectangle(cornerRadius: theme.controlRadius))
                    }
                }
            }
            if let problem { Text(problem).font(theme.fonts.secondary).foregroundStyle(theme.colors.bad) }
            HStack(alignment: .bottom) {
                TextField(running ? "Add to the running turn…" : "Message the agent…", text: $draft, axis: .vertical)
                    .lineLimit(1...6)
                    .archipelField()
                    // These ask for the menu when the key is pressed. The one drawn above may be
                    // older: the field keeps its handlers until its own text changes, and the
                    // files to mention arrive later than that.
                    .onSubmit { if !pickChosen() { submit() } }
                    .onKeyPress(.downArrow) { move(1, in: self.menu.count) }
                    .onKeyPress(.upArrow) { move(-1, in: self.menu.count) }
                    .onKeyPress(.tab) { pickChosen() ? .handled : .ignored }
                    .onKeyPress(.escape) {
                        guard !self.menu.isEmpty else { return .ignored }
                        closed = true
                        return .handled
                    }
                if accepts.image || accepts.files {
                    Button("Attach") { picking = true }.archipelButton()
                }
                Button("Send") { submit() }.archipelButton(primary: true)
                if running { Button("Stop", action: cancel).archipelButton() }
            }
        }
        .onChange(of: draft) {
            closed = false
            chosen = 0
        }
        .task(id: mentions == nil ? nil : mentionQuery(draft)?.query) {
            guard let mentions, let query = mentionQuery(draft)?.query else { return found = [] }
            let list = await mentions(query)
            if !Task.isCancelled { found = list }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: accepts.files ? [.item] : [.image], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { attach(urls) }
        }
        .dropDestination(for: URL.self) { urls, _ in
            guard accepts.image || accepts.files else { return false }
            attach(urls)
            return true
        }
    }

    /// Takes the chosen entry of the menu, if a menu is open.
    private func pickChosen() -> Bool {
        let menu = menu
        guard !menu.isEmpty else { return false }
        menu[min(chosen, menu.count - 1)].pick()
        return true
    }

    private func move(_ by: Int, in count: Int) -> KeyPress.Result {
        guard count > 0 else { return .ignored }
        chosen = (min(chosen, count - 1) + by + count) % count
        return .handled
    }

    private func attach(_ urls: [URL]) {
        problem = nil
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let type = UTType(filenameExtension: url.pathExtension)
            let mimeType = type?.preferredMIMEType ?? (type?.conforms(to: .text) == true ? "text/plain" : "application/octet-stream")
            let image = mimeType.hasPrefix("image/")
            guard image ? accepts.image : accepts.files else {
                problem = "\(url.lastPathComponent): the agent does not take \(image ? "pictures" : "files")"
                continue
            }
            guard let data = try? Data(contentsOf: url) else {
                problem = "\(url.lastPathComponent): could not be read"
                continue
            }
            guard data.count <= maxBytes else {
                problem = "\(url.lastPathComponent): larger than \(maxBytes / 1_000_000) MB"
                continue
            }
            attached.append(Attachment(name: url.lastPathComponent, mimeType: mimeType, data: data))
        }
    }

    private func submit() {
        let text = draft, files = attached, links = mentioned
        let content = promptContent(text, attachments: files, mentions: links)
        guard !content.isEmpty else { return }
        draft = ""
        attached = []
        mentioned = []
        problem = nil
        Task {
            do {
                try await send(content)
            } catch {
                // A refused message goes back into the field, unless the user has typed since.
                if draft.isEmpty { draft = text }
                if attached.isEmpty { attached = files }
                if mentioned.isEmpty { mentioned = links }
                // The notice above already says it when the session is open elsewhere.
                if (error as? RPCError)?.openElsewhere != true { problem = "Not sent: \(describe(error))" }
            }
        }
    }
}

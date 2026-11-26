import SwiftUI

/// One block of Markdown text, ready to draw.
enum MarkdownBlock: Equatable {
    case paragraph(AttributedString)
    case heading(Int, AttributedString)
    case code(String, language: String?)
    /// `marker` is `•` or `3.`; `depth` counts the lists the item is inside, from 1.
    case listItem(AttributedString, marker: String, depth: Int)
    case quote(AttributedString)
    case table([[AttributedString]])
    case rule
}

/// Splits Markdown into blocks with Apple's own parser (Foundation), so no dependency.
///
/// Text that is still arriving is fine: a code fence that is not closed yet is code up to where
/// the text ends. Markup the agent writes between angle brackets stays text, and a link is
/// kept only when it goes to the web or to a mail address.
func markdownBlocks(_ text: String) -> [MarkdownBlock] {
    let options = AttributedString.MarkdownParsingOptions(
        allowsExtendedAttributes: false, interpretedSyntax: .full, failurePolicy: .returnPartiallyParsedIfPossible)
    guard var parsed = try? AttributedString(markdown: text, options: options) else {
        return text.isEmpty ? [] : [.paragraph(AttributedString(text))]
    }
    for run in parsed.runs {
        if let link = run.link, !["http", "https", "mailto"].contains(link.scheme?.lowercased() ?? "") {
            parsed[run.range].link = nil
        }
    }

    var blocks: [MarkdownBlock] = []
    /// The table being collected, by the identity the parser gives it.
    var table: (id: Int, rows: [[AttributedString]])?
    /// The list item last appended, to tell its second paragraph from a new item.
    var lastItem: Int?
    func endTable() {
        // Rows are placed by the parser's own numbering; a number it skipped leaves no row.
        if let done = table { blocks.append(.table(done.rows.filter { !$0.isEmpty })) }
        table = nil
    }
    // Runs of one block share its intent; a block ends where the intent changes.
    var start = parsed.startIndex
    while start < parsed.endIndex {
        let intent = parsed.runs[start].presentationIntent
        var end = parsed.runs[start].range.upperBound
        while end < parsed.endIndex, parsed.runs[end].presentationIntent == intent { end = parsed.runs[end].range.upperBound }
        let piece = AttributedString(parsed[start..<end])
        start = end

        let kinds = intent?.components ?? []
        var level: Int?, language: String??, ordinal: Int?, item: Int?, ordered = false, quoted = false, depth = 0
        var cell: (table: Int, row: Int)?
        var rule = false
        // From the innermost outwards. The first list met is the item's own.
        for component in kinds {
            switch component.kind {
            case .header(let found): level = found
            case .codeBlock(let hint): language = .some(hint)
            case .listItem(let found): if ordinal == nil { ordinal = found; item = component.identity }
            case .orderedList: if depth == 0 { ordered = true }; depth += 1
            case .unorderedList: depth += 1
            case .blockQuote: quoted = true
            case .thematicBreak: rule = true
            case .tableHeaderRow: cell = (cell?.table ?? 0, 0)
            case .tableRow(let row): cell = (cell?.table ?? 0, row + 1)
            case .table: cell = (component.identity, cell?.row ?? 0)
            default: break
            }
        }

        if let cell {
            if table?.id != cell.table { endTable(); table = (cell.table, []) }
            while (table?.rows.count ?? 0) <= cell.row { table?.rows.append([]) }
            table?.rows[cell.row].append(piece)
            continue
        }
        endTable()
        if rule {
            blocks.append(.rule)
        } else if let language {
            var code = String(piece.characters)
            if code.hasSuffix("\n") { code.removeLast() }
            blocks.append(.code(code, language: language))
        } else if let level {
            blocks.append(.heading(level, piece))
        } else if let ordinal {
            let marker = ordered ? "\(ordinal)." : "•"
            // A second paragraph of one item continues it, without a second marker.
            if item == lastItem, case .listItem(let before, _, let same)? = blocks.last, same == depth {
                blocks[blocks.count - 1] = .listItem(before + AttributedString("\n") + piece, marker: marker, depth: depth)
            } else {
                blocks.append(.listItem(piece, marker: marker, depth: max(depth, 1)))
            }
            lastItem = item
        } else if quoted {
            blocks.append(.quote(piece))
        } else {
            blocks.append(.paragraph(piece))
        }
    }
    endTable()
    return blocks
}

/// The agent's Markdown, drawn block by block.
public struct MarkdownText: View {
    let text: String
    @Environment(\.archipelTheme) private var theme

    public init(_ text: String) { self.text = text }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(markdownBlocks(text).enumerated()), id: \.offset) { _, block in
                view(block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .textSelection(.enabled)
    }

    @ViewBuilder
    private func view(_ block: MarkdownBlock) -> some View {
        switch block {
        case .paragraph(let text):
            Text(text)
        case .heading(let level, let text):
            // Chat text is not a document: its headings stay close to the text's size.
            Text(text).font(level <= 1 ? theme.fonts.headingLarge : theme.fonts.heading).fontWeight(.semibold).padding(.top, 2)
        case .code(let code, _):
            // Long lines wrap, as in the kit's other code blocks.
            CodeBlock(text: code)
        case .listItem(let text, let marker, let depth):
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(marker).foregroundStyle(theme.colors.secondary).monospacedDigit()
                Text(text)
            }
            .padding(.leading, CGFloat(depth - 1) * 16)
        case .quote(let text):
            Text(text).foregroundStyle(theme.colors.secondary).padding(.leading, 12)
                .overlay(alignment: .leading) { Rectangle().fill(theme.colors.edge).frame(width: 3) }
        case .table(let rows):
            Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 4) {
                ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                    GridRow {
                        ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                            Text(cell).fontWeight(index == 0 ? .semibold : .regular)
                        }
                    }
                    if index == 0 { Divider() }
                }
            }
        case .rule:
            Divider()
        }
    }
}

private struct AgentTextKey: EnvironmentKey {
    static let defaultValue: @MainActor (String) -> AnyView = { AnyView(MarkdownText($0)) }
}

extension EnvironmentValues {
    /// How the agent's text is drawn. The default is `MarkdownText`.
    public var agentText: @MainActor (String) -> AnyView {
        get { self[AgentTextKey.self] }
        set { self[AgentTextKey.self] = newValue }
    }
}

extension View {
    /// Draws the agent's text with another view: one with code highlighting, say, or
    /// `{ AnyView(Text($0)) }` for the text as written.
    public func agentText(_ render: @escaping @MainActor (String) -> AnyView) -> some View {
        environment(\.agentText, render)
    }
}

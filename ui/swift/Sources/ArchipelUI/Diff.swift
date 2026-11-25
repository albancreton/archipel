import ArchipelClient
import SwiftUI

/// One row of a diff.
indirect enum DiffRow: Equatable {
    case add(String, new: Int)
    case remove(String, old: Int)
    case context(String, old: Int, new: Int)
    case hunk(String)
    /// Unchanged lines that are not shown until the user asks.
    case fold([DiffRow])

    var isContext: Bool { if case .context = self { true } else { false } }
    var isHunk: Bool { if case .hunk = self { true } else { false } }
}

struct DiffFile: Equatable {
    var path: String
    var added = 0
    var removed = 0
    var rows: [DiffRow] = []
}

/// Reads a unified patch (what `git diff` prints). Text that is no patch gives no files.
func parsePatch(_ patch: String) -> [DiffFile] {
    func name(_ line: Substring) -> String {
        var name = String(line.dropFirst(4))
        if let tab = name.firstIndex(of: "\t") { name = String(name[..<tab]) }
        if name.hasPrefix("a/") || name.hasPrefix("b/") { name.removeFirst(2) }
        return name
    }
    var files: [DiffFile] = []
    var oldLine = 0, newLine = 0, inHunk = false, hasFile = false
    var lines = patch.split(separator: "\n", omittingEmptySubsequences: false)
    if lines.last == "" { lines.removeLast() }
    var at = 0
    while at < lines.count {
        let line = lines[at]
        at += 1
        if line.hasPrefix("diff --git ") { hasFile = false; inHunk = false; continue }
        if line.hasPrefix("--- "), at < lines.count, lines[at].hasPrefix("+++ ") {
            let to = name(lines[at])
            files.append(DiffFile(path: to == "/dev/null" || to == "dev/null" ? name(line) : to))
            hasFile = true
            inHunk = false
            at += 1
            continue
        }
        if let hunk = line.firstMatch(of: hunkHeader) {
            if !hasFile { files.append(DiffFile(path: "")); hasFile = true }
            oldLine = hunk.output[1].substring.flatMap { Int($0) } ?? 0
            newLine = hunk.output[2].substring.flatMap { Int($0) } ?? 0
            inHunk = true
            files[files.count - 1].rows.append(.hunk(String(line)))
            continue
        }
        guard hasFile, inHunk else { continue } // index lines, modes, and text before the first hunk
        let text = String(line.dropFirst())
        let file = files.count - 1
        if line.hasPrefix("+") {
            files[file].rows.append(.add(text, new: newLine)); newLine += 1; files[file].added += 1
        } else if line.hasPrefix("-") {
            files[file].rows.append(.remove(text, old: oldLine)); oldLine += 1; files[file].removed += 1
        } else if line.hasPrefix("\\") {
            continue // "\ No newline at end of file"
        } else {
            files[file].rows.append(.context(text, old: oldLine, new: newLine)); oldLine += 1; newLine += 1
        }
    }
    return files
}

private nonisolated(unsafe) let hunkHeader = try! Regex(#"^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@"#)

/// Folds runs of unchanged lines, keeping `keep` of them next to each change. A run is folded
/// only when that hides more than three lines; fewer are not worth a click.
func foldContext(_ rows: [DiffRow], keep: Int = 3) -> [DiffRow] {
    var folded: [DiffRow] = []
    var at = 0
    while at < rows.count {
        guard rows[at].isContext else { folded.append(rows[at]); at += 1; continue }
        var end = at
        while end < rows.count, rows[end].isContext { end += 1 }
        let run = Array(rows[at..<end])
        // No change before the run (the start, or a hunk header) or after it: keep that side short.
        let before = (folded.last.map { !$0.isHunk } ?? false) ? keep : 0
        let after = end < rows.count ? keep : 0
        if run.count - before - after > 3 {
            folded += run.prefix(before)
            folded.append(.fold(Array(run[before..<(run.count - after)])))
            folded += run.suffix(after)
        } else {
            folded += run
        }
        at = end
    }
    return folded
}

/// A diff: per file, how much changed and the changed lines with their numbers. Long unchanged
/// parts are folded. A patch this cannot read is shown as it is.
public struct DiffView: View {
    let block: JSON
    public init(block: JSON) { self.block = block }

    public var body: some View {
        let patch = block.at("patch.text")?.string ?? ""
        let files = parsePatch(patch)
        let changes = block["changes"]?.array ?? []
        VStack(alignment: .leading, spacing: 6) {
            if files.isEmpty {
                ForEach(Array(changes.enumerated()), id: \.offset) { _, change in
                    Text("\(change["operation"]?.string ?? "") \(change["path"]?.string ?? "")").font(.caption).foregroundStyle(.secondary)
                }
                if !patch.isEmpty { CodeBlock(text: patch) }
            }
            ForEach(Array(files.enumerated()), id: \.offset) { _, file in
                let operation = changes.first { $0["path"]?.string == file.path }?["operation"]?.string
                VStack(alignment: .leading, spacing: 0) {
                    HStack(spacing: 6) {
                        Text([operation, file.path].compactMap { $0 }.joined(separator: " "))
                        Text("+\(file.added)").foregroundStyle(.green)
                        Text("−\(file.removed)").foregroundStyle(.red)
                    }
                    .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 8).padding(.vertical, 4)
                    Divider()
                    DiffRows(rows: foldContext(file.rows))
                }
                .background(.background, in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.secondary.opacity(0.25)))
            }
        }
    }
}

/// The rows of one file. A fold opens in place.
struct DiffRows: View {
    let rows: [DiffRow]
    @State private var opened: Set<Int> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                if case .fold(let inner) = row {
                    if opened.contains(index) {
                        DiffRows(rows: inner)
                    } else {
                        Button("Show \(inner.count) unchanged lines") { opened.insert(index) }
                            .buttonStyle(.plain).foregroundStyle(Color.accentColor).padding(.horizontal, 8)
                    }
                } else {
                    line(row)
                }
            }
        }
        .font(.system(.caption, design: .monospaced))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func line(_ row: DiffRow) -> some View {
        switch row {
        case .hunk(let text):
            Text(text).foregroundStyle(.secondary).padding(.horizontal, 8)
                .frame(maxWidth: .infinity, alignment: .leading).background(Color.secondary.opacity(0.1))
        case .add(let text, let new):
            numbered(nil, new, "+", text).background(Color.green.opacity(0.14))
        case .remove(let text, let old):
            numbered(old, nil, "-", text).background(Color.red.opacity(0.14))
        case .context(let text, let old, let new):
            numbered(old, new, " ", text)
        case .fold:
            EmptyView()
        }
    }

    private func numbered(_ old: Int?, _ new: Int?, _ sign: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 6) {
            Text(old.map(String.init) ?? "").frame(width: 34, alignment: .trailing).foregroundStyle(.secondary)
            Text(new.map(String.init) ?? "").frame(width: 34, alignment: .trailing).foregroundStyle(.secondary)
            Text(sign)
            Text(text)
            Spacer(minLength: 0)
        }
    }
}

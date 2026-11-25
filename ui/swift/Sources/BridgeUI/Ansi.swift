import SwiftUI

/// A colour in terminal output: one of the terminal's own 16, or an exact one.
enum AnsiColor: Equatable {
    case palette(Int)
    case rgb(Int, Int, Int)

    /// A colour of the 256-colour table.
    static func indexed(_ n: Int) -> AnsiColor {
        if n < 16 { return .palette(n) }
        if n >= 232 {
            let grey = 8 + (n - 232) * 10
            return .rgb(grey, grey, grey)
        }
        func step(_ value: Int) -> Int { value == 0 ? 0 : 55 + value * 40 }
        let at = n - 16
        return .rgb(step(at / 36), step(at / 6 % 6), step(at % 6))
    }

    var color: Color {
        switch self {
        case .rgb(let red, let green, let blue):
            return Color(red: Double(red) / 255, green: Double(green) / 255, blue: Double(blue) / 255)
        case .palette(let index):
            // The system's colours, so they follow light and dark by themselves.
            let base: [Color] = [.primary, .red, .green, .yellow, .blue, .purple, .cyan, .secondary]
            return index < 8 ? base[index] : (index == 8 ? .secondary : index == 15 ? .primary : base[index - 8])
        }
    }
}

struct AnsiStyle: Equatable {
    var fg: AnsiColor?
    var bg: AnsiColor?
    var bold = false
    var dim = false
    var italic = false
    var underline = false
    var inverse = false

    mutating func apply(_ codes: [Int]) {
        var at = 0
        while at < codes.count {
            let code = codes[at]
            switch code {
            case 0: self = AnsiStyle()
            case 1: bold = true
            case 2: dim = true
            case 3: italic = true
            case 4: underline = true
            case 7: inverse = true
            case 22: bold = false; dim = false
            case 23: italic = false
            case 24: underline = false
            case 27: inverse = false
            case 30...37: fg = .palette(code - 30)
            case 90...97: fg = .palette(code - 90 + 8)
            case 39: fg = nil
            case 40...47: bg = .palette(code - 40)
            case 100...107: bg = .palette(code - 100 + 8)
            case 49: bg = nil
            case 38, 48:
                var color: AnsiColor?
                if at + 2 < codes.count, codes[at + 1] == 5 {
                    color = .indexed(codes[at + 2])
                    at += 2
                } else if at + 4 < codes.count, codes[at + 1] == 2 {
                    color = .rgb(codes[at + 2], codes[at + 3], codes[at + 4])
                    at += 4
                }
                if let color { if code == 38 { fg = color } else { bg = color } }
            default: break
            }
            at += 1
        }
    }
}

struct AnsiPiece: Equatable {
    var text: String
    var style: AnsiStyle
}

/// Splits terminal output into pieces of one style each. The colour codes a command prints are
/// read; the other control codes are dropped. Not a terminal emulator: nothing moves the cursor
/// except a return to the start of the line, which is how progress bars redraw.
func ansiPieces(_ output: String) -> [AnsiPiece] {
    // CSI (ESC [ … letter), OSC (ESC ] … BEL or ESC \), and two-character escapes.
    let escape = ansiEscape
    var pieces: [AnsiPiece] = []
    var style = AnsiStyle()
    var from = output.startIndex
    func push(_ text: Substring) {
        let shown = withReturns(String(text)).filter { character in
            character == "\n" || character == "\t" || !character.unicodeScalars.contains { $0.value < 0x20 }
        }
        if !shown.isEmpty { pieces.append(AnsiPiece(text: shown, style: style)) }
    }
    for found in output.matches(of: escape) {
        push(output[from..<found.range.lowerBound])
        from = found.range.upperBound
        if found.output[2].substring == "m" {
            style.apply((found.output[1].substring ?? "").split(omittingEmptySubsequences: false) { $0 == ";" || $0 == ":" }.map { Int($0) ?? 0 })
        }
    }
    push(output[from...])
    return pieces
}

// Built once: a pattern in a string, since the escape character cannot be written in a regex literal.
private nonisolated(unsafe) let ansiEscape = try! Regex(#"\x1B\[([0-9;:?]*)([@-~])|\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)|\x1B[@-Z\\^_]"#)

/// Applies returns to the start of a line: what is printed after one replaces what was there.
private func withReturns(_ text: String) -> String {
    guard text.contains("\r") else { return text }
    // "\r\n" is one character in Swift, so lines are split on the scalars.
    let scalars = String.UnicodeScalarView(text.unicodeScalars)
    return scalars.split(separator: "\n", omittingEmptySubsequences: false).map { line -> String in
        var parts = line.split(separator: "\r", omittingEmptySubsequences: false).map { String(String.UnicodeScalarView($0)) }
        while parts.count > 1, parts.last == "" { parts.removeLast() }
        return parts.reduce("") { shown, part in part + shown.dropFirst(part.count) }
    }.joined(separator: "\n")
}

/// Terminal output with its colours.
public struct AnsiText: View {
    let output: String
    public init(_ output: String) { self.output = output }

    public var body: some View {
        Text(attributed)
    }

    var attributed: AttributedString {
        var result = AttributedString()
        for piece in ansiPieces(output) {
            var part = AttributedString(piece.text)
            let style = piece.style
            let (fg, bg): (Color?, Color?) = style.inverse
                ? (style.bg?.color ?? Color(white: 1), style.fg?.color ?? .primary)
                : (style.fg?.color, style.bg?.color)
            if let fg { part.foregroundColor = style.dim ? fg.opacity(0.65) : fg } else if style.dim { part.foregroundColor = .secondary }
            if let bg { part.backgroundColor = bg }
            var intent: InlinePresentationIntent = []
            if style.bold { intent.insert(.stronglyEmphasized) }
            if style.italic { intent.insert(.emphasized) }
            if !intent.isEmpty { part.inlinePresentationIntent = intent }
            if style.underline { part.underlineStyle = .single }
            result += part
        }
        return result
    }
}

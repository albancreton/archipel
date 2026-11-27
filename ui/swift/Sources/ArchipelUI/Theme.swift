import SwiftUI

/// How the kit looks: its colours, type, shapes, and whether it draws its own controls. Every
/// view reads it from the environment, so `.archipelTheme(_:)` on any view above restyles them
/// all. `.standard` is the system's own look; a theme such as Minitel (`ui/themes/minitel/swift`)
/// is another value of this type.
public struct ArchipelTheme {
    public struct Colors {
        /// Text, and secondary text (labels, metadata, what is done).
        public var text: Color
        public var secondary: Color
        /// Behind the chat and the session list. Nil leaves the window's own.
        public var background: Color?
        /// Cards: tool calls, plans, requests, messages between agents.
        public var card: Color
        /// Chips, menus, buttons that are not the primary one.
        public var raised: Color
        /// The user's message, and the text on it.
        public var message: Color
        public var onMessage: Color
        /// Code, diffs, terminal output, fields.
        public var sunken: Color
        /// Borders, and the stronger edge of a hovered border, a meter, a guide line.
        public var line: Color
        public var edge: Color
        /// Primary buttons and links, with `onAccent` on them.
        public var accent: Color
        public var onAccent: Color
        /// Done, failed, running, and waiting for the user.
        public var ok: Color
        public var bad: Color
        public var running: Color
        public var attention: Color
        /// The terminal's 16 colours, for command output.
        public var ansi: [Color]

        public init(
            text: Color, secondary: Color, background: Color?, card: Color, raised: Color, message: Color, onMessage: Color, sunken: Color,
            line: Color, edge: Color, accent: Color, onAccent: Color, ok: Color, bad: Color, running: Color, attention: Color,
            ansi: [Color]
        ) {
            (self.text, self.secondary, self.background, self.card, self.raised, self.message, self.onMessage, self.sunken) =
                (text, secondary, background, card, raised, message, onMessage, sunken)
            (self.line, self.edge, self.accent, self.onAccent) = (line, edge, accent, onAccent)
            (self.ok, self.bad, self.running, self.attention, self.ansi) = (ok, bad, running, attention, ansi)
        }
    }

    public struct Fonts {
        public var body: Font
        /// Folds, notices, the activity line.
        public var secondary: Font
        /// The session's title, and a request's.
        public var title: Font
        public var requestTitle: Font
        /// The agent's headings: level 1, then the others.
        public var headingLarge: Font
        public var heading: Font
        /// Tool titles and commands, and code blocks, diffs and terminal output.
        public var mono: Font
        public var code: Font
        /// Badges, a row's details, usage.
        public var meta: Font
        /// The uppercase word naming a part, and its letter spacing.
        public var label: Font
        public var labelTracking: CGFloat

        public init(
            body: Font, secondary: Font, title: Font, requestTitle: Font, headingLarge: Font, heading: Font, mono: Font,
            code: Font, meta: Font, label: Font, labelTracking: CGFloat
        ) {
            (self.body, self.secondary, self.title, self.requestTitle, self.headingLarge, self.heading) =
                (body, secondary, title, requestTitle, headingLarge, heading)
            (self.mono, self.code, self.meta, self.label, self.labelTracking) = (mono, code, meta, label, labelTracking)
        }
    }

    public enum BadgeStyle {
        /// A coloured outline around coloured text.
        case outlined
        /// A pale fill of the tone, a dot of the tone, and text between the tone and the text colour.
        case tinted
    }

    public var colors: Colors
    public var fonts: Fonts
    public var badge: BadgeStyle
    public var cardRadius: CGFloat
    /// Code blocks, chips, fields and drawn buttons.
    public var controlRadius: CGFloat
    public var cardPadding: EdgeInsets
    /// Between the rows of a timeline.
    public var rowSpacing: CGFloat
    /// How wide a message's column gets, the user's or the agent's: about 70 characters of the
    /// body font.
    public var columnWidth: CGFloat
    /// How wide a chat gets, in the middle of a wider window: about 150 characters of the body font.
    public var chatWidth: CGFloat
    /// The border of a card waiting for the user, and how much of the attention colour fills it.
    public var waitingBorder: CGFloat
    public var waitingFill: Double
    /// True: buttons and text fields are drawn from the theme's colours. False: the system's own.
    public var drawsControls: Bool
    /// The size of the settings bar's menus and switches.
    public var settingsControlSize: ControlSize
    /// True: the session list's folder headings are drawn as labels (capitals, the label font).
    public var folderLabels: Bool
    /// Forced on everything below, so system controls match; nil follows the system.
    public var colorScheme: ColorScheme?

    public init(
        colors: Colors, fonts: Fonts, badge: BadgeStyle = .outlined, cardRadius: CGFloat = 10, controlRadius: CGFloat = 6,
        cardPadding: EdgeInsets = EdgeInsets(top: 10, leading: 10, bottom: 10, trailing: 10), rowSpacing: CGFloat = 10,
        waitingBorder: CGFloat = 1, waitingFill: Double = 0, drawsControls: Bool = false, settingsControlSize: ControlSize = .small,
        folderLabels: Bool = false, colorScheme: ColorScheme? = nil, columnWidth: CGFloat = 540,
        chatWidth: CGFloat = 1155
    ) {
        (self.colors, self.fonts, self.badge, self.cardRadius, self.controlRadius, self.cardPadding) =
            (colors, fonts, badge, cardRadius, controlRadius, cardPadding)
        (self.rowSpacing, self.waitingBorder, self.waitingFill, self.drawsControls, self.settingsControlSize) =
            (rowSpacing, waitingBorder, waitingFill, drawsControls, settingsControlSize)
        (self.folderLabels, self.colorScheme, self.columnWidth, self.chatWidth) = (folderLabels, colorScheme, columnWidth, chatWidth)
    }

    /// The system's look: its text styles, semantic colours and controls, light or dark as the system is.
    public static let standard = ArchipelTheme(
        colors: Colors(
            text: .primary, secondary: .secondary, background: nil, card: Color.primary.opacity(0.05),
            raised: Color.secondary.opacity(0.14), message: .accentColor, onMessage: .white, sunken: systemBackground,
            line: Color.secondary.opacity(0.25), edge: .secondary, accent: .accentColor, onAccent: .white,
            ok: .green, bad: .red, running: .orange, attention: .orange,
            ansi: [.primary, .red, .green, .yellow, .blue, .purple, .cyan, .secondary, .secondary, .red, .green, .yellow, .blue, .purple, .cyan, .primary]),
        fonts: Fonts(
            body: .body, secondary: .callout, title: .headline, requestTitle: .body.bold(), headingLarge: .title3, heading: .headline,
            mono: .system(.callout, design: .monospaced), code: .system(.caption, design: .monospaced), meta: .caption,
            label: .caption2, labelTracking: 0))

    /// The background of text, as the system draws it.
    static var systemBackground: Color {
        #if canImport(AppKit)
        Color(nsColor: .textBackgroundColor)
        #else
        Color(uiColor: .systemBackground)
        #endif
    }

    /// The colour of a status: `ok`, `bad`, `running` or `attention`.
    func color(_ tone: Tone) -> Color {
        switch tone {
        case .ok: colors.ok
        case .bad: colors.bad
        case .running: colors.running
        case .attention: colors.attention
        }
    }
}

/// What a badge says about a status.
public enum Tone {
    case ok, bad, running, attention

    /// The tone a word of status has: done, failed, waiting for the user, or anything else running.
    public init(status: String) {
        switch status {
        case "completed", "idle", "exit 0": self = .ok
        case "failed", "cancelled": self = .bad
        case "requires_action", "needs you": self = .attention
        default: self = status.hasPrefix("exit") ? .bad : .running
        }
    }
}

private struct ThemeKey: EnvironmentKey {
    static let defaultValue = ArchipelTheme.standard
}

extension EnvironmentValues {
    /// The kit's look. Set it with `.archipelTheme(_:)`.
    public var archipelTheme: ArchipelTheme {
        get { self[ThemeKey.self] }
        set { self[ThemeKey.self] = newValue }
    }
}

extension View {
    /// Draws the kit's views below with this theme: its colours, type and controls, and its
    /// light or dark when it has one.
    public func archipelTheme(_ theme: ArchipelTheme) -> some View {
        modifier(ThemeModifier(theme: theme))
    }

    /// A button in the theme: the primary one of its group, or another.
    func archipelButton(primary: Bool = false) -> some View {
        modifier(ThemedButton(primary: primary))
    }

    /// A text field in the theme.
    func archipelField() -> some View {
        modifier(ThemedField())
    }
}

private struct ThemeModifier: ViewModifier {
    let theme: ArchipelTheme
    func body(content: Content) -> some View {
        if let scheme = theme.colorScheme {
            styled(content).environment(\.colorScheme, scheme)
        } else {
            styled(content)
        }
    }

    private func styled(_ content: Content) -> some View {
        content
            .environment(\.archipelTheme, theme)
            .font(theme.fonts.body)
            .tint(theme.colors.accent)
    }
}

private struct ThemedButton: ViewModifier {
    let primary: Bool
    @Environment(\.archipelTheme) private var theme
    func body(content: Content) -> some View {
        if theme.drawsControls {
            content.buttonStyle(DrawnButtonStyle(primary: primary, theme: theme))
        } else if primary {
            content.buttonStyle(.borderedProminent)
        } else {
            content
        }
    }
}

/// A button drawn from the theme: the accent with its own ink when primary, a raised fill otherwise.
struct DrawnButtonStyle: ButtonStyle {
    let primary: Bool
    let theme: ArchipelTheme
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: theme.controlRadius)
        configuration.label
            .font(theme.fonts.body.weight(primary ? .semibold : .medium))
            .foregroundStyle(primary ? theme.colors.onAccent : theme.colors.text)
            .padding(.horizontal, 14).padding(.vertical, 7)
            .background(primary ? theme.colors.accent : theme.colors.raised, in: shape)
            .overlay(shape.stroke(primary ? Color.clear : theme.colors.line))
            .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.5)
            .contentShape(shape)
    }
}

private struct ThemedField: ViewModifier {
    @Environment(\.archipelTheme) private var theme
    func body(content: Content) -> some View {
        if theme.drawsControls {
            let shape = RoundedRectangle(cornerRadius: theme.controlRadius)
            content
                .textFieldStyle(.plain)
                .padding(.horizontal, 12).padding(.vertical, 9)
                .background(theme.colors.sunken, in: shape)
                .overlay(shape.stroke(theme.colors.line))
        } else {
            content.textFieldStyle(.roundedBorder)
        }
    }
}

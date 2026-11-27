import ArchipelUI
import SwiftUI

// The Minitel theme for the SwiftUI kit: the look recorded in ui/themes/minitel/DESIGN.md, the
// same colours and sizes as react/styles.css. #282C34, the terminal's own background, is the
// main one, so a session reads as one surface with the terminal beside it.

extension ArchipelTheme {
    /// Minitel, dark by default as in the app; `.light` is the light version of the same colours.
    ///
    ///     LiveChatView(session: session).archipelTheme(.minitel())
    public static func minitel(_ scheme: ColorScheme = .dark) -> ArchipelTheme {
        let dark = scheme == .dark
        func color(_ onDark: UInt32, _ onLight: UInt32) -> Color { Color(hex: dark ? onDark : onLight) }
        let text = color(0xE6E9EF, 0x16191D)
        let raised = color(0x3A3F4B, 0xE4E7EE)
        return ArchipelTheme(
            colors: Colors(
                text: text,
                secondary: color(0xA9B0BF, 0x545B6B),
                background: color(0x282C34, 0xF3F4F7),
                card: color(0x30343E, 0xFFFFFF),
                raised: raised,
                message: raised,
                onMessage: text,
                sunken: color(0x16191D, 0xEAECF1),
                line: color(0x444A58, 0xD3D7E0),
                edge: Color(hex: 0x868EA2),
                accent: color(0x8799DE, 0x3F55B0),
                onAccent: color(0x16191D, 0xFFFFFF),
                ok: color(0x59C780, 0x1D7A45),
                bad: color(0xF2777A, 0xB33833),
                // Purple: running, and the selected session. Amber: only what waits on the user.
                running: color(0xBA8AF5, 0x7A3FC9),
                attention: color(0xF2A640, 0x8A5300),
                ansi: (dark ? ansiDark : ansiLight).map { Color(hex: $0) }),
            // Nothing small: body 16, code and secondary 15, labels, badges and details 14.
            fonts: Fonts(
                body: .system(size: 16),
                secondary: .system(size: 15),
                title: .system(size: 18, weight: .semibold),
                requestTitle: .system(size: 17, weight: .semibold),
                headingLarge: .system(size: 19, weight: .semibold),
                heading: .system(size: 17, weight: .semibold),
                mono: .system(size: 15, design: .monospaced),
                code: .system(size: 15, design: .monospaced),
                meta: .system(size: 14),
                label: .system(size: 14, weight: .semibold),
                labelTracking: 0.7),
            badge: .tinted,
            cardRadius: 10,
            controlRadius: 8,
            cardPadding: EdgeInsets(top: 12, leading: 16, bottom: 12, trailing: 16),
            rowSpacing: 14,
            waitingBorder: 2,
            waitingFill: 0.08,
            drawsControls: true,
            settingsControlSize: .regular,
            folderLabels: true,
            colorScheme: scheme,
            columnWidth: 616,
            chatWidth: 1320)
    }

    private static let ansiDark: [UInt32] = [
        0x3A3F4B, 0xF2777A, 0x59C780, 0xF2A640, 0x8799DE, 0xBA8AF5, 0x56C2D6, 0xC8CDD8,
        0x868EA2, 0xFF9A9C, 0x84DDA4, 0xF7C47A, 0xAAB7EC, 0xD2B1F9, 0x86DCEA, 0xFFFFFF,
    ]
    private static let ansiLight: [UInt32] = [
        0x16191D, 0xB33833, 0x1D7A45, 0x8A5300, 0x3F55B0, 0x7A3FC9, 0x12707F, 0x545B6B,
        0x6B7385, 0xC9463F, 0x23914F, 0x9E6200, 0x4F66C7, 0x8E52DA, 0x16879A, 0x2B2F37,
    ]
}

extension Color {
    /// A colour from its 0xRRGGBB value, in sRGB.
    init(hex: UInt32) {
        self.init(.sRGB, red: Double(hex >> 16 & 0xFF) / 255, green: Double(hex >> 8 & 0xFF) / 255, blue: Double(hex & 0xFF) / 255)
    }
}

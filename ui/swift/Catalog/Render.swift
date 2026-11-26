import AppKit
import ArchipelMinitel
import ArchipelUI
import SwiftUI

/// The themes the catalogue knows, by the name given after `--theme`.
@MainActor
func theme(named name: String?, _ scheme: ColorScheme) -> ArchipelTheme {
    switch name {
    case "minitel": .minitel(scheme)
    default: .standard
    }
}

/// Draws a view in a window that is never shown, so real controls (fields, menus, switches)
/// are drawn as they are on screen, and returns it as a PNG.
@MainActor
func snapshot(_ content: AnyView, width: CGFloat, theme: ArchipelTheme, scheme: ColorScheme) -> Data? {
    let ground = theme.colors.background ?? Color(nsColor: .windowBackgroundColor)
    let root = content
        .padding(16)
        .frame(width: width, alignment: .topLeading)
        .background(ground)
        .archipelTheme(theme)
        .environment(\.colorScheme, scheme)
    let host = NSHostingView(rootView: root)
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: 200), styleMask: [.borderless], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.appearance = NSAppearance(named: scheme == .dark ? .darkAqua : .aqua)
    window.contentView = host
    defer { window.close() }
    // Let the views settle: lists lay out their rows, tasks (such as finding mentions) run.
    for _ in 0..<3 {
        let size = host.fittingSize
        window.setContentSize(size)
        host.frame = NSRect(origin: .zero, size: size)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.08))
    }
    // At twice the size, as on a Retina screen; the page shows it at its size in points.
    let size = host.bounds.size
    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2), bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
    else { return nil }
    rep.size = size
    host.cacheDisplay(in: host.bounds, to: rep)
    return rep.representation(using: .png, properties: [:])
}

/// "#RRGGBB" of a colour as the theme draws it in that scheme.
@MainActor
func hex(_ color: Color, _ scheme: ColorScheme) -> String {
    var environment = EnvironmentValues()
    environment.colorScheme = scheme
    let resolved = color.resolve(in: environment)
    func byte(_ value: Float) -> Int { Int((min(max(value, 0), 1) * 255).rounded()) }
    let alpha = resolved.opacity < 0.999 ? String(format: " · %d%%", Int((resolved.opacity * 100).rounded())) : ""
    return String(format: "#%02X%02X%02X", byte(resolved.red), byte(resolved.green), byte(resolved.blue)) + alpha
}

/// The colour as CSS, keeping its transparency, for a swatch.
@MainActor
func css(_ color: Color, _ scheme: ColorScheme) -> String {
    var environment = EnvironmentValues()
    environment.colorScheme = scheme
    let resolved = color.resolve(in: environment)
    func byte(_ value: Float) -> Int { Int((min(max(value, 0), 1) * 255).rounded()) }
    return "rgba(\(byte(resolved.red)), \(byte(resolved.green)), \(byte(resolved.blue)), \(resolved.opacity))"
}

/// Writes the catalogue page and its pictures into `out`, and returns the page's path.
@MainActor
func writeCatalog(themeName: String?, out: URL) throws -> URL {
    let name = themeName ?? "standard"
    let pictures = out.appendingPathComponent("catalog-\(name)")
    try FileManager.default.createDirectory(at: pictures, withIntermediateDirectories: true)
    // The theme's default first: Minitel is dark first, the system look follows the system, so light.
    let schemes: [ColorScheme] = themeName == "minitel" ? [.dark, .light] : [.light, .dark]
    let darkChrome = schemes.first == .dark
    func label(_ scheme: ColorScheme) -> String { scheme == .dark ? "dark" : "light" }
    func escape(_ text: String) -> String {
        text.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
    }

    var count = 0
    var body = ""
    for section in catalog {
        body += "<section id=\"\(section.id)\" class=\"cat-section\"><h2>\(escape(section.title))</h2><p class=\"cat-intro\">\(escape(section.intro))</p><div class=\"cat-grid\">"
        for specimen in section.specimens {
            let slug = specimen.name.lowercased().replacingOccurrences(of: "[^a-z0-9]+", with: "-", options: .regularExpression)
            var panes = ""
            for scheme in schemes {
                let file = "\(slug)-\(label(scheme)).png"
                if let png = snapshot(specimen.draw(), width: specimen.width, theme: theme(named: themeName, scheme), scheme: scheme) {
                    try png.write(to: pictures.appendingPathComponent(file))
                }
                panes += "<div class=\"cat-pane cat-\(label(scheme))\"><div class=\"cat-label\">\(label(scheme))</div><img src=\"catalog-\(name)/\(file)\" width=\"\(Int(specimen.width))\" alt=\"\(escape(specimen.name)), \(label(scheme))\"></div>"
            }
            body += """
            <article class="cat-specimen\(specimen.wide ? " cat-wide" : "")" id="\(slug)">
              <header><h3>\(escape(specimen.name))</h3><code class="cat-code">\(escape(specimen.code))</code>\(specimen.note.map { "<p>\(escape($0))</p>" } ?? "")</header>
              <div class="cat-pair">\(panes)</div>
            </article>
            """
            count += 1
        }
        body += "</div></section>"
    }

    // The theme's colours, as each scheme draws them.
    let themes = schemes.map { theme(named: themeName, $0) }
    let roles: [(String, String, (ArchipelTheme.Colors) -> Color?)] = [
        ("text", "Text", { $0.text }), ("secondary", "Secondary text, labels", { $0.secondary }), ("background", "Behind the chat", { $0.background }),
        ("card", "Cards", { $0.card }), ("raised", "Chips, menus, buttons", { $0.raised }), ("message", "The user's message", { $0.message }),
        ("sunken", "Code, diffs, fields", { $0.sunken }), ("line", "Borders", { $0.line }), ("edge", "Meter, guide line", { $0.edge }),
        ("accent", "Primary button, links", { $0.accent }), ("onAccent", "Text on the accent", { $0.onAccent }), ("ok", "Done, added lines", { $0.ok }),
        ("bad", "Failed, removed lines", { $0.bad }), ("running", "Running", { $0.running }), ("attention", "Waiting for the user", { $0.attention }),
    ]
    var swatches = ""
    for (key, role, pick) in roles {
        let values = zip(themes, schemes).map { theme, scheme in pick(theme.colors).map { hex($0, scheme) } ?? "the window's" }
        let chips = zip(themes, schemes).map { theme, scheme in
            "<span class=\"cat-chip-\(label(scheme))\"><span style=\"background:\(pick(theme.colors).map { css($0, scheme) } ?? "transparent")\"></span></span>"
        }.joined()
        swatches += "<div class=\"cat-swatch\"><div class=\"cat-chip-pair\">\(chips)</div><div><code>colors.\(key)</code><div class=\"cat-role\">\(role)</div><div class=\"cat-hex\">\(values.joined(separator: " · "))</div></div></div>"
    }

    let title = "Archipel components · SwiftUI\(themeName.map { " · \($0)" } ?? "")"
    let page = """
    <!doctype html>
    <html lang="en">
    <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>\(title)</title>
    <style>
    :root { \(darkChrome
        ? "color-scheme: dark; --fg: #E6E9EF; --muted: #A9B0BF; --bg: #16191D; --card: #1E2127; --line: #3A3F4B; --dark-pane: #282C34; --light-pane: #F3F4F7;"
        : "--fg: #1c1c1e; --muted: #5f5f66; --bg: #f7f7f9; --card: #ffffff; --line: #e2e2e7; --dark-pane: #1e1e1e; --light-pane: #ffffff;") }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    .cat-top { position: sticky; top: 0; z-index: 10; padding: 12px 24px; background: color-mix(in srgb, var(--bg) 92%, transparent); backdrop-filter: blur(8px); border-bottom: 1px solid var(--line); }
    .cat-top h1 { margin: 0 0 4px; font-size: 18px; }
    .cat-top nav { display: flex; flex-wrap: wrap; gap: 2px 14px; font-size: 14px; }
    .cat-top a { color: var(--muted); text-decoration: none; } .cat-top a:hover { color: var(--fg); }
    main { max-width: 1900px; margin: 0 auto; padding: 8px 24px 64px; }
    .cat-lede, .cat-intro { color: var(--muted); max-width: 64ch; text-wrap: pretty; }
    .cat-section { scroll-margin-top: 110px; padding-top: 24px; }
    .cat-section h2 { margin: 24px 0 6px; font-size: 26px; }
    .cat-section > h3 { margin: 28px 0 10px; font-size: 14px; color: var(--muted); text-transform: uppercase; letter-spacing: 0.05em; }
    .cat-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 1120px), 1fr)); gap: 32px 24px; }
    .cat-specimen { scroll-margin-top: 110px; min-width: 0; }
    .cat-specimen header { padding: 4px 0 10px; }
    .cat-specimen header h3 { margin: 0; font-size: 16px; }
    .cat-specimen header p { margin: 4px 0 0; color: var(--muted); font-size: 14px; }
    .cat-code { display: block; margin-top: 4px; font: 14px ui-monospace, Menlo, monospace; color: var(--muted); overflow-wrap: anywhere; }
    .cat-pair { display: flex; flex-wrap: wrap; }
    .cat-pane { position: relative; padding: 32px 0 0; overflow-x: auto; }
    .cat-pane img { display: block; max-width: none; height: auto; }
    .cat-pane.cat-light { background: var(--light-pane); } .cat-pane.cat-dark { background: var(--dark-pane); }
    .cat-label { position: absolute; top: 6px; left: 12px; font: 14px ui-monospace, Menlo, monospace; text-transform: uppercase; letter-spacing: 0.06em; }
    .cat-light .cat-label { color: #545B6B; } .cat-dark .cat-label { color: #A9B0BF; }
    .cat-swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 10px; }
    .cat-swatch { display: flex; gap: 10px; align-items: center; padding: 8px; background: var(--card); border: 1px solid var(--line); border-radius: 10px; }
    .cat-swatch code { font: 14px ui-monospace, Menlo, monospace; }
    .cat-chip-pair { display: flex; flex: none; border-radius: 8px; overflow: hidden; border: 1px solid var(--line); }
    .cat-chip-pair > span { width: 28px; height: 40px; }
    .cat-chip-pair > span > span { display: block; width: 100%; height: 100%; }
    .cat-chip-light { background: var(--light-pane); } .cat-chip-dark { background: var(--dark-pane); }
    .cat-role { font-size: 14px; } .cat-hex { font: 14px ui-monospace, Menlo, monospace; color: var(--muted); }
    </style>
    </head>
    <body>
    <header class="cat-top"><h1>\(title)</h1><nav><a href="#foundations">Foundations</a>\(catalog.map { "<a href=\"#\($0.id)\">\(escape($0.title))</a>" }.joined())</nav></header>
    <main>
    <p class="cat-lede">Every view of <code>ArchipelUI</code> in each of its states, drawn by SwiftUI in a window that is never shown, \(label(schemes[0])) and \(label(schemes[1])) side by side. \(count) specimens, with the same data as the React catalogue. Generated by <code>swift run ArchipelCatalog\(themeName.map { " --theme \($0)" } ?? "")</code>; <code>--live</code> shows them in a window, where they can be used.</p>
    <section id="foundations" class="cat-section"><h2>Foundations</h2><p class="cat-intro">The colours of <code>ArchipelTheme\(themeName.map { ".\($0)()" } ?? ".standard")</code>, as each scheme draws them: \(label(schemes[0])), then \(label(schemes[1])).</p><h3>Colour</h3><div class="cat-swatches">\(swatches)</div></section>
    \(body)
    </main>
    </body>
    </html>
    """
    let file = out.appendingPathComponent(themeName.map { "catalog-\($0).html" } ?? "catalog.html")
    try page.write(to: file, atomically: true, encoding: .utf8)
    print("wrote \(count) specimens to \(file.path)")
    return file
}

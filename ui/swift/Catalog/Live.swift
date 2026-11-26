import AppKit
import ArchipelUI
import SwiftUI

/// The catalogue in a window: the same specimens, live, so they can be clicked, typed into and
/// opened. The toolbar chooses the theme and the scheme.
struct CatalogApp: App {
    @State private var section: String? = catalog.first?.id
    @State private var themeName = CommandLine.arguments.firstIndex(of: "--theme").flatMap { at in
        CommandLine.arguments.indices.contains(at + 1) ? CommandLine.arguments[at + 1] : nil
    } ?? "standard"
    @State private var dark = true

    init() {
        // Started from a terminal there is no app bundle: without this the window gets no keyboard.
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)
    }

    var body: some Scene {
        WindowGroup("Archipel components") {
            let scheme: ColorScheme = dark ? .dark : .light
            let theme = theme(named: themeName, scheme)
            NavigationSplitView {
                List(catalog, selection: $section) { Text($0.title).tag($0.id) }
                    .navigationSplitViewColumnWidth(220)
            } detail: {
                ScrollView {
                    VStack(alignment: .leading, spacing: 28) {
                        if let current = catalog.first(where: { $0.id == section }) {
                            Text(current.title).font(.system(size: 26, weight: .bold))
                            Text(current.intro).foregroundStyle(.secondary)
                            ForEach(current.specimens) { specimen in
                                VStack(alignment: .leading, spacing: 8) {
                                    Text(specimen.name).font(.headline)
                                    Text(specimen.code).font(.system(size: 13, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
                                    if let note = specimen.note { Text(note).font(.callout).foregroundStyle(.secondary) }
                                    specimen.draw()
                                        .padding(16)
                                        .frame(width: specimen.width, alignment: .topLeading)
                                        .background(theme.colors.background ?? Color(nsColor: .windowBackgroundColor))
                                        .archipelTheme(theme)
                                        .environment(\.colorScheme, scheme)
                                        .id("\(themeName)-\(dark)-\(specimen.id)")
                                }
                            }
                        }
                    }
                    .padding(24)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .toolbar {
                Picker("Theme", selection: $themeName) {
                    Text("Standard").tag("standard")
                    Text("Minitel").tag("minitel")
                }
                .pickerStyle(.segmented)
                Toggle(isOn: $dark) { Label("Dark", systemImage: "moon") }
            }
            .frame(minWidth: 1100, minHeight: 760)
        }
    }
}

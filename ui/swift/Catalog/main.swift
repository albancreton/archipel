import AppKit
import Foundation

// Every view of ArchipelUI in each of its states: a page of pictures, or a live window.
//
//   swift run ArchipelCatalog                      writes ui/swift/Catalog/out/catalog.html (the system look)
//   swift run ArchipelCatalog --theme minitel      writes ui/swift/Catalog/out/catalog-minitel.html
//   swift run ArchipelCatalog --live               opens the catalogue in a window (theme and scheme in its toolbar)
//   --out <folder>                                 writes elsewhere

let arguments = CommandLine.arguments
func value(after flag: String) -> String? {
    arguments.firstIndex(of: flag).flatMap { arguments.indices.contains($0 + 1) ? arguments[$0 + 1] : nil }
}

if arguments.contains("--live") {
    CatalogApp.main()
} else {
    let out = value(after: "--out").map { URL(fileURLWithPath: $0) }
        ?? URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("out")
    MainActor.assumeIsolated {
        _ = NSApplication.shared
        do {
            try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
            _ = try writeCatalog(themeName: value(after: "--theme"), out: out)
        } catch {
            FileHandle.standardError.write(Data("ArchipelCatalog: \(error)\n".utf8))
            exit(1)
        }
    }
}

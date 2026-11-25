// swift-tools-version: 6.0
// The Swift packages of this repository: the client SDK and the SwiftUI component kit.
// Sources live in clients/swift and ui/swift; this file is at the root because that is where
// Swift Package Manager looks for it.

import PackageDescription

let package = Package(
    name: "Archipel",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "ArchipelClient", targets: ["ArchipelClient"]),
        .library(name: "ArchipelUI", targets: ["ArchipelUI"]),
    ],
    targets: [
        .target(
            name: "ArchipelClient",
            path: "clients/swift/Sources/ArchipelClient",
            resources: [.copy("Resources/state-rules.json")]
        ),
        .target(name: "ArchipelUI", dependencies: ["ArchipelClient"], path: "ui/swift/Sources/ArchipelUI"),
        .executableTarget(name: "ArchipelChatDemo", dependencies: ["ArchipelClient", "ArchipelUI"], path: "ui/swift/Demo"),
        // An example to copy, kept compiling and tested: start the daemon from a macOS app.
        .target(name: "DaemonExample", dependencies: ["ArchipelClient"], path: "examples/swift", exclude: ["Tests"]),
        .testTarget(name: "ExamplesTests", dependencies: ["DaemonExample", "ArchipelClient"], path: "examples/swift/Tests"),
        .testTarget(name: "ArchipelClientTests", dependencies: ["ArchipelClient"], path: "clients/swift/Tests/ArchipelClientTests"),
        .testTarget(name: "ArchipelUITests", dependencies: ["ArchipelUI"], path: "ui/swift/Tests/ArchipelUITests"),
    ],
    swiftLanguageModes: [.v5]
)

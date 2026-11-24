// swift-tools-version: 6.0
// The Swift packages of this repository: the client SDK and the SwiftUI component kit.
// Sources live in clients/swift and ui/swift; this file is at the root because that is where
// Swift Package Manager looks for it.

import PackageDescription

let package = Package(
    name: "HarnessBridge",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "BridgeClient", targets: ["BridgeClient"]),
        .library(name: "BridgeUI", targets: ["BridgeUI"]),
    ],
    targets: [
        .target(
            name: "BridgeClient",
            path: "clients/swift/Sources/BridgeClient",
            resources: [.copy("Resources/state-rules.json")]
        ),
        .target(name: "BridgeUI", dependencies: ["BridgeClient"], path: "ui/swift/Sources/BridgeUI"),
        .executableTarget(name: "BridgeChatDemo", dependencies: ["BridgeClient", "BridgeUI"], path: "ui/swift/Demo"),
        // An example to copy, kept compiling and tested: start the daemon from a macOS app.
        .target(name: "DaemonExample", dependencies: ["BridgeClient"], path: "examples/swift", exclude: ["Tests"]),
        .testTarget(name: "ExamplesTests", dependencies: ["DaemonExample", "BridgeClient"], path: "examples/swift/Tests"),
        .testTarget(name: "BridgeClientTests", dependencies: ["BridgeClient"], path: "clients/swift/Tests/BridgeClientTests"),
        .testTarget(name: "BridgeUITests", dependencies: ["BridgeUI"], path: "ui/swift/Tests/BridgeUITests"),
    ],
    swiftLanguageModes: [.v5]
)

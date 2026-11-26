// swift-tools-version: 6.0
// A package of its own, so that the SDK does not carry an SSH library: see README.md.

import PackageDescription

let package = Package(
    name: "SSHExample",
    platforms: [.macOS(.v15), .iOS(.v18)],
    dependencies: [
        .package(name: "Archipel", path: "../.."),
        .package(url: "https://github.com/orlandos-nl/Citadel", from: "0.12.1"),
    ],
    targets: [
        .target(
            name: "SSHExample",
            dependencies: [.product(name: "ArchipelClient", package: "Archipel"), .product(name: "Citadel", package: "Citadel")]
        ),
        .testTarget(name: "SSHExampleTests", dependencies: ["SSHExample"]),
    ],
    swiftLanguageModes: [.v5]
)

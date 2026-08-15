// swift-tools-version: 5.9
import PackageDescription

// Everything that talks to the agent, kept apart from the SwiftUI app on
// purpose: this half is Foundation only, so it builds and its tests run on
// Linux without Xcode. It is also the half that carries what we know about the
// firmware's response shapes, which is the half worth testing.
let package = Package(
    name: "OpenU60Kit",
    platforms: [.iOS(.v16), .macOS(.v13)],
    products: [
        .library(name: "OpenU60Kit", targets: ["OpenU60Kit"])
    ],
    targets: [
        .target(name: "OpenU60Kit"),
        .testTarget(name: "OpenU60KitTests", dependencies: ["OpenU60Kit"]),
    ]
)

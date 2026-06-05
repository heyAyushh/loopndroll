// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "LooperMenuBar",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .executable(name: "LooperMenuBar", targets: ["LooperMenuBar"]),
        .library(name: "LooperMenuBarCore", targets: ["LooperMenuBarCore"]),
    ],
    targets: [
        .target(name: "LooperMenuBarCore"),
        .executableTarget(
            name: "LooperMenuBar",
            dependencies: ["LooperMenuBarCore"]
        ),
        .testTarget(
            name: "LooperMenuBarCoreTests",
            dependencies: ["LooperMenuBarCore"]
        ),
    ]
)

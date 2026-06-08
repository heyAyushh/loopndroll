// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "LooperMenuBar",
    platforms: [
        .macOS("15.0")
    ],
    products: [
        .executable(name: "LooperMenuBar", targets: ["LooperMenuBar"]),
        .library(name: "LooperMenuBarCore", targets: ["LooperMenuBarCore"]),
    ],
    dependencies: [
        .package(path: "../../swift/LooperRealtime")
    ],
    targets: [
        .target(name: "LooperMenuBarCore"),
        .executableTarget(
            name: "LooperMenuBar",
            dependencies: [
                "LooperMenuBarCore",
                .product(name: "LooperRealtime", package: "LooperRealtime"),
            ]
        ),
        .testTarget(
            name: "LooperMenuBarCoreTests",
            dependencies: ["LooperMenuBarCore"]
        ),
    ]
)

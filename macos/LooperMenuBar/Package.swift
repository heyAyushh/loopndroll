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
        .package(path: "../../swift/LooperClientCore")
    ],
    targets: [
        .target(
            name: "LooperMenuBarCore",
            dependencies: [
                .product(name: "LooperClientCore", package: "LooperClientCore"),
            ]
        ),
        .executableTarget(
            name: "LooperMenuBar",
            dependencies: [
                "LooperMenuBarCore",
                .product(name: "LooperClientCore", package: "LooperClientCore"),
            ]
        ),
        .testTarget(
            name: "LooperMenuBarCoreTests",
            dependencies: [
                "LooperMenuBarCore",
                .product(name: "LooperClientCore", package: "LooperClientCore"),
            ]
        ),
    ]
)

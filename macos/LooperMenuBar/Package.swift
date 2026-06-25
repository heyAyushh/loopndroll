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
        .package(path: "../../swift/LooperClientCore"),
        .package(path: "../../swift/LooperRealtime")
    ],
    targets: [
        .target(
            name: "LooperMenuBarCore",
            dependencies: [
                .product(name: "LooperClientCore", package: "LooperClientCore"),
                .product(name: "LooperRealtime", package: "LooperRealtime"),
            ]
        ),
        .executableTarget(
            name: "LooperMenuBar",
            dependencies: [
                "LooperMenuBarCore",
                .product(name: "LooperClientCore", package: "LooperClientCore"),
                .product(name: "LooperRealtime", package: "LooperRealtime"),
            ]
        ),
        .testTarget(
            name: "LooperMenuBarCoreTests",
            dependencies: [
                "LooperMenuBarCore",
                .product(name: "LooperClientCore", package: "LooperClientCore"),
                .product(name: "LooperRealtime", package: "LooperRealtime"),
            ]
        ),
    ]
)

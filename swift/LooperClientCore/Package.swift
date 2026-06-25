// swift-tools-version: 6.1

import PackageDescription

let package = Package(
    name: "LooperClientCore",
    platforms: [
        .iOS("18.0"),
        .macOS("15.0"),
    ],
    products: [
        .library(name: "LooperClientCore", targets: ["LooperClientCore"]),
    ],
    targets: [
        .binaryTarget(
            name: "LooperClientCoreFFI",
            path: "Frameworks/LooperClientCoreFFI.xcframework"
        ),
        .target(
            name: "LooperClientCore",
            dependencies: ["LooperClientCoreFFI"],
            swiftSettings: [
                .swiftLanguageMode(.v6),
            ]
        ),
        .testTarget(
            name: "LooperClientCoreTests",
            dependencies: ["LooperClientCore"],
            swiftSettings: [
                .swiftLanguageMode(.v6),
            ]
        ),
    ]
)

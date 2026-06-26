// swift-tools-version: 6.1

import PackageDescription

let package = Package(
    name: "LooperRealtime",
    platforms: [
        .iOS("18.0"),
        .macOS("15.0"),
    ],
    products: [
        .library(name: "LooperRealtime", targets: ["LooperRealtime"]),
    ],
    dependencies: [
        .package(path: "../LooperClientCore"),
    ],
    targets: [
        .target(
            name: "LooperRealtime",
            dependencies: [
                .product(name: "LooperClientCore", package: "LooperClientCore"),
            ],
            exclude: ["Generated"],
            swiftSettings: [
                .swiftLanguageMode(.v6),
            ]
        ),
        .testTarget(
            name: "LooperRealtimeTests",
            dependencies: ["LooperRealtime"]
        ),
    ]
)

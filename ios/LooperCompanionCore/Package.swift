// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "LooperCompanionCore",
    platforms: [
        .iOS(.v16),
        .macOS(.v13),
    ],
    products: [
        .library(name: "LooperCompanionCore", targets: ["LooperCompanionCore"]),
    ],
    targets: [
        .target(
            name: "LooperCompanionCore"
        ),
        .testTarget(
            name: "LooperCompanionCoreTests",
            dependencies: ["LooperCompanionCore"]
        ),
    ]
)
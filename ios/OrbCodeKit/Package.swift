// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "OrbCodeKit",
    platforms: [
        .iOS(.v16),
    ],
    products: [
        .library(name: "OrbCodeKit", targets: ["OrbCodeKit"]),
    ],
    targets: [
        .binaryTarget(
            name: "OrbCodeFFI",
            path: "Frameworks/OrbCodeFFI.xcframework"
        ),
        .target(
            name: "OrbCodeKit",
            dependencies: ["OrbCodeFFI"]
        ),
        .testTarget(
            name: "OrbCodeKitTests",
            dependencies: ["OrbCodeKit"]
        ),
    ]
)

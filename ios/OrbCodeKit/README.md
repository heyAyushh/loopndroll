# OrbCodeKit

Attachable Swift package for iOS apps that need to derive orb IDs, generate orb
PNGs, scan orb IDs, and verify orb images through the Rust core.

## Rebuild the xcframework

From the repository root:

```bash
bash scripts/build-orb-code-ios-package.sh
```

## Add to an iOS app

In Xcode:

1. Open your app project.
2. Add a local Swift package.
3. Choose `ios/OrbCodeKit`.
4. Import `OrbCodeKit` in Swift.

## Swift API

```swift
import OrbCodeKit

let orb = try OrbCodeKit.generateOrb(fromData: "https://example.com")
let derivedOrbID = try OrbCodeKit.deriveOrbID(from: "https://example.com")
let scannedOrbID = try OrbCodeKit.scanOrbID(fromPNG: orb.pngData)
let isValid = try OrbCodeKit.verifyOrb(fromPNG: orb.pngData)
```

This package targets camera-safe orb images by embedding a visible orb-native
ring payload inside the rendered orb card.

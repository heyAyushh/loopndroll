# AGENTS.md - LooperRealtime

## OVERVIEW

`swift/LooperRealtime` is the shared Swift gRPC client package used by iOS and macOS clients for realtime events and prompt delivery.

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Public client | `Sources/LooperRealtime/LooperRealtimeClient.swift` | Streams mobile/desktop events and sends prompts. |
| Client models | `Sources/LooperRealtime/LooperRealtimeModels.swift` | Endpoint and credential types. |
| Generated protobuf/gRPC | `Sources/LooperRealtime/Generated/` | Regenerate, never hand-edit. |
| Tests | `Tests/LooperRealtimeTests/` | Model and behavior tests for package-level changes. |
| Proto source | `../../crates/agent-control-plane/proto/looper/v1/control_plane.proto` | Rust-side protocol source. |

## CONVENTIONS

- Keep this package app-agnostic. iOS/macOS-specific connection policy belongs in the app packages.
- Preserve Swift 6 concurrency safety; do not hide sendability problems with unchecked wrappers unless the owning type really controls isolation.
- Generated files must stay aligned with the Rust proto via `scripts/generate-swift-grpc.sh`.
- A cold build may fetch remote gRPC/SwiftProtobuf packages; distinguish dependency fetch failures from code failures.
- Any protocol change must be verified with Rust proto/server side and both Apple clients that consume the package.

## ANTI-PATTERNS

- Do not edit `Generated/looper_v1_control_plane.pb.swift` or `Generated/looper_v1_control_plane.grpc.swift` by hand.
- Do not put companion-specific retry, cache, or route preference policy in this shared package.
- Do not add UI or Foundation/AppKit/UIKit dependencies here.

## COMMANDS

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperRealtime
bash scripts/generate-swift-grpc.sh
```

## TEST HOTSPOTS

- `Tests/LooperRealtimeTests/LooperRealtimeModelsTests.swift`: endpoint/model behavior.
- Consumers must also run `swift test --package-path macos/LooperMenuBar` and the relevant iOS verifier after public API changes.

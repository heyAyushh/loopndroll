# AGENTS.md - Swift Client Core Bridge

## OVERVIEW

`swift/LooperClientCore` is the Swift package wrapper for the Rust `looper-client-core` UniFFI boundary. It gives iOS and macOS typed Swift access to Rust-owned state-mini sync, local snapshots, and session command intents.

This package should stay thin. Behavior belongs in Rust client-core unless it is Swift API shape, Swift concurrency bridging, or package wiring.

## STRUCTURE

```text
swift/LooperClientCore/
├── Package.swift
├── Sources/LooperClientCore/
│   ├── LooperClientCoreSessionManager.swift   # human-owned Swift facade
│   └── Generated/                             # generated UniFFI Swift and headers
├── Frameworks/LooperClientCoreFFI.xcframework # generated binary target
└── Tests/LooperClientCoreTests/
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Swift package wiring | `Package.swift` | Binary target plus Swift facade target. |
| Human-owned API wrapper | `Sources/LooperClientCore/LooperClientCoreSessionManager.swift` | Keep signatures ergonomic but behavior-thin. |
| Generated Swift bindings | `Sources/LooperClientCore/Generated/` | No-edit; regenerated from Rust UniFFI. |
| Generated binary target | `Frameworks/LooperClientCoreFFI.xcframework` | No-edit; rebuilt by the repo script. |
| Bridge tests | `Tests/LooperClientCoreTests/` | API surface and realtime bridge smoke coverage. |

## CONVENTIONS

- Regenerate this package from the repo root with `scripts/build-looper-client-core-package.sh`.
- Keep Swift methods as narrow wrappers around `LooperClientCoreSessionRuntime`; do not duplicate reducers, local stores, command queues, or transport clients here.
- Use Swift concurrency only to adapt async Rust calls into app-friendly APIs.
- When Rust exported models change, update Swift call sites and tests in the same slice.
- Use the Xcode 27 beta `DEVELOPER_DIR` when running package tests for current Looper Apple proof.

## ANTI-PATTERNS

- Do not hand-edit `Sources/LooperClientCore/Generated` or `Frameworks/LooperClientCoreFFI.xcframework`.
- Do not add HTTP command routes, event-stream clients, SQLite ownership, or session-control truth in Swift.
- Do not make iOS/macOS call generated raw objects directly when a stable facade method belongs in `LooperClientCoreSessionManager`.

## COMMANDS

```bash
bash scripts/build-looper-client-core-package.sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore
```

## TEST HOTSPOTS

- `Tests/LooperClientCoreTests/LooperClientCoreTests.swift`: session manager API and local runtime behavior.
- `Tests/LooperClientCoreTests/LooperClientCoreRealtimeBridgeTests.swift`: bridge-level realtime/state-mini behavior.

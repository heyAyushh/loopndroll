# PROJECT KNOWLEDGE BASE

**Generated:** 2026-06-23
## OVERVIEW

Looper keeps AI coding agent sessions moving after supported agents stop. The product is a Rust control plane with native macOS and iOS clients; the root `pnpm` package is only an orchestration wrapper.

## STRUCTURE

```text
looper/
├── crates/agent-control-plane/   # Rust server, CLI, TUI, hooks, sessions, mobile API
├── crates/orb-code/              # Rust orb-code generator/decoder and iOS staticlib
├── ios/                          # iPhone app, iOS Swift packages, app/UI tests
├── macos/LooperMenuBar/          # native menu bar app that bundles Rust binaries
├── swift/LooperRealtime/         # shared Swift gRPC/realtime client package
├── scripts/                      # install, package, release, generation, validation scripts
└── docs/                         # authored docs, QA matrices, migration notes
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Rust server, CLI, TUI, hooks, auth, sessions, scheduler, mobile API | `crates/agent-control-plane` | Source of truth for backend/control-plane behavior. |
| Zed/Devin/ACP host control | `crates/agent-control-plane/src/acp`, `src/zed`, `src/devin`, `src/control_plane/acp_hosts.rs` | Keep host routes and runtime identity in Rust. |
| iPhone companion state, search, scanner, App Intents | `ios/LooperCompanion`, `ios/LooperCompanionCore` | Native SwiftUI client of Rust HTTP/SSE/gRPC APIs. |
| macOS menu bar lifecycle, settings, diagnostics | `macos/LooperMenuBar` | Native client that launches bundled Rust binaries. |
| Shared realtime gRPC client | `swift/LooperRealtime` | Used by iOS and macOS; generated files live under `Sources/LooperRealtime/Generated`. |
| Orb pairing FFI | `crates/orb-code`, `ios/OrbCodeKit` | Rebuild XCFramework with `scripts/build-orb-code-ios-package.sh`. |
| Packaging and install | `scripts/build-macos-menu-bar-package.sh`, `scripts/install-looper-cli.sh` | High-impact scripts; inspect targets before running. |
| Release | `scripts/release-macos.sh` | Reads local `.env`, requires signing/notarization env, refuses dirty tree unless explicitly overridden. |
| QA/proof docs | `docs/qa`, `GOAL*.md` | Record observed control surfaces, not speculative status. |

## CODE MAP

| Symbol | Type | Location | Refs | Role |
| --- | --- | --- | --- | --- |
| `ControlPlane` | Rust struct | `crates/agent-control-plane/src/control_plane.rs:162` | central | SQLite-backed state owner for sessions, hooks, mobile state, events, settings. |
| `run_server` | Rust function | `crates/agent-control-plane/src/runtime.rs` | 1 caller | Server/hook runtime entry behind `looper-server` and wrapper binary. |
| `CompanionAppModel` | Swift class | `ios/LooperCompanion/App/CompanionAppModel.swift:24` | central | iPhone snapshot, connectivity, realtime, search, Siri/open-url state. |
| `loadSnapshot` | Swift method | `ios/LooperCompanion/App/CompanionAppModel.swift` | 6 callers | Main iPhone refresh path; keep concurrency and revision guards intact. |
| `BundledControlPlaneService` | Swift class | `macos/LooperMenuBar/Sources/LooperMenuBarCore/LooperLifecycleCoordinator.swift:21` | lifecycle | Starts/stops embedded Rust server and filters launch env. |
| `LooperRealtimeClient` | Swift class | `swift/LooperRealtime/Sources/LooperRealtime/LooperRealtimeClient.swift:6` | shared | gRPC streams for mobile events, desktop events, and prompt delivery. |

## CONVENTIONS

- Rust is the backend and control-plane source of truth. Do not add core backend, auth, mobile API, hook, notification, or session-control behavior outside `crates/agent-control-plane`.
- Keep macOS, iOS, and TUI surfaces as clients of the Rust HTTP/SSE/gRPC API.
- Local-first is the default. Tailscale-style reachable URLs are a remote-control boundary; hosted/cloud paths are not the default.
- `looper` is the primary terminal command. `looper-cli` exists for compatibility callers and should not become the preferred product surface.
- Apple projects are XcodeGen-driven. Update `ios/project.yml` or `macos/LooperMenuBar/project.yml` when target/package wiring changes, then regenerate and review the tracked project diff.
- Confirmed Apple toolchain for current iOS proof is Xcode 27 beta:
  `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`
  (`Xcode 27.0`, build `27A5194q`). Treat default `/Applications/Xcode.app`
  platform details as stale for Looper iOS verification.
- Swift generated gRPC files under `swift/LooperRealtime/Sources/LooperRealtime/Generated` are no-edit zones. Regenerate with `scripts/generate-swift-grpc.sh`.
- Treat `.omo/`, `.build/`, `build/`, `target/`, DerivedData, generated packages, and release artifacts as evidence/build output, not source.

## ANTI-PATTERNS (THIS PROJECT)

- Do not revive Bun/Electrobun/loopndroll as backend ownership. Legacy import paths may remain only as explicit migration compatibility.
- Do not silently edit user config files touched by Looper (`~/.codex/hooks.json`, `~/.codex/config.toml`, `~/.claude/settings.json`, `~/.grok/hooks/looper.json`, `~/.config/devin/config.json`) without a matching product path and verification.
- Do not hand-edit generated Swift protobuf/gRPC files.
- Do not call native Apple work done from build/test proof alone when installed-app, simulator, or paired-device behavior is the requested surface.
- Do not widen install/release actions beyond the repo script target; report exact target paths first for `/Applications`, device installs, or external config writes.

## COMMANDS

```bash
pnpm run dev:server
pnpm run dev:tui
pnpm run doctor
pnpm run dev:ios-api
pnpm run check:rust-control-plane
swift test --package-path macos/LooperMenuBar
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperRealtime
bash scripts/check-ios.sh
bash scripts/build-macos-menu-bar-package.sh --no-install
bash scripts/build-macos-menu-bar-xcode.sh --no-install
bash scripts/install-looper-cli.sh
bash scripts/release-macos.sh
```

## NOTES

- There is no root Cargo workspace; use each `--manifest-path` explicitly.
- No `.github/workflows` directory is present in this checkout. CI/release truth is in local scripts and manifests.
- iOS/macOS signing uses team `Z5454ZPPUX` in project specs; packaging also depends on local keychain/provisioning profile state.
- `scripts/check-ios.sh` is an App Intents/Siri gate, not just a compile check.
- The canonical main checkout is `/Users/ay/Documents/looper`; this worktree may be ahead of it.

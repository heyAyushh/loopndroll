# AGENTS.md - macOS Menu Bar

## OVERVIEW

`macos/LooperMenuBar` is the native menu bar client. It launches and controls bundled Rust binaries instead of opening a separate terminal window.

## STRUCTURE

```text
macos/LooperMenuBar/
├── project.yml                  # XcodeGen source of truth
├── Package.swift                # SwiftPM package for app/core/tests
├── Sources/LooperMenuBar/       # AppKit entrypoint and window controllers
├── Sources/LooperMenuBarCore/   # testable menu, client, lifecycle, diagnostics logic
├── Tests/LooperMenuBarCoreTests/
└── Generated/                   # tracked plist/entitlements generated from project spec
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| App entry/status item | `Sources/LooperMenuBar/main.swift` | App delegate, status item, menu refresh, diagnostics window. |
| Rust server lifecycle | `Sources/LooperMenuBarCore/LooperLifecycleCoordinator.swift` | Starts bundled `looper-server`, filters launch env, probes health. |
| HTTP/control client | `Sources/LooperMenuBarCore/ControlPlaneClient.swift` | Desktop/mobile/control-plane models and API calls. |
| Menu rendering/cache | `Sources/LooperMenuBarCore/MenuRefreshCoordinator.swift`, tests | Keep refresh throttling and cache behavior deterministic. |
| Diagnostics | `Sources/LooperMenuBarCore/LooperDiagnosticsContent.swift` | ACP/route/timing/classification report content. |
| Packaging | `../../scripts/build-macos-menu-bar-package.sh`, `../../scripts/build-macos-menu-bar-xcode.sh` | Embeds Rust release binaries into app bundle. |

## CONVENTIONS

- Update `project.yml` for target, package, entitlement, or build-setting changes; regenerate and review the tracked project diff.
- `LooperMenuBarCore` should stay testable and UI-light; AppKit wiring belongs in `Sources/LooperMenuBar`.
- The app depends on Rust release binaries `looper`, `looper-cli`, and `looper-server` in the bundle.
- System Settings links should be limited to permissions Looper actually needs.
- Notification target settings default to macOS; iPhone/Telegram/other targets should be dynamic, not hardcoded as permanent UI assumptions.

## ANTI-PATTERNS

- Do not bypass `BundledControlPlaneService` for server lifecycle.
- Do not move backend/session/mobile logic into the menubar client.
- Do not treat generated plist/entitlements as independent source of truth.
- Do not launch or replace `/Applications/looper.app` without an explicit install task and exact target path.

## COMMANDS

```bash
swift test --package-path macos/LooperMenuBar
xcodegen generate --spec macos/LooperMenuBar/project.yml --project macos/LooperMenuBar
bash scripts/build-macos-menu-bar-package.sh --no-install
bash scripts/build-macos-menu-bar-xcode.sh --no-install
```

## TEST HOTSPOTS

- `Tests/LooperMenuBarCoreTests/LooperMenuContentTests.swift`: menu sections and labels.
- `Tests/LooperMenuBarCoreTests/MenuRefreshCoordinatorTests.swift`: refresh/cache timing.
- `Tests/LooperMenuBarCoreTests/LooperLifecycleCoordinatorTests.swift`: bundled process launch and health.
- `Tests/LooperMenuBarCoreTests/LooperDiagnosticsContentTests.swift`: diagnostics report formatting.

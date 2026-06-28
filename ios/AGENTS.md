# AGENTS.md - iOS Companion

## OVERVIEW

`ios/` contains the iPhone companion app, its XcodeGen project, companion core package, OrbCodeKit package, app tests, and UI tests.

## STRUCTURE

```text
ios/
├── project.yml                    # XcodeGen source of truth
├── LooperCompanion/               # SwiftUI app target
├── LooperCompanionCore/           # shared iOS/macOS Swift package
├── OrbCodeKit/                    # binary FFI wrapper for orb-code
├── LooperCompanionTests/          # app-level tests
└── LooperCompanionUITests/        # simulator UI control-flow tests
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| App entry | `LooperCompanion/App/LooperCompanionApp.swift` | `@main` app, URL/activity handling, pending-open draining. |
| App state/realtime | `LooperCompanion/App/CompanionAppModel.swift`, `Services/` | Cached session minis, connectivity, Session stream sync, prompt mutations. |
| Models | `LooperCompanion/Models/CompanionModels.swift` | Keep in sync with Rust mobile API JSON. |
| Sessions UI | `LooperCompanion/UI/Sessions/` | Search, details, rows, device hub, management controls. |
| Settings/scanner | `LooperCompanion/UI/Settings/`, `UI/Scanner/` | Connection setup and diagnostics surfaces. |
| App Intents/Siri | `LooperCompanion/AppIntents/`, `LooperCompanionCore` | Current/default/session entity routing. |
| Core helpers | `LooperCompanionCore/Sources/LooperCompanionCore/` | URL routing, freshness, surface filtering, Siri entity support. |
| Realtime architecture cuts | `../.agents/skills/looper-realtime-cutter/SKILL.md`, `../docs/architecture/realtime-cutter-workflow.md` | Use when iOS symptoms require server/client-core/iOS/macOS/device-truth coordination. |
| Session sync and command ordering | `../.agents/skills/ios-session-sync-debugging/SKILL.md`, `LooperCompanion/Services/`, `../swift/LooperClientCore`, `../crates/looper-client-core` | Use for assistant switcher, Siri/default-session, state-mini, pending command, latest-wins, and stale async result bugs. |
| Primary app proof surface | `../.agents/skills/ios-browser-simulator-proof/SKILL.md` | Use XcodeBuildMCP + `serve-sim` + Codex in-app Browser before physical-phone install unless the behavior is device-only. |
| Simulator diagnostics | `../scripts/ios-diagnostics.sh`, `../.agents/skills/ios-perf-diagnostics/SKILL.md` | Use oslog-live, lldb-trap, perf-loop/xctrace, and ETTrace for latency, switcher, hang, and crash proof. |

## CONVENTIONS

- Treat the Rust mobile API as source of truth; iOS models should decode missing/new fields defensively.
- Update `ios/project.yml` when files, packages, build settings, or targets change; regenerate and review `LooperCompanion.xcodeproj`.
- Use Xcode 27 beta explicitly for iOS 27/device proof:
  `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`.
- Confirmed local toolchain detail: `Xcode 27.0` build `27A5194q`; default
  `/Applications/Xcode.app` details are stale for this repo's iOS proof.
- Keep UI-test-only branches behind `UITestLaunchArguments`; do not leak mock behavior into normal runtime.
- Keep first prompt, Codex title, work status, assistant surface, and launched-by/subagent metadata visible through models before UI.
- Use `../.agents/skills/looper-realtime-cutter/SKILL.md` before broad iOS realtime edits. Missing/stale session bugs must compare Codex sqlite truth, phone local store truth, Rust client-core projection, and iOS surface projection before patching UI.
- Use `../.agents/skills/ios-session-sync-debugging/SKILL.md` for assistant switcher and session command lifecycle bugs. Command queues, latest-wins coalescing, mutation identity, and stale-result policy belong in `SessionSyncEngine` or another Services-layer client manager, not ad hoc presenter tasks in `CompanionAppModel`.
- Use `../.agents/skills/ios-browser-simulator-proof/SKILL.md` as the default observable iOS workflow. The primary loop is XcodeBuildMCP simulator launch, `serve-sim` for that simulator UDID, and Codex in-app Browser proof. Physical iPhone install is second priority unless the user asks for the phone or the bug needs real-device behavior.
- Keep iOS diagnostic instrumentation on existing `CompanionDiagnostics` categories; capture artifacts belong under `build/ios-diagnostics/`.
- For simulator performance captures, pass `--device <simulator-udid>` to `scripts/ios-diagnostics.sh perf-loop` or `capture`; otherwise host processes with the same name can be profiled by mistake.

## ANTI-PATTERNS

- Do not call iPhone work done from SwiftPM tests alone when the requested surface is simulator/device behavior.
- Do not use static JPEG pages, standalone Chromium, or raw Playwright as iOS app proof when the Codex in-app Browser was requested.
- Do not reintroduce `AskLatestCodexSessionIntent`; `scripts/check-ios.sh` rejects stale intent names.
- Do not hardcode simulator/device assumptions outside test launch arguments or scripts.
- Do not edit `OrbCodeKit/Frameworks/OrbCodeFFI.xcframework` manually; rebuild it from Rust.

## COMMANDS

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
bash scripts/check-ios.sh
bash scripts/ios-diagnostics.sh doctor
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'generic/platform=iOS' build
xcodegen generate --spec ios/project.yml --project ios
bash scripts/build-orb-code-ios-package.sh
```

## TEST HOTSPOTS

- `LooperCompanionUITests/LooperCompanionControlFlowUITests.swift`: onboarding, scanner, sessions, settings, search, session control.
- `LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`: cached Session mini sync and local-first behavior.
- `LooperCompanionTests/SessionSummaryTimingTests.swift`: mobile timing display behavior.
- `LooperCompanionTests/SessionSummaryTimingTests.swift`: Siri entity wrapper projection and current/default session routing.
- `LooperCompanionCore/Tests/LooperCompanionCoreTests/LooperSessionFreshnessTests.swift`: activity ordering.

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

## CONVENTIONS

- Treat the Rust mobile API as source of truth; iOS models should decode missing/new fields defensively.
- Update `ios/project.yml` when files, packages, build settings, or targets change; regenerate and review `LooperCompanion.xcodeproj`.
- Use Xcode 27 beta explicitly for iOS 27/device proof:
  `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`.
- Confirmed local toolchain detail: `Xcode 27.0` build `27A5194q`; default
  `/Applications/Xcode.app` details are stale for this repo's iOS proof.
- Keep UI-test-only branches behind `UITestLaunchArguments`; do not leak mock behavior into normal runtime.
- Keep first prompt, Codex title, work status, assistant surface, and launched-by/subagent metadata visible through models before UI.

## ANTI-PATTERNS

- Do not call iPhone work done from SwiftPM tests alone when the requested surface is simulator/device behavior.
- Do not reintroduce `AskLatestCodexSessionIntent`; `scripts/check-ios.sh` rejects stale intent names.
- Do not hardcode simulator/device assumptions outside test launch arguments or scripts.
- Do not edit `OrbCodeKit/Frameworks/OrbCodeFFI.xcframework` manually; rebuild it from Rust.

## COMMANDS

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
bash scripts/check-ios.sh
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

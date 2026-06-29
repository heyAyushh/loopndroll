# Todo 10 client/UI transport demotion

## Changed files

- `ios/LooperCompanion/App/CompanionAppModel.swift`
  - Restores Rust-owned SessionMini cache before computing HTTP snapshot failure UI state.
  - Rejects local/recovered SessionMini snapshots with a lower seq than the known realtime seq, even when visible state was temporarily reset.
- `ios/LooperCompanion/Models/CompanionModels.swift`
  - Treats an empty `effectiveMode` string as nil when decoding `SessionSummary`, matching local mini payloads without dropping the whole local projection.
- `ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift`
  - Rejects corrupt placeholder mini projections so corruption falls through to allowed recovery instead of rendering fake visible truth.
- `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`
  - Adds regressions for stale local mini replay after visible reset and HTTP failure restoring local minis.

## Strict grep

Output: `.omo/evidence/local-first-multinode-architecture-lock/task-10-strict-grep.txt`

Remaining hits:

- `swift/LooperClientCore/Sources/LooperClientCore/Generated/looper_client_core.swift` lines 4017, 4052, 4088.
  - Classification: generated UniFFI enum text for `setAssistantSurface`; generated file was not edited; not product runtime HTTP/event-stream path.
- `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift` lines 244, 248, 298, 302, 372, 445, 486.
  - Classification: test-only assertions that assistant-surface switching does not enqueue `setAssistantSurface`; not product runtime path.

Product runtime hits for old event streams, mobile/desktop event routes, and HTTP command patterns: none.

## Tests

- PASS: `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' -only-testing:LooperCompanionTests/CompanionSessionMiniLocalFirstTests`
  - Evidence: `.omo/evidence/local-first-multinode-architecture-lock/task-10-ios-focused-tests.txt`
- PASS: `git diff --check` (re-run before staging; no output)

## Scope notes

- No Rust server files edited.
- No generated Swift bindings edited.
- No Pinball/Maze/game files edited.
- No installs and no broad gates run.

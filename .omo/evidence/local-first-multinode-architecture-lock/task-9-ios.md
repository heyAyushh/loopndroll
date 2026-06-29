# Todo 9 iOS Evidence

Repair Commit: 758fb3cce08c8ca49358bc5ae0c7d0dc4780fa0e

## Repair Changed Files

- `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`
- `.omo/evidence/local-first-multinode-architecture-lock/task-9-ios.md`

## Focused Commands

- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' '-only-testing:LooperCompanionTests/CompanionSessionMiniLocalFirstTests/testAssistantSurfaceSwitchPreservesLocalMiniSourceWithoutCommandsOrReload()'`
  - Passed. Ran one Swift Testing test: `testAssistantSurfaceSwitchPreservesLocalMiniSourceWithoutCommandsOrReload()`.
  - The test exercises `CompanionAppModel.selectAssistantSurface`, verifies the selected assistant surface changes locally, the visible session projection moves to the requested surface, the raw local mini source IDs stay unchanged, no pending Session command is enqueued, and the service load/health counters remain zero.
- `git diff --check`
  - Passed with no output.

## Scope Notes

- Assistant surface selection remains local UI projection only. The repair proof is now at the app-model seam, not only the pure `LooperCompanionCore` surface-filtering array helper.
- No standalone install, broad suite, manual simulator launch, route/connectivity file edit, macOS edit, Rust edit, generated Swift edit, or game file edit was performed.
- ETTrace and real-surface 100-switch profiler proof are deferred to Todo 11.

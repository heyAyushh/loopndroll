# Todo 9 iOS Evidence

Commit: 40f0b671fbcc3dab33612324e9d037bde5453598

## Changed Files

- `ios/LooperCompanion/App/CompanionAppModel.swift`
- `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/CompanionSurfaceFilteringTests.swift`
- `.omo/evidence/local-first-multinode-architecture-lock/task-9-ios.md`

## Focused Commands

- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter rapidSurfaceSwitchesKeepLocalMiniSourceStable`
  - Passed. Built `LooperCompanionCore` for debugging and ran one Swift Testing test: `Rapid surface switches keep local mini source stable`.
- `git diff --check`
  - Passed with no whitespace errors.

## Scope Notes

- Assistant surface selection remains local UI projection only. The app setter now routes through the same local selection method as the switcher action instead of bypassing it.
- The focused Core test exercises 100 rapid local surface projections against one unchanged mini source list and verifies visible IDs remain surface-filtered without mutating or collapsing the local source list.
- No install, simulator launch, route/connectivity file edit, macOS edit, Rust edit, generated Swift edit, or game file edit was performed.
- ETTrace and real-surface 100-switch profiler proof are deferred to Todo 11.

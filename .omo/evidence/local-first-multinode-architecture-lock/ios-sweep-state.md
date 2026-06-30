# iOS state/freshness sweep

Date: 2026-06-30
Scope: `ios/LooperCompanion/App`, `ios/LooperCompanion/Services`, `ios/LooperCompanion/Models`, `ios/LooperCompanionTests`

## Finding

Fixed one concrete route/liveness UI lie.

`CompanionSessionMiniController` emits a restart liveness event with `latestSeq == 0`, `isLive == false`, and `endpointURL == nil` when the state-mini stream exits before retrying (`ios/LooperCompanion/Services/CompanionSessionMiniController.swift:76`, `ios/LooperCompanion/Services/CompanionSessionMiniController.swift:83`). `CompanionAppModel.applyRealtimeStreamLiveness` rejected every liveness event with `latestSeq < realtimeLatestSeq`, so after any live stream update with a positive seq the restart event was skipped. Result: the app could keep `realtimeStreamIsLive == true`, `connectionState == .connected`, and a stale `activeSessionRouteBaseURL` visible while the stream was actually restarting.

Fix: allow only the restart sentinel to clear live route state without lowering `realtimeLatestSeq`; keep stale live updates rejected (`ios/LooperCompanion/App/CompanionAppModel.swift:435`, `ios/LooperCompanion/App/CompanionAppModel.swift:2155`).

Regression: `testStreamRestartLivenessClearsLiveRouteWithoutLoweringSeq` verifies restart liveness clears live state/route, returns to `.connecting`, and preserves the previous seq (`ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift:1311`). Existing `testStaleLivenessCannotResurrectConnectedState` still proves stale live updates cannot resurrect a connected route (`ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift:1286`).

## Read-only sweep notes

- Stale HTTP snapshot overwrites: existing guards already skip network snapshots once a local state-mini cursor/store is known and reject snapshot loads after connection revision changes (`ios/LooperCompanion/App/CompanionAppModel.swift:652`, `ios/LooperCompanion/App/CompanionAppModel.swift:669`, `ios/LooperCompanion/App/CompanionAppModel.swift:2022`).
- Cached snapshot overwrites: legacy cached snapshot restore already skips when state-mini evidence exists and respects `onlyWhenSnapshotMissing` plus restore revision (`ios/LooperCompanion/App/CompanionAppModel.swift:1920`).
- Assistant surface switching: local projection is view-only and does not dispatch runtime commands or reload snapshots (`ios/LooperCompanion/App/CompanionAppModel.swift:1725`, covered by existing tests around `CompanionSessionMiniLocalFirstTests.swift:241`).
- Broad AppModel invalidation/flicker: no concrete patch made. The state store already fingerprints visible projections before mutating visible snapshot state, and repeated liveness updates return `false` when unchanged (`ios/LooperCompanion/App/CompanionSnapshotStateStore.swift:277`, `ios/LooperCompanion/App/CompanionAppModel.swift:442`).

External reference used as background only: Apple SwiftUI performance guidance emphasizes proving unnecessary updates through Instruments and the relationship between data changes and view updates, so I avoided speculative view refactors without a local failing signal.

## Gates

Focused gates only. No full suites or installs.

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' -only-testing 'LooperCompanionTests/CompanionSessionMiniLocalFirstTests/testStreamRestartLivenessClearsLiveRouteWithoutLoweringSeq()'
```

Result: passed, Swift Testing executed 1 test.

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro' -only-testing 'LooperCompanionTests/CompanionSessionMiniLocalFirstTests/testStaleLivenessCannotResurrectConnectedState()'
```

Result: passed, Swift Testing executed 1 test.

```bash
git diff --check -- ios/LooperCompanion/App/CompanionAppModel.swift ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift
```

Result: passed.

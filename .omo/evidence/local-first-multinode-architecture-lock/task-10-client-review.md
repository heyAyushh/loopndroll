# Task 10 Client Code Review

Verdict: confirmed
codeQualityStatus: WATCH
recommendation: APPROVE
reportPath: `.omo/evidence/local-first-multinode-architecture-lock/task-10-client-review.md`
reviewedCommit: `0e6e585c3d400d713705ab427d21c0934a54cb4b`
plan: `.omo/plans/local-first-multinode-architecture-lock.md`
notepadPath: not provided in review input
blockers: []

## Review Boundary

- Reviewed Todo 10 client commit `0e6e585c3d400d713705ab427d21c0934a54cb4b` against plan Todo 10 acceptance at `.omo/plans/local-first-multinode-architecture-lock.md:182`.
- Changed files match the requested surface: `ios/LooperCompanion/App/CompanionAppModel.swift`, `ios/LooperCompanion/Models/CompanionModels.swift`, `ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift`, `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`, and task-10 evidence files.
- I did not edit source, install, or run broad gates. I did not rerun the Xcode test because the existing focused artifact is complete and within the allowed exact test surface.
- Web search was executed per repo instruction, but no external source was needed for any finding; this review is based on local plan, diff, source, grep, and test artifacts.

## Skill Perspective Check

- `remove-ai-slops`: unavailable as a named skill in the session skill list, so I applied the prompt criteria directly. No deletion-only, tautological, implementation-constant-only, or removal-only tests were found in the changed tests.
- `programming`: unavailable as a named skill in the session skill list, so I applied the prompt criteria directly. No brittle prompt tests, untyped escape hatches, needless abstraction, or unjustified production parsing were found.
- Residual watch: the Swift corrupt-placeholder filter in `ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift:127` is heuristic, but it is narrow enough for this task because compact minis with Rust's normal defaults keep `canSendPrompt == true`; the filter only rejects id-as-title/id-as-ref plus blank timestamps plus `canSendPrompt == false`.

## Evidence Checked

- Commit scope: `git diff-tree --no-commit-id --name-status -r 0e6e585c3d400d713705ab427d21c0934a54cb4b`.
- Whitespace: `git diff --check 0e6e585c3d400d713705ab427d21c0934a54cb4b^ 0e6e585c3d400d713705ab427d21c0934a54cb4b` returned clean.
- Strict grep: `.omo/evidence/local-first-multinode-architecture-lock/task-10-strict-grep.txt:1` and a live rerun of the exact plan grep both showed only generated UniFFI `setAssistantSurface` enum text and test assertions.
- Focused test artifact: `.omo/evidence/local-first-multinode-architecture-lock/task-10-ios-focused-tests.txt:349` shows `CompanionSessionMiniLocalFirstTests` passed 41 Swift Testing tests; line 358 shows `** TEST SUCCEEDED **`.
- Broader command-route check found desktop HTTP mutation routes, but `crates/agent-control-plane/src/http/mod.rs:251` wires archive to disabled handlers, `:255` and `:260` wire mute/delete to disabled handlers, and `crates/agent-control-plane/src/http/session_actions.rs:8` returns the disabled mutation response.

## Findings

### CRITICAL

None.

### HIGH

None.

### MEDIUM

None.

### LOW

None requiring changes. Watch item only: `containsCorruptFallbackSession` is a client-side heuristic in `ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift:127`, but I do not consider it a blocker for this commit because it protects against fake visible truth without rejecting normal compact minis.

## Acceptance Review

- HTTP snapshot success does not overwrite newer Session/local seq. `shouldApplyNetworkSnapshot` rejects network snapshots when `hasKnownSessionMiniCursor()` is true in `ios/LooperCompanion/App/CompanionAppModel.swift:1883`, and `testReconnectingSessionMiniTruthWinsOverSuccessfulHttpSnapshot` covers this path.
- HTTP failure restores local minis before failure projection. The catch path restores SessionMini cache before computing `hasUsableSnapshot` in `ios/LooperCompanion/App/CompanionAppModel.swift:615`; `testHttpSnapshotFailureRestoresLocalMinisAfterVisibleSnapshotReset` asserts local rows and `Local` status in `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift:1062`.
- Older local/recovery snapshots are rejected even when visible state was reset. `shouldApplyStateMiniSnapshot` rejects `latestSeq < realtimeLatestSeq` before the nil-snapshot fast path in `ios/LooperCompanion/App/CompanionAppModel.swift:1926`; `testOlderStateMiniCacheCannotReplayAfterVisibleSnapshotReset` covers it at `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift:1001`.
- UI does not say disconnected when usable local minis exist. `connectivityStatusLabel` returns `Local` for usable local state in `ios/LooperCompanion/App/CompanionAppViewState.swift:163`, with usable state defined at `:250`. The focused test artifact logs the underlying failure as offline but the test asserts the visible label remains `Local`.
- HTTP detail fallback is not authoritative in the reviewed iOS surface. `LooperSiriSessionClient.loadSessionDetail` returns local mini-backed detail or throws in `ios/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift:483`; `loadSnapshotLocalFirst` requires Rust local mini state at `:536`; the focused suite includes `testSiriDetailResolvesFromLocalMiniWhenHTTPUnavailableAndMarksContentGap`.
- Strict runtime grep artifact is honest for the plan grep. Remaining hits are generated Swift and tests only. No production runtime HTTP command/state/SSE hit appeared in the exact grep.

## Verdict

Approved for this Todo 10 client commit. No blockers found. Keep the server-side HTTP detail/read routes under the later final Todo 10/12 architecture audit if the intended final state is stricter than client-side demotion.

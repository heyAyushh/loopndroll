# Task 6 iOS Local-First Detail/Search/AppIntents

## Resolver order

- Siri/AppIntents entity, default/current, detail, and summary paths resolve through `LooperSiriSessionClient` using `LooperClientCoreSessionRuntime` first.
- The Siri local resolver now requires a non-empty Rust client-core state-mini cursor before accepting a cached snapshot, so an old HTTP snapshot cache is not treated as authoritative local state.
- `SessionDetail(summary:snapshot:)` marks mini-derived detail content as `localMiniOnly`; AppIntent summaries and context include that content state when full transcript content is unavailable.
- The visible iOS detail screen renders the same content gap from `SessionDetail.contentGapDescription` instead of implying that mini state is full transcript content.

## Stale HTTP/Spotlight guard

- The Siri/AppIntents path does not call HTTP snapshot/detail fallback when local mini state is unavailable; it throws the deterministic local-store unavailable path.
- A stale HTTP snapshot cannot replace newer local state in the changed path because the resolver only accepts cached summaries after `currentStateMiniSnapshot().latestSeq > 0`.
- Spotlight/search identifiers remain hints against the current local mini-backed session lists. Stale Spotlight IDs that are absent from local sessions do not create visible search rows.

## Focused commands

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter Siri --parallel | tee .omo/evidence/local-first-multinode-architecture-lock/task-6-siri.txt
```

Result: passed 5 Siri/AppIntents support tests.

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test-without-building -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,id=F12A26E9-9FBC-4EA1-B72B-8EF098D5B93C' '-only-testing:LooperCompanionTests/CompanionSessionMiniLocalFirstTests/testSiriDetailResolvesFromLocalMiniWhenHTTPUnavailableAndMarksContentGap()' -parallel-testing-enabled NO | tee .omo/evidence/local-first-multinode-architecture-lock/task-6-local-first-detail.txt
```

Result: passed the local mini + unavailable HTTP detail/AppIntents regression test.

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,id=F12A26E9-9FBC-4EA1-B72B-8EF098D5B93C' '-only-testing:LooperCompanionTests/SessionSummaryTimingTests/searchIgnoresStaleSpotlightIDsAbsentFromLocalMinis()' -parallel-testing-enabled NO | tee .omo/evidence/local-first-multinode-architecture-lock/task-6-search.txt
```

Result: passed the stale Spotlight/search regression test.

```bash
git diff --check
```

Result: passed with no whitespace errors.

## Worktree scope

- Pre-existing tracked Rust server changes were present before staging and were left untouched.
- This task's source edits are limited to iOS detail, search, AppIntents, and their focused tests.

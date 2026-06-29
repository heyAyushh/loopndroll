# Task 6 Code Review and Manual QA Matrix

Date: 2026-06-29

## Scope reviewed

- Reviewed Todo 6 commits: `172e2ac6` and `2fd6c39c`.
- Evidence directory: `.omo/evidence/local-first-multinode-architecture-lock/`.
- Product files reviewed from commit scope only: iOS AppIntents, iOS companion models/detail screen, and focused iOS tests.
- No product edits were made for this evidence repair.

## Architecture lock

| Check | Result | Evidence |
| --- | --- | --- |
| Local-first resolver order | Pass | `LooperSiriSessionClient.loadSnapshotLocalFirst()` requires an injected `CompanionSessionRuntime`, then requires `currentStateMiniSnapshot().latestSeq > 0`, then accepts `cachedSnapshot()`. If local mini state is unavailable, the path throws `localStoreUnavailable` instead of falling back to HTTP. |
| No Swift reducer truth | Pass | The Siri entity/default/current tests in `2fd6c39c` seed `CompanionSessionRuntime` with mini records and pending Siri commands; the Swift service snapshot is intentionally `unusedServiceSnapshot()`. Swift remains a caller/presenter for Rust client-core state, not a second source of truth. |
| Stale HTTP guard | Pass | `loadSessionDetail(for:)` returns local mini-backed detail or throws. `testSiriDetailResolvesFromLocalMiniWhenHTTPUnavailableAndMarksContentGap()` sets the service HTTP snapshot to fail and asserts `service.loadSnapshotCallCount == 0`. |
| Stale Spotlight guard | Pass | `searchIgnoresStaleSpotlightIDsAbsentFromLocalMinis()` proves Spotlight IDs absent from current local minis do not create visible running/stopped/archived rows. |
| Degraded content gap | Pass | `SessionDetail(summary:snapshot:)` marks mini-derived detail as `localMiniOnly`; Siri fallback/context strings include the content state; `SessionDetailScreen` renders `contentGapDescription`. |
| Generated/game files | Pass | `git show --name-only --format= 172e2ac6 2fd6c39c` lists only `.omo` evidence plus `ios/LooperCompanion/...` and `ios/LooperCompanionTests/...`; no generated, game, macOS, or Rust server files are in the reviewed Todo 6 commits. |
| Product diff scope | Pass | Before this evidence repair, `git diff --name-only HEAD` produced no output. During the repair, the tracked diff is intentionally limited to `task-6-git-check-rerun.txt`; the new `task-6-code-review.md` is evidence-only. No tracked product source files are dirty. |

## remove-ai-slops matrix

| Probe | Result | Notes |
| --- | --- | --- |
| Test asserts real behavior, not deletion | Pass | Siri default/current tests write through `setSiriDefaultSession` and `setSiriCurrentSession`, then resolve entities through the local mini runtime projection. |
| No tautological success | Pass | The empty service snapshot cannot satisfy the assertions; success depends on the Rust-backed local store. |
| No hidden fallback | Pass | Missing local runtime or empty local mini seq now errors instead of silently using stale cached HTTP truth. |
| No broad rewrite | Pass | The change is bounded to AppIntents/detail/search behavior and focused tests. |
| No new magic compatibility path | Pass | The nil-runtime test initializer was removed; tests must inject a real `CompanionSessionRuntime`. |

## programming matrix

| Area | Result | Notes |
| --- | --- | --- |
| Single responsibility | Pass | Resolver order, detail construction, degraded content status, and search filtering remain in their existing owners. |
| Meaningful names | Pass | New test/runtime helpers describe their role: `temporarySessionRuntime`, `seedMiniCache`, `unusedServiceSnapshot`, and `SiriRuntimeMiniRecord`. |
| DRY | Pass | The repair extracts shared mini-runtime seeding helpers instead of duplicating JSON store construction in each Siri test. |
| Encapsulation | Pass | Tests exercise through `CompanionSessionRuntime` and `LooperSiriSessionClient`; they do not reach into generated bindings or hand-edit generated stores. |
| Focused gates | Pass | Evidence uses Siri/AppIntents-focused SwiftPM and XCTest selectors plus whitespace checks, not unrelated broad suites. |

## Manual QA matrix

| Surface | Expected behavior | Evidence artifact | Result |
| --- | --- | --- | --- |
| Siri entity suggestions | Suggested entities come from Rust local mini projection order and exclude archived sessions. | `task-6-siri-rerun.txt`, `task-6-siri-default-current-rerun.txt` | Pass |
| Siri default/current | Default/current routing uses runtime pending commands and local mini projection, not an injected HTTP snapshot. | `task-6-siri-default-current-rerun.txt` | Pass |
| Detail from local mini | Detail opens from local mini state while HTTP detail is unavailable and marks full content as degraded. | `task-6-local-first-detail.txt`, `task-6-ios-local-first.md` | Pass |
| Search with stale Spotlight | Stale Spotlight IDs not present in local minis do not produce visible rows. | `task-6-search.txt`, `task-6-ios-local-first.md` | Pass |
| Whitespace gate | Current `HEAD` has no `git show --check` whitespace errors and the worktree diff has no whitespace errors. | `task-6-git-check-rerun.txt`, `git diff --check` rerun | Pass |
| Product scope | Todo 6 reviewed commits do not touch generated/game/macOS/Rust server files. | `git show --name-only --format= 172e2ac6 2fd6c39c` review output | Pass |

No manual device install or UI screenshot pass was run for this repair because the requested work is evidence-only and the Todo 6 code blockers were already closed by focused rerun artifacts.

## Focused commands

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter Siri --parallel
```

Recorded in `task-6-siri-rerun.txt`; passed 5 Siri/AppIntents support tests.

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test-without-building -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,id=F12A26E9-9FBC-4EA1-B72B-8EF098D5B93C' '-only-testing:LooperCompanionTests/SessionSummaryTimingTests/siriDefaultAndCurrentSessionsUseRustProjection()' -parallel-testing-enabled NO
```

Recorded in `task-6-siri-default-current-rerun.txt`; Swift Testing executed and passed the selected `Siri default and current sessions use Rust projection` test.

```bash
git show --check --format= HEAD
git diff --check
git diff --name-only HEAD
```

Rerun during this evidence repair. `git show --check --format= HEAD` and `git diff --check` exited 0. Product source diff remained empty; the only task-local changes from this repair are Todo 6 evidence artifacts.

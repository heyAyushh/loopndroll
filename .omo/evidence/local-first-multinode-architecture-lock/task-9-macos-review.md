# Task 9 macOS Code Review

Commit reviewed: `94c3b194d0589e12e864b0059e52445a8f1df241`

Verdict: `needs-fix`
codeQualityStatus: `BLOCK`
recommendation: `REQUEST_CHANGES`

## Skill Perspective Check

- `remove-ai-slops`: unavailable as a loadable skill in this session. Applied the prompt's documented overfit/slop criteria instead.
- `programming`: unavailable as a loadable skill in this session. Applied the prompt's documented programming-review criteria instead.
- Result: the diff violates both perspectives because the new cached-refresh test only proves the requested nil-preserve behavior with corrupt JSON while the production change conflates corrupt/unreadable local state with an authoritative empty local projection.

## CRITICAL

- None.

## HIGH

1. `macos/LooperMenuBar/Sources/LooperMenuBarCore/MenuRefreshCoordinator.swift:51` keeps the previous `sessionMiniSnapshot` whenever the local reread returns `nil`. `macos/LooperMenuBar/Sources/LooperMenuBarCore/MenuBarSessionMiniLocalStore.swift:176` also returns `nil` for a valid local state with zero sessions, not just read failures. Together, a normal cached refresh can keep stale menu rows after Rust client-core/local state authoritatively has no minis. This breaks the Task 9 requirement that SessionMini/local state is primary truth; the code needs to distinguish "local reread failed, preserve prior rows" from "local projection is empty, clear stale rows."

2. `.omo/evidence/local-first-multinode-architecture-lock/task-9-macos.md:11` lists focused commands and claims success, but the file contains no stdout/stderr artifact paths for those commands. Existing nearby evidence includes several failed `merge-9c32912f-macos-malformed-cache-*` runs and one passing local-first rerun, but they are not referenced by the task evidence. I reran the stated focused macOS filter successfully during review, so the source currently passes, but the submitted focused gate evidence is summary-only and not independently auditable.

## MEDIUM

- `macos/LooperMenuBar/Tests/LooperMenuBarCoreTests/MenuRefreshCoordinatorTests.swift:438` validates the nil-preserve path by corrupting the local file, but it does not cover the valid-empty projection case above. That leaves false confidence around the highest-risk branch introduced by this commit.

## LOW

- No deletion-only tests, generated-file edits, broad visual redesign, game-surface changes, or unrelated refactors found.

## Verification

- Inspected Todo 9 acceptance in `.omo/plans/local-first-multinode-architecture-lock.md`.
- Inspected changed files from `git show --stat --patch 94c3b194`.
- Inspected `.omo/evidence/local-first-multinode-architecture-lock/task-9-macos.md` and nearby task/merge evidence files.
- Ran `git diff --check -- macos/LooperMenuBar/Sources/LooperMenuBarCore/LooperDiagnosticsContent.swift macos/LooperMenuBar/Sources/LooperMenuBarCore/MenuRefreshCoordinator.swift macos/LooperMenuBar/Sources/LooperMenuBarCore/MobileRouteReadinessState.swift macos/LooperMenuBar/Tests/LooperMenuBarCoreTests/MenuRefreshCoordinatorTests.swift`: passed with no output.
- Ran `git diff --check`: passed with no output.
- Ran `swift test --package-path macos/LooperMenuBar --filter 'MenuRefreshCoordinator|MenuBarSessionMiniLocalFirst|LooperDiagnosticsContent'`: passed, 34 tests in 3 suites.

## Blockers

- Split local reread failure from valid empty SessionMini projection before preserving old rows.
- Replace or supplement `task-9-macos.md` with real focused gate output paths, or link the exact current passing artifacts.

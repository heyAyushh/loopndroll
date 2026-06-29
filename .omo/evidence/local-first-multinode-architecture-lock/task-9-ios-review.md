# Todo 9 iOS Code Review

Reviewed commit: `9c0b4fa3adba7d2d80a331365683941e452a128c`
Verdict: `needs-fix`
codeQualityStatus: `BLOCK`
recommendation: `REQUEST_CHANGES`

## Scope Reviewed

- Plan acceptance: `.omo/plans/local-first-multinode-architecture-lock.md` Todo 9.
- Source diff:
  - `ios/LooperCompanion/App/CompanionAppModel.swift`
  - `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/CompanionSurfaceFilteringTests.swift`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-9-ios.md`
- Focus: assistant surface switching must be local-only and instant; no command enqueue, stream restart, HTTP state/snapshot call, or mini collapse on surface switch.

## Skill-Perspective Check

- `remove-ai-slops`: unavailable as a loadable skill. I searched local skill roots for `remove-ai-slops/SKILL.md` and found no file, so I applied the review prompt criteria directly.
- `programming`: unavailable as a loadable skill. I searched local skill roots for `programming/SKILL.md` and found no file, so I applied the review prompt criteria directly.
- Result: the production diff does not add unnecessary parsing, extraction, or abstraction. The new focused test does violate the review perspective because it does not exercise the changed app-model path and gives weak confidence for the claimed Todo 9 behavior.

## Evidence Checked

- Commit diff: `git diff --no-renames 9c0b4fa3^ 9c0b4fa3`.
- Whitespace: `git diff --check 9c0b4fa3^ 9c0b4fa3` passed.
- Focused test rerun:
  `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter rapidSurfaceSwitchesKeepLocalMiniSourceStable`
  passed 1 test in 1 suite.
- Web evidence rule satisfied with Swift.org package-manager documentation for `swift test` package execution and `--package-path`.

## CRITICAL

None.

## HIGH

1. Misleading evidence artifact references the wrong commit.
   `.omo/evidence/local-first-multinode-architecture-lock/task-9-ios.md:3` says the evidence is for `40f0b671fbcc3dab33612324e9d037bde5453598`, but the reviewed commit is `9c0b4fa3adba7d2d80a331365683941e452a128c`. The old hash exists locally as a commit object, but the artifact is committed inside `9c0b4fa3`; this makes the evidence ambiguous for the reviewed commit.

2. The new focused test does not prove the changed behavior or the Todo 9 no-hot-path requirements.
   `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/CompanionSurfaceFilteringTests.swift:210` adds `rapidSurfaceSwitchesKeepLocalMiniSourceStable`, but the test only filters a local `[LocalMini]` array through `CompanionSurfaceFiltering.matches` at lines 231-240. It never instantiates `CompanionAppModel`, never calls `selectAssistantSurface`, and cannot observe pending commands, stream start/stop, `loadSnapshot`, or mini collapse. It is redundant with the existing surface-filtering tests above it and is not adequate acceptance evidence for Todo 9.

## MEDIUM

1. Todo 9 real-surface acceptance evidence is missing from the artifact.
   `.omo/evidence/local-first-multinode-architecture-lock/task-9-ios.md:22` explicitly defers ETTrace and real-surface 100-switch profiler proof to Todo 11, while Todo 9 acceptance requires 100 rapid assistant tab switches with zero stream restarts, zero HTTP command/state calls, no list collapse, and no stale label lies. This may be acceptable only if Todo 9 is deliberately narrowed, but as written it is not confirmed.

## LOW

1. No source blocker found in the one-line production change.
   `ios/LooperCompanion/App/CompanionAppModel.swift:246` now routes the property setter through `selectAssistantSurface`. The target helper at `ios/LooperCompanion/App/CompanionAppModel.swift:1594` calls local snapshot projection only and records `assistant-surface:selected-local`; I found no command enqueue, sync restart, or HTTP snapshot call on that path.

## Blockers

- Correct `.omo/evidence/local-first-multinode-architecture-lock/task-9-ios.md:3` so the evidence identifies the reviewed commit or clearly explains the relationship to `40f0b671`.
- Replace or supplement the new Core-only array-filter test with evidence that exercises the changed app-model selection path and proves no `setAssistantSurface` pending command, no stream restart, no `loadSnapshot`/HTTP call, and no local mini collapse across rapid surface switches.

## Final Recommendation

Request changes. The production source change appears directionally correct and local-only, but the committed evidence is misleading and the new focused test is not relevant enough to confirm Todo 9 acceptance.

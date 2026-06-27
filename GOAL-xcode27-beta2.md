# Looper Xcode 27 Beta 2 Upgrade Goal

## Outcome

Ship a release-note-backed Looper upgrade pass for Xcode 27 beta 2 that adds and verifies multiple real beta 2 features or fixes across the native Apple surfaces Looper actually uses.

## Source Evidence

- Apple release notes: https://developer.apple.com/documentation/xcode-release-notes/xcode-27-release-notes
- Local evidence copies:
  - `.build/xcode27-beta2-goal/xcode-27-release-notes.json`
  - `.build/xcode27-beta2-goal/xcode-27-release-notes.md`
- Current selected toolchain:
  - `xcode-select -p` => `/Applications/Xcode-beta.app/Contents/Developer`
  - `xcodebuild -version` => `Xcode 27.0`, build `27A5194q`
  - installed runtime includes `iOS 27.0 (24A5370g)`

## Relevant Beta 2 Areas

- App Intents: resolved Siri AppShortcut phrase behavior for App enum values.
- Core AI: resolved CoreAI model execution under Metal API Validation and model parameter display issues.
- Debugging: LLDB now ships `lldb-mcp`.
- Device Hub: new network pairing for iOS 27/iPadOS 27/watchOS 27 and new simulated pointer gesture behavior.
- `devicectl`: `--json-output` can write JSON to stdout.
- Simulator: fixed log rotation, but simulator deletion and Accessibility Inspector still have known issues.
- Testing: Swift Testing and XCTest cross-framework issue surfacing fixes.
- Previews: iOS preview/runtime and main-actor preview fixes.
- Coding Intelligence: Preview Snapshot MCP now returns simulator platform/device/OS, and beta deep-link handling was fixed.

## Baseline

- Siri/App Intents simulator proof is now complete for the previous goal.
- Current worktree has existing iOS/App Intents/project diffs; do not revert unrelated changes.
- Xcode 27 beta 2 app bundle `27A5209h` is not installed locally because Apple Developer authentication was required for the download. The current beta Xcode still reports `27A5194q`.
- The available iOS 27 simulator runtime is `24A5370g`, which is sufficient for simulator proof unless a chosen feature specifically requires a newer beta 2 Xcode app.

## Required Scope

Implement at least six meaningful Looper changes backed by the beta 2 notes. The set must include:

1. One Siri/App Intents robustness improvement or verifier.
2. One `devicectl` or Device Hub proof/script improvement.
3. One simulator reliability or log hygiene improvement.
4. One Swift Testing, XCTest, or CI gate improvement.
5. One preview, debug, or local developer workflow improvement.
6. One Core AI/FoundationModels/Spotlight/App Entity compatibility improvement, or a documented no-op guard if the API is unavailable.

Each change must be code, script, test, or repo documentation that changes Looper behavior, proof quality, or developer workflow. Do not pad the count with cosmetic edits.

## Non-Goals

- Do not install a new Xcode app without explicit Apple ID/2FA participation from the user.
- Do not publish, release, push, or install to a physical device unless separately requested.
- Do not modify `.git`, `.env`, credentials, or user config files.
- Do not add hosted/cloud behavior; keep Looper local-first.

## Primary Verifier

Run a final beta 2 proof pack that demonstrates the selected changes with local commands and artifacts. The proof pack must include:

```sh
PATH="/opt/homebrew/bin:$PATH" DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer bash scripts/check-ios.sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'generic/platform=iOS' build
git diff --check
```

Add focused tests or proof commands for any changed macOS/Rust/script surface.

## Iteration Loop

1. Read the Apple beta 2 notes and map only relevant items to Looper surfaces.
2. Pick the highest-impact small batch of changes with independent verifiers.
3. Change one coherent slice at a time.
4. Run the narrow verifier for that slice.
5. Record evidence paths and failures.
6. Repeat until the required scope and primary verifier pass.

## Anti-Cheating Rules

- Do not count a release-note item unless a Looper artifact changes or a verifier proves the current behavior.
- Do not weaken App Intents metadata checks, simulator proof, or existing tests to get green.
- Do not replace end-to-end proof with build-only proof for Siri, simulator, or device-facing behavior.
- Do not rely on stale local skills when Apple beta 2 notes or installed Xcode behavior contradict them.

## Approval Gates

- Ask before deleting runtimes, simulators, DerivedData outside this repo, device data, or user configuration.
- Ask before replacing Xcode, changing signing identities, installing to physical devices, publishing, pushing, or opening external authenticated flows.

## Blocker Standard

Only mark blocked after the same external Apple/Xcode/runtime/auth blocker recurs for three goal turns and no safe repo, script, simulator, or documentation work remains.

## Completion Proof

Before marking complete, report:

- The six or more beta 2-backed changes.
- Exact files changed.
- Exact commands run and pass/fail status.
- Toolchain and simulator runtime used.
- Evidence artifacts under `.build/`.
- Any release-note items intentionally left out and why.

# Looper Exhaustive UI Control QA Goal

## Outcome

Prove that every user-visible iOS companion control and every user-visible macOS Looper menu/diagnostics control either works end to end or is explicitly documented as unavailable with a product-owned disabled/error state.

## Baseline

- Previous ACP/mobile goal proved core routes, simulator build/install/launch, Swift model tests, macOS diagnostics content, and server behavior.
- It did not prove a tap-through matrix for every visible iOS button, tab, picker, search scope, sheet, toolbar item, text input, destructive confirmation, game/debug toggle, or macOS menu/diagnostics action.
- iOS proof must use the built-in Xcode simulator. Physical phone install is out of scope unless the user explicitly reopens it.

## Required Coverage

- Inventory all visible iOS controls under `ios/LooperCompanion/UI/**` and all visible macOS menu/diagnostics controls under `macos/LooperMenuBar/Sources/**`.
- Add accessibility identifiers where automation cannot reliably find a control by stable label.
- Add XCTest/XCUITest or product-level UI-state tests that interact with:
  - root tabs and onboarding gate
  - sessions list, assistant surface picker, refresh/search/device hub affordances
  - session detail prompt/send/delete/cancel or disabled states
  - settings save, route preference, quick action, Face ID/local-network/scanner/game toggles
  - search scopes, recent/suggested result navigation, settings destinations
  - macOS menu actions and diagnostics window controls/content
- Controls that depend on unavailable hardware, biometric prompts, camera permission, or external apps must still have deterministic disabled/error/fallback proof.

## Verifiers

Primary verifier:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=27.0' test
```

Supporting verifiers:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
swift test --package-path macos/LooperMenuBar
cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane acp_hosts
cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check
git diff --check
```

Runtime/manual verifier:

```text
Install and launch LooperCompanion on the built-in Xcode simulator, capture at least one screenshot or accessibility tree for each major UI surface, and record the control inventory matrix with pass/fail evidence.
```

## Anti-Cheating Rules

- Do not count a control as covered merely because the app launches.
- Do not weaken labels, hide controls, skip tests, or replace real behavior with mock-only success unless the user explicitly accepts a non-runtime control.
- Do not mark destructive controls as working without proving the confirmation/cancel path and the safe side effect.
- Do not require a physical iPhone for this acceptance pass.
- Do not mutate `.git`, `.env`, credential files, or files outside the project root.

## Blocker Standard

Only mark blocked after the same external simulator/Xcode tooling blocker recurs for three consecutive goal turns and no safe repo-side control test or fallback proof remains.

## Completion Proof

Completion requires:

- A checked-in control inventory artifact mapping every visible control to an automated or manual proof.
- Passing iOS simulator UI tests for the full control matrix.
- Passing relevant Swift/Rust regression checks.
- Screenshots or accessibility snapshots for the major iOS/macOS UI surfaces.
- A final report listing any controls intentionally classified as unavailable, with product-owned disabled/error behavior.

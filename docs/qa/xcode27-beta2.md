# Xcode 27 Beta 2 Proof Notes

## Source

- Apple release notes: https://developer.apple.com/documentation/xcode-release-notes/xcode-27-release-notes
- Script-fetched DocC JSON: `.build/xcode27-beta2-proof/xcode-27-release-notes.json`
- Script-fetched Markdown: `.build/xcode27-beta2-proof/xcode-27-release-notes.md`

## Local Toolchain State

- Selected developer directory: `/Applications/Xcode-beta.app/Contents/Developer`
- Current local Xcode build observed by the proof script: `Xcode 27.0`, build `27A5194q`
- Available simulator runtime observed by the proof script: `iOS 27.0`
- `lldb-mcp` is treated as a required beta 2 app-bundle feature only when `scripts/prove-xcode27-beta2.sh --require-lldb-mcp` is used. The default proof records a warning when the selected Xcode app does not include it.

## Beta 2 Mapping

| Release-note area | Looper proof or guard |
| --- | --- |
| App Intents Siri AppShortcut phrase fix | `scripts/check-ios.sh` rejects stale Siri metadata, private helper enum metadata, and enum-backed shortcut phrases until runtime coverage is added. |
| `devicectl --json-output -` stdout support | `scripts/prove-xcode27-beta2.sh` and `scripts/prove-ios-siri-runtime.sh` parse stdout JSON from `devicectl`. |
| Simulator reliability and log hygiene | `scripts/prove-ios-siri-runtime.sh --simulator-only` writes bounded Siri surface artifacts under `.build/siri-runtime/surface/` instead of relying on external simulator log locations. |
| Swift Testing improvements | `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/LooperCurrentSessionResolutionTests.swift` includes parameterized current-session resolver coverage. |
| Debug workflow | `scripts/prove-xcode27-beta2.sh` verifies or warns on `lldb-mcp` so the local app bundle mismatch is visible. |
| Core AI and FoundationModels | `scripts/prove-xcode27-beta2.sh` archives the guarded Siri AI capability report and records whether FoundationModels and SpotlightSearchTool are available. |

## Verifier

```sh
PATH="/opt/homebrew/bin:$PATH" DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer bash scripts/prove-xcode27-beta2.sh
PATH="/opt/homebrew/bin:$PATH" DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer bash scripts/check-ios.sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'generic/platform=iOS' build
git diff --check
```

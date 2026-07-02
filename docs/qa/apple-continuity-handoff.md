# Apple Continuity Handoff QA

Date: 2026-07-02
Branch: `worktree-phase0-orchestration`

## Scope

This note covers Looper's native Apple Handoff path between the iPhone companion
and the installed macOS menu bar app.

The verified direction in this pass was iPhone to Mac:

1. Open a Looper session on iPhone.
2. Click the Looper Handoff badge in the macOS Dock.
3. macOS opens the matching local session target.

## Implementation Contract

- iOS publishes `dev.looper.app.continue-session` with:
  - `sessionID`
  - `assistantSurface`
  - `handoffWebpageURL`
  - `targetContentIdentifier = looper.session.<sessionID>`
- iOS marks the Handoff payload keys as required.
- macOS accepts:
  - native `dev.looper.app.continue-session` activities
  - `NSUserActivityTypeBrowsingWeb` fallback activities whose URL matches
    `/handoff/sessions/<sessionID>`
- macOS restores the Handoff-compatible installed bundle identity:
  - `CFBundleIdentifier = dev.looper.app.ios`
  - `com.apple.application-identifier = Z5454ZPPUX.dev.looper.app.ios`
- macOS only opens `codex://threads/<sessionID>` for Codex-compatible surfaces.
  Non-Codex assistant surfaces fall back to transcript/project URLs instead of
  fabricating a Codex deep link.

## Verification

Commands run:

```bash
swift test --package-path macos/LooperMenuBar
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild \
  -project ios/LooperCompanion.xcodeproj \
  -scheme LooperCompanion \
  -configuration Debug \
  -destination 'id=D7749DEB-F9A8-5FD6-B33B-BF715B8B2F7C' \
  -derivedDataPath /tmp/looper-ios-continuity-derived \
  build
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcrun devicectl device install app \
  --device D7749DEB-F9A8-5FD6-B33B-BF715B8B2F7C \
  /tmp/looper-ios-continuity-derived/Build/Products/Debug-iphoneos/Looper.app
bash scripts/build-macos-menu-bar-package.sh --install
```

Installed macOS registration checked:

```text
path: /Applications/looper.app
identifier: dev.looper.app.ios
teamID: Z5454ZPPUX
activityTypes: Z5454ZPPUX:dev.looper.app.continue-session, NSUserActivityTypeBrowsingWeb
```

Physical iPhone was launched to:

```text
looper://session/019f2330-e00b-7d52-aa18-e859951bba6a?baseURL=http%3A%2F%2F192.168.1.10%3A8765
```

Phone screenshot artifact:

```text
build/continuity-proof/iphone-session-relaunched.png
```

Mac Dock screenshot artifact:

```text
build/continuity-proof/macos-dock-after-phone-relaunch.png
```

Receiver proof from `log stream`:

```text
handoff continuation received session=019f2330-e00b-7d52-aa18-e859951bba6a assistantSurface=codex targetContentIdentifier=looper.session.019f2330-e00b-7d52-aa18-e859951bba6a webpageURL=http://100.119.200.69:8765/handoff/sessions/019f2330-e00b-7d52-aa18-e859951bba6a
handoff continuation open session=019f2330-e00b-7d52-aa18-e859951bba6a codexTarget=codex://threads/019f2330-e00b-7d52-aa18-e859951bba6a fallbackTarget=file:///Users/ay/Documents/looper/ opened=true
```

User-visible confirmation:

```text
its working for codex
```

## Residual Risk

Codex reverse Handoff is proved. Non-Codex reverse Handoff target selection is
unit-tested for avoiding fake Codex URLs, but it still needs the same installed
Dock-click QA on a live non-Codex session before calling that path fully proved.

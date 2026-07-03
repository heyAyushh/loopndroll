# Apple Continuity Handoff QA

Date: 2026-07-03
Branch: `worktree-phase0-orchestration`
Latest proof commit: `7bedc648a fix(handoff): preserve Continuity session URLs`

## Scope

This note covers Looper's native Apple Handoff path between the iPhone companion
and the installed macOS menu bar app.

Two directions are covered:

- iPhone to Mac: open a Looper session on iPhone, then click the Looper Handoff
  badge in the macOS Dock. macOS opens the matching local session target.
- Mac to iPhone: press the macOS Handoff hotkey while a Looper session is
  current. iOS should show the Looper Continuity affordance at the bottom of the
  app switcher; tapping it opens the matching session on iPhone.

## Implementation Contract

- iOS publishes `dev.looper.app.continue-session` with:
  - `sessionID`
  - `assistantSurface`
  - `handoffWebpageURL`
  - `targetContentIdentifier = looper.session.<sessionID>`
- iOS marks the Handoff payload keys as required.
- iOS declares and receives both:
  - `dev.looper.app.continue-session`
  - `NSUserActivityTypeBrowsingWeb`
- macOS accepts:
  - native `dev.looper.app.continue-session` activities
  - `NSUserActivityTypeBrowsingWeb` fallback activities whose URL matches
    `/handoff/sessions/<sessionID>`
- macOS publishes the current session activity with:
  - `targetContentIdentifier = looper.session.<sessionID>`
  - `webpageURL = <reachable-base-url>/handoff/sessions/<sessionID>`
  - matching `handoffWebpageURL` in `userInfo`
- macOS must preserve the last known-good authenticated Handoff base URL between
  HTTP health refreshes. SessionMini updates must not overwrite a valid current
  activity with `webpageURL=none` while the server route is still usable.
- Command-L performs a forced menu refresh, builds a fresh session descriptor,
  and publishes that descriptor as the focus-assisted current activity in one
  step. It must not first publish a stale/generic activity and then separately
  request focus assist.
- macOS restores the Handoff-compatible installed bundle identity:
  - `CFBundleIdentifier = dev.looper.app.ios`
  - `com.apple.application-identifier = Z5454ZPPUX.dev.looper.app.ios`
- macOS only opens `codex://threads/<sessionID>` for Codex-compatible surfaces.
  Non-Codex assistant surfaces fall back to transcript/project URLs instead of
  fabricating a Codex deep link.

## Verification: iPhone to Mac

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

## Verification: Mac to iPhone

Final verified commit:

```text
7bedc648a fix(handoff): preserve Continuity session URLs
```

Commands run:

```bash
swift test --package-path macos/LooperMenuBar
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild \
  -project ios/LooperCompanion.xcodeproj \
  -scheme LooperCompanion \
  -configuration Debug \
  -sdk iphoneos \
  -destination 'id=D7749DEB-F9A8-5FD6-B33B-BF715B8B2F7C' \
  -derivedDataPath build/continuity-proof/ios-physical-derived \
  build
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcrun devicectl device install app \
  --device D7749DEB-F9A8-5FD6-B33B-BF715B8B2F7C \
  build/continuity-proof/ios-physical-derived/Build/Products/Debug-iphoneos/Looper.app
bash scripts/build-macos-menu-bar-package.sh --install
```

Installed app identity checked:

```text
iPhone bundle: dev.looper.app.ios 1.2.0 (2)
macOS path: /Applications/looper.app
macOS bundle identifier: dev.looper.app.ios
macOS team identifier: Z5454ZPPUX
NSUserActivityTypes:
  - dev.looper.app.continue-session
  - NSUserActivityTypeBrowsingWeb
```

Route health checked:

```text
ok=true
requiresAuthentication=true
baseURLs=http://192.168.1.28:8765,http://100.119.200.69:8765,http://127.0.0.1:8765
tailscale=http://100.119.200.69:8765 running=true
```

Direct iPhone receiver proof:

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcrun devicectl device process openURL \
  --device D7749DEB-F9A8-5FD6-B33B-BF715B8B2F7C \
  --activate 'looper://session/019f2330-e00b-7d52-aa18-e859951bba6a?baseURL=http%3A%2F%2F100.119.200.69%3A8765'
```

Observed device result:

```text
Opened URL looper://session/019f2330-e00b-7d52-aa18-e859951bba6a?baseURL=http%3A%2F%2F100.119.200.69%3A8765 on device.
```

Screenshot artifact:

```text
build/continuity-proof/iphone-after-direct-openurl-2.png
```

The screenshot showed the physical iPhone on the `Add goal icon` session:

```text
session=019f2330-e00b-7d52-aa18-e859951bba6a
title=Add goal icon
assistant=Codex
project=/Users/ay/Documents/looper
status=Active
```

Final Handoff sender proof from `log stream`:

```text
2026-07-03 13:12:19 useractivityd doMarkUserActivityAsDirty ... webpageURL=private
2026-07-03 13:12:49 useractivityd doMarkUserActivityAsDirty ... webpageURL=private
```

Earlier failing evidence showed the bug:

```text
handoff activity published ... session=019f2330-e00b-7d52-aa18-e859951bba6a ... webpageURL=http://100.119.200.69:8765/handoff/sessions/019f2330-e00b-7d52-aa18-e859951bba6a
handoff activity published ... session=019f2330-e00b-7d52-aa18-e859951bba6a ... webpageURL=none
```

The final proof stayed on `webpageURL=private` past the old 30 second health
freshness boundary, so SessionMini refreshes no longer erase the URL payload.

User-visible confirmation after install:

```text
worked
```

## Debugging Notes

- A Looper publish log with `webpageURL=none` after a valid session publish is a
  regression for Mac to iPhone Continuity.
- `useractivityd` redacts actual URLs as `webpageURL=private`; that is expected.
  The failure signal is `webpageURL=-`.
- The macOS app can show `supported=false` while still publishing the URL-backed
  Handoff activity. `supported=false` means ambient native readiness is not
  proved by a live Session route; Command-L and fresh HTTP health may still
  supply the manual Handoff URL.
- `devicectl openURL` proves the iOS receiver and session router, but it does
  not programmatically open the app switcher. The bottom Continuity affordance
  remains a human visual check.

## Residual Risk

Codex reverse Handoff is proved. Non-Codex reverse Handoff target selection is
unit-tested for avoiding fake Codex URLs, but it still needs the same installed
Dock-click QA on a live non-Codex session before calling that path fully proved.

Mac to iPhone was proved for Codex on a paired physical iPhone. Repeat the same
installed-device QA on a live non-Codex session before treating every assistant
surface as fully proved.

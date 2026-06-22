# Looper UI Control Matrix

Date: 2026-06-22
Primary simulator: iPhone 17 Pro, iOS 27.0

This inventory maps the user-visible Looper iOS companion and macOS menu bar
controls to the strongest proof available in this goal pass. Simulator-only
hardware gaps are listed explicitly with product-owned fallback behavior.

## Evidence Index

- iOS UI tap-through, first six non-pinball flows:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-ios-xcodebuild-full-scheme-final-12.txt`
- iOS Settings tap-through:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-ios-targeted-settings-final-2.txt`
- Xcode beta note:
  `final-12` was interrupted after the first six UI tests passed because Xcode
  hung launching the final Settings test; `targeted-settings-final-2` records
  the Settings pass before Xcode's post-test `simctl diagnose` finalizer was
  interrupted. The product test bodies passed.
- iOS companion model/core tests:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-ios-companion-core-swift-test-final.txt`
- Swift realtime tests:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-swift-looper-realtime-test-final.txt`
- macOS menu bar product-level tests:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-macos-menubar-swift-test-final.txt`
- Rust ACP host regression:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-rust-acp-hosts-test-final.txt`
- Rust formatting:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-rust-cargo-fmt-check-final.txt`
- Diff whitespace:
  `.omo/ulw-loop/019edcdb-db12-7390-8c69-895bc9800406/evidence/ui-control-git-diff-check-final-3.txt`

## iOS Companion

| Surface | Controls | Proof |
| --- | --- | --- |
| Onboarding | `Enter device code`, keyboard `Done`, `Login with Device Code`, `Scan Mac Orb`, scanner close, `Enable Local Network`, `Enable Notifications`, `Start Using looper` | `testOnboardingControlsReachMainApp`; attachments `ios-onboarding.*`, `ios-sessions-after-onboarding.*` |
| Scanner fallback | Close, controls menu, `Upload Image`, `Paste orb_id directly`, field keyboard `Done`, `Connect with Orb`, `Resume Live Scan`, `Reset` | `testOrbScannerLaunchControls`; attachment `ios-scanner-fallback.*`; unavailable live camera is rendered as `Camera Unavailable` |
| Sessions root | Sessions tab, Settings tab, Search button, device hub button, session row navigation, route/status badges | `testSessionsDeviceHubAndSessionDetailControls`, `testSettingsControls`, `testSearchControlsAndDestinations`; attachment `ios-sessions-list.*` |
| Assistant surface picker | Codex, Claude Code, Zed, Devin, Grok Build surface chips and per-surface filtering | `CompanionSurfaceFilteringTests`, `LooperCurrentSessionResolutionTests`, iOS full scheme root screenshot/accessibility attachments |
| Device hub sheet | `Done`, `Scan Mac Orb`, notification status, route details | `testSessionsDeviceHubAndSessionDetailControls`; attachment `ios-device-hub.*` |
| Session detail | `Use with Siri`, prompt editor, keyboard `Done`, `Send Prompt`, `Archive Session`, `Unarchive Session`, `Delete Session`, destructive alert `Cancel` | `testSessionsDeviceHubAndSessionDetailControls`; attachment `ios-session-detail.*` |
| Session modes | `Await Reply`, `Completion Checks`, `Max Turns 1`, `Max Turns 2`, `Max Turns 3`, `Use Global Default`, `Infinite` | `testSessionModeControls` taps every mode |
| Settings route/auth | Route picker `Remote`, `Tailscale`, `LAN`; connection code field; `Save`; local network request; Face ID toggle, `Lock Now`, Face ID error/status | `testSettingsControls`; `CompanionConnectionRoutePresentationTests`; attachment `ios-settings.*` |
| Settings quick actions | `Open Session`, `Continue`, `Reply`, `Archive`, `Mute Session` toggles | `testSettingsControls` toggles each action |
| Settings appearance | `Dark`, `Light`, `System` | `testSettingsControls`; attachment `ios-settings.*` |
| Settings routes/checks | `Notification Routes`, `Completion Checks` destination links | `testSettingsControls` opens both destinations and returns to Settings; model data is covered by companion core tests |
| Search | Search tab/button, search field, keyboard `Done`, scopes `All`, `Sessions`, `Actions`, `Settings`, `Device`, recent query chip, session result navigation, settings/action destination rows | `testSearchControlsAndDestinations`; attachment `ios-search.*`; `LooperSiriEntitySupportTests` covers search matching fields |
| App locked screen | `Unlock with Face ID`, `Turn Off Face ID Unlock` | Hardware-dependent state. Simulator proof is the Face ID settings fallback/status path in `testSettingsControls`; locked-screen buttons remain product-owned recovery controls. |

## macOS Menu Bar

| Surface | Controls | Proof |
| --- | --- | --- |
| Main status menu | Status rows, active/ready counts, coverage details, thread sections, assistant/client rows | `LooperMenuContentTests`, `LooperLifecycleCoordinatorTests`, `swift test --package-path macos/LooperMenuBar` |
| Menu actions | `Refresh`, `Diagnostics`, `Stop Server`, `Quit` | `MenuRefreshCoordinatorTests` for refresh reuse/force/error paths; `LooperLifecycleCoordinatorTests` for launch, terminate, detached quit, manual stop, lifecycle endpoints |
| ACP hosts | `Assistant Hosts (ACP)`, `Configured ACP Targets`, agent details, Codex/Claude/Zed/Devin/Grok rows | `LooperMenuContentTests` ACP rows; `LooperLifecycleCoordinatorTests` ACP host decoding; `cargo test ... acp_hosts` |
| Mobile route menu | `Remote first`, `Tailscale first`, `LAN first`, mobile health route labels | `LooperContinuationActivityTests`, `LooperDiagnosticsContentTests`, `swift test --package-path macos/LooperMenuBar` |
| Handoff controls | Focus assist options, hotkey options, hold duration options | `LooperContinuationActivityTests`; route/handoff URL tests in same package |
| Diagnostics window/content | Diagnostics launch argument, mobile health, ACP targets, desktop connections, routes, unavailable/error report rows | `LooperDiagnosticsContentTests`, `MenuRefreshCoordinatorTests` |

## Simulator Fallbacks

- Physical iPhone install was intentionally out of scope for this goal.
- Camera scanning cannot complete in the simulator; the app shows `Camera Unavailable`
  and exposes upload/direct-code recovery controls.
- Notification and local-network prompts are accepted when the simulator presents
  them; otherwise the UI remains deterministic and the test proceeds through the
  product controls.
- Face ID cannot be treated as physical biometric proof in the simulator; settings
  expose a deterministic Face ID status/error path and locked-screen recovery
  buttons remain covered as hardware-dependent controls.
- Pinball testing is intentionally excluded from this matrix per the current
  user instruction.

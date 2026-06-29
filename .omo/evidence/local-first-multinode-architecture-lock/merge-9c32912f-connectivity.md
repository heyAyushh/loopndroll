# merge-9c32912f connectivity

Date: 2026-06-29

## Commits

- Source commit: `9c32912ffb293aa7a7838a63d1172d8d5776afcd`
- Resulting commit: this integration commit
- Integration method: `git cherry-pick -n 9c32912f`
- Conflicts: none

## Integration summary

- The staged route/connectivity patch has stable patch-id `83b8e09e3435cb13686c7b3e3d173d7a843966b7`, matching source commit `9c32912f`.
- Server mobile route advertisement now keeps LAN, Tailscale/MagicDNS, and loopback fallback while filtering explicit public remote URLs from HTTP and gRPC mobile route lists.
- iOS route selection/presentation no longer exposes Remote as a first-class selectable or displayable route. Legacy stored `remote` still decodes and falls back to Tailscale ordering.
- macOS route ordering treats stale Remote preference as Tailscale ordering instead of prioritizing a public remote URL.
- The macOS `MenuBarSessionMiniLocalFirstTests` expectation was updated for current local-first runtime behavior: a valid cached mini row is preserved as a local fallback even when the stored row id and payload id disagree. Offline outbox assertions are unchanged.

## Tailscale LocalAPI boundary

- Looper's prior project decision says not to embed `tailscale-rs` on the critical path; prefer the official Tailscale client or an external boundary.
- Tailscale's CLI docs describe the installed client/CLI as the supported device management and troubleshooting surface: https://tailscale.com/kb/1080/cli
- Tailscale's `tailscaled` docs describe the privileged daemon as the component that handles network work on devices: https://tailscale.com/kb/1278/tailscaled
- The official Go docs expose `tailscale.com/client/local` as a LocalAPI client package: https://pkg.go.dev/tailscale.com/client/local
- Looper should consume LAN/Tailscale routes discovered through that installed-client/daemon boundary instead of embedding a third-party Tailscale implementation in the Rust control plane.

## Changed files

- `.omo/evidence/local-first-multinode-architecture-lock/merge-9c32912f-connectivity.md`
- `crates/agent-control-plane/src/mobile/network.rs`
- `ios/LooperCompanion/App/CompanionAppViewState.swift`
- `ios/LooperCompanion/App/CompanionConfiguration.swift`
- `ios/LooperCompanion/UI/Settings/SettingsScreen.swift`
- `ios/LooperCompanionCore/Sources/LooperCompanionCore/CompanionBaseURLFiltering.swift`
- `ios/LooperCompanionCore/Sources/LooperCompanionCore/CompanionBaseURLRouting.swift`
- `ios/LooperCompanionCore/Sources/LooperCompanionCore/CompanionConnectionRoutePresentation.swift`
- `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/CompanionBaseURLFilteringTests.swift`
- `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/CompanionBaseURLSelectionTests.swift`
- `ios/LooperCompanionCore/Tests/LooperCompanionCoreTests/CompanionConnectionRoutePresentationTests.swift`
- `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`
- `macos/LooperMenuBar/Sources/LooperMenuBarCore/ControlPlaneClient.swift`
- `macos/LooperMenuBar/Tests/LooperMenuBarCoreTests/LooperContinuationActivityTests.swift`
- `macos/LooperMenuBar/Tests/LooperMenuBarCoreTests/MenuBarSessionMiniLocalFirstTests.swift`

## Focused verification

- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml mobile::network -- --nocapture`
  - Passed: 15 tests, 214 filtered.
- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter CompanionBaseURL --parallel`
  - Passed: 22 tests in 2 suites.
- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter CompanionConnectionRoutePresentation --parallel`
  - Passed: 11 tests in 1 suite.
- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path macos/LooperMenuBar --filter 'MobileRoute|LooperContinuationActivity|MenuBarSessionMiniLocalFirst'`
  - Passed: 38 tests in 2 suites.
- `cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check`
  - Passed with no output.
- `git diff --check`
  - Passed with no output.

## Intentionally out of scope

- No installs, simulator proof, physical-device proof, broad Swift suites, or broad Rust suites.
- No APNs/push behavior changes.
- No new architecture slice and no cloud/public remote route as a first-class selectable realtime route.

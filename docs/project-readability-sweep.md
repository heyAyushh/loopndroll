# Looper Whole-Project Readability Sweep

This note captures the cross-surface module boundaries tightened during the
Rust, macOS, and iPhone readability pass.

## Rust Control Plane

- `crates/agent-control-plane/src/http/mod.rs` composes route groups; route
  families live in `http/events.rs`, `http/handoff.rs`,
  `http/mobile_state.rs`, and `http/session_actions.rs`.
- `crates/agent-control-plane/src/acp_client_host.rs` is the public facade for
  ACP host summaries. Provider-specific mapping lives in
  `acp_client_host/devin.rs` and `acp_client_host/zed.rs`.
- `mobile_auth.rs` keeps the connection-code response values in local typed
  variables instead of reading them back from transient JSON.

## macOS Menu Bar

- `TailscaleNetworkPattern` owns shared Tailscale host and text detection for
  menu subtitles and control-plane URL classification.
- `DesktopThreadDisplayText` owns shared desktop-thread title and project-name
  fallback rules for menu rows and Handoff continuation descriptors.
- Larger AppKit splits remain intentionally separate: `ControlPlaneClient.swift`,
  `main.swift`, and `LooperContinuationActivityPublisher.swift` still need
  type-level extraction, but those moves touch broader runtime behavior.

## iPhone Companion Core

- `CompanionBaseURLFacts` owns normalized companion URL scheme, host, and port
  facts used by routing and selection.
- `CompanionBaseURLIdentity` owns normalized URL identity and deduplication, so
  route selection and physical-device filtering cannot drift.
- `LooperSiriSessionSupport.swift` keeps the beta `SpotlightSearchTool` path
  behind both module availability and a non-x86 simulator architecture guard.
- `ios/project.yml` and the checked-in Xcode project exclude x86_64 simulator
  builds because `OrbCodeFFI.xcframework` ships the required simulator slice as
  arm64.
- The next iPhone cleanup should target stale Spotlight qualified identifiers
  when a session changes assistant surface.

## Verification

- `cargo fmt --check --manifest-path crates/agent-control-plane/Cargo.toml`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test mobile_auth mobile_connection`
- `swift test --package-path ios/LooperCompanionCore`
- `swift test --package-path macos/LooperMenuBar`
- `DEVELOPER_DIR=/Applications/Xcode-beta.app xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -configuration Debug -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' build`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml`
- `git diff --check`

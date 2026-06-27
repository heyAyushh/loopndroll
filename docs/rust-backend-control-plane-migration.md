# Rust Backend Control Plane Status

## Direction

Rust is the backend and control-plane source of truth. Native macOS, native iOS,
the Rust CLI, and the Rust TUI are clients of the Rust HTTP/bootstrap and gRPC Session APIs.

## Rust Owns

- Local server lifecycle, status, shutdown, and event streaming.
- Supported agent session discovery, session details, desktop snapshots, and capabilities.
- Hook registration, live-hook unregistration, hook execution, and hook contract metadata.
- Session settings, per-session overrides, archive/delete/mute, queued prompts, and continuation modes.
- Completion-check configuration and execution.
- Slack and Telegram notification route management.
- Telegram chat discovery, stop-event delivery, delivery receipts, and reply bridge polling.
- Mobile connection-code/orb pairing, pairing-token management, passkey registration, passkey authentication, and passkey sessions.
- iPhone snapshot/detail, prompt/archive/delete/mute, default prompt, and push registration/test routes.
- SQLite control-plane persistence under `~/Library/Application Support/looper/agent-control-plane.sqlite`.

## Clients

- `looper` renders local server state in the current terminal and sends command mutations through the Rust API.
- `looper-cli` is kept as a compatibility binary for script callers; product help prefers `looper`.
- `macos/LooperMenuBar` starts the bundled Rust server and exposes local lifecycle actions.
- `ios/LooperCompanion` talks only to the Rust mobile API.

## Removed Surfaces

- The legacy JavaScript backend and renderer are removed from the active tree.
- The repository check no longer runs JavaScript backend, renderer, or TypeScript gates.
- `pnpm check` remains as a convenience wrapper for Rust, macOS, and iOS checks.

## Verification

Use these gates before claiming the split is healthy:

```bash
cargo fmt --check --manifest-path crates/agent-control-plane/Cargo.toml
cargo test --manifest-path crates/agent-control-plane/Cargo.toml
swift test --package-path macos/LooperMenuBar
xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -configuration Debug -destination 'generic/platform=iOS' build
pnpm check
```

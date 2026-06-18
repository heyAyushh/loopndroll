# Looper Rust and Swift Concurrency Sweep

This note records the current concurrency boundaries used by Looper's Rust
control plane, macOS menu bar client, and iPhone companion app.

## Rust Control Plane

- `crates/agent-control-plane` remains the backend source of truth for sessions,
  hooks, auth, mobile API, events, and ACP host state.
- Desktop ACP client-host response types live in
  `crates/agent-control-plane/src/acp_client_host/model.rs`; provider mapping
  logic lives in sibling modules such as `acp_client_host/devin.rs` and
  `acp_client_host/zed.rs`.
- Rust public response types should include rustdoc when they are shared across
  HTTP routes, Swift clients, or tests.
- Recoverable failures should flow through `Result`; panics and `expect` calls
  are acceptable only in tests or invariant-only code.
- Async Rust work should stay inside Tokio-managed tasks and use structured
  cancellation-friendly primitives where possible.

## Swift Clients

- SwiftUI view models and UI event handlers stay on `@MainActor`.
- Network and model helpers stay value-oriented and `Sendable` where possible;
  add `@unchecked Sendable` only when a type owns explicit synchronization.
- Long-lived `Task` handles must be cancelled on replacement, `deinit`, or
  lifecycle shutdown.
- HTTP route failover should start with the preferred route and defer fallback
  work. This avoids duplicate LAN/Tailscale requests during healthy refreshes
  while preserving failover for stale or slow routes.
- The iPhone route filter treats Tailscale CGNAT addresses as private,
  attemptable routes even when the server advertises plain HTTP. Tailscale's
  data plane is WireGuard-encrypted, while public HTTP remains blocked by the
  app's own route filter.
- The iPhone plist keeps ATS permissive enough for dynamic LAN and Tailscale
  IPs; security for this surface lives in the route classifier plus mobile API
  authentication instead of static per-domain ATS exceptions.
- SwiftUI render helpers should use stable domain IDs rather than fresh `UUID`
  values during every body pass.

## Verification

For changes in this area, use the smallest relevant subset first, then widen:

- Rust: `cargo fmt --check --manifest-path crates/agent-control-plane/Cargo.toml`
- Rust: targeted `cargo test --manifest-path crates/agent-control-plane/Cargo.toml <test-name>`
- Swift core: `DEVELOPER_DIR=/Applications/Xcode-beta.app swift test --package-path ios/LooperCompanionCore`
- iPhone app: Xcode device build plus `xcrun devicectl device install app`
- Runtime: pull `Library/Caches/looper-diagnostics.log` from the app container
  when route freshness or mobile behavior is changed.

## Source Guidance

- Apple Swift structured concurrency guidance emphasizes task trees,
  cancellation propagation, and task groups.
- Rust API Guidelines recommend documented public APIs, including explicit
  error and panic sections where applicable.
- Tokio's `select!` and task APIs are appropriate when control-plane async work
  needs first-completer or cancellation-aware coordination.

# Looper Rust Module Sweep

This note captures the Rust module boundaries tightened during the module
organization pass.

## ACP Client Hosts

- `crates/agent-control-plane/src/acp_client_host.rs` is the public facade for
  normalized ACP client-host responses and shared route helpers.
- `acp_client_host/model.rs` owns serialized HTTP response models and rustdoc
  for fields shared with clients.
- `acp_client_host/devin.rs` owns Devin Desktop mapping, because Looper can
  install the bridge, probe it, and observe runtime sessions.
- `acp_client_host/zed.rs` owns Zed mapping, because Zed owns External Agent
  installation, authentication, and runtime control. Looper exposes configured
  targets as read-only visibility only.

## Zed Detection

- `crates/agent-control-plane/src/zed.rs` reads Zed JSONC settings from the
  home or XDG path and redacts launch payload details before serialization.
- JSONC parsing accepts comments and trailing commas because Zed settings are
  hand-edited configuration files.
- Configured Zed ACP targets are not treated as ready executable targets.
  `ready=false` and `status=read-only` prevent the menu or phone from implying
  Looper can control Zed runtime behavior directly.

## Devin Session Discovery

- `DevinSessionDiscovery` now preserves source-level failures in
  `errors: Vec<DevinSessionDiscoveryError>` instead of forcing the whole
  snapshot to look empty.
- Discovery errors are sanitized into stable codes and generic details before
  they reach desktop or mobile clients.
- Mobile snapshots expose these diagnostics under
  `devinDesktop.sessionDiagnostics`, so stale or partial Devin data can be
  distinguished from a real zero-session state.

## HTTP Route Organization

- `crates/agent-control-plane/src/http/mod.rs` now keeps `build_router` as
  route-group composition instead of one long route chain.
- `http/events.rs` owns desktop/mobile SSE streams and the automation tail
  stream constants.
- `http/handoff.rs` owns the handoff HTML/deep-link renderer and escaping tests.
- `http/mobile_state.rs` owns shared mobile snapshot/state responses and mobile
  event emission helpers.
- `http/session_actions.rs` owns route-neutral session mutations for
  mode/archive/mute/delete. Desktop and mobile handlers keep their different
  auth and response contracts, but no longer duplicate mutation side effects.

## Codex Row Mapping

- `crates/agent-control-plane/src/codex.rs` now uses small row mappers for
  dynamic tools and spawn edges.
- The larger `codex.rs` split into state, rollout, hook, process, and SQLite
  modules remains a separate follow-up because it moves many tests and public
  helpers at once.

## Verification

Use these checks for this slice:

- `cargo fmt --check --manifest-path crates/agent-control-plane/Cargo.toml`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml handoff`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml mobile_session`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml codex`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml acp_client_host`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml zed`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml devin_discovery_keeps_good_sources_when_one_source_fails`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml assistant_adapters_detect_gui_and_cli_surfaces`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml desktop_connections_manage_mobile_pairings_and_codex_rows`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml`

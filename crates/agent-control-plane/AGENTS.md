# AGENTS.md - Rust Control Plane

## OVERVIEW

This crate owns Looper's backend/control-plane behavior: server, CLI, TUI, hooks, scheduler, mobile API, auth, events, persistence, notifications, and agent integrations.

## STRUCTURE

```text
crates/agent-control-plane/
├── src/bin/                 # agent-control-plane, looper, looper-cli, looper-server
├── src/acp/                 # ACP runtime, targets, stdio, client-host adapters
├── src/control_plane/       # Rust state/service coordination
├── src/http/                # HTTP routes and request models
├── src/mobile/              # iPhone-facing API, auth, network, prompt/session state
├── src/tui/                 # terminal UI
├── src/zed/, src/devin/     # editor/agent host integrations
└── tests/                   # isolated integration suites
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Server/hook startup | `src/runtime.rs`, `src/bin/*` | `looper-server` dispatches `serve` and `hook`. |
| CLI commands | `src/cli/` | `looper` and `looper-cli` both delegate here. |
| State owner | `src/control_plane.rs`, `src/control_plane/` | Keep DB/state mutations centralized. |
| Mobile API routes | `src/mobile/api.rs`, `src/mobile/api/` | `api.rs` is facade; logic belongs in focused modules. |
| Mobile session persistence | `src/mobile/session/` | Schema, queries, notifications, completion checks. |
| ACP host control | `src/acp/`, `src/control_plane/acp_hosts/`, `src/zed/`, `src/devin/` | Keep identity/probe/control paths in Rust. |
| Tests | `tests/isolated_control_plane.rs`, `tests/isolated_control_plane/`, `tests/mobile_auth.rs` | Integration-heavy safety net. |

## CONVENTIONS

- Do not add core Looper backend behavior outside this crate.
- Preserve the Rust module split. Add files under the matching domain folder instead of expanding facade files.
- Keep `looper` user-facing; treat `looper-cli` as compatibility.
- Use explicit manifest commands because there is no repo-root Cargo workspace.
- Keep legacy Bun/mobile import code isolated and named as migration compatibility; do not spread it into new paths.
- When mobile response models change, update iOS Swift models/tests in the same slice.

## ANTI-PATTERNS

- Do not put route logic, SQL queries, hook behavior, or session-control state in macOS/iOS clients.
- Do not handwave large `api.rs`, `http/mod.rs`, `cli/mod.rs`, or `control_plane.rs` growth; split by route/domain.
- Do not delete legacy import tests unless the migration path is intentionally removed and verified.
- Do not weaken `isolated_control_plane` fixtures to make a behavior pass.

## COMMANDS

```bash
cargo fmt --check --manifest-path crates/agent-control-plane/Cargo.toml
cargo test --manifest-path crates/agent-control-plane/Cargo.toml
cargo run --manifest-path crates/agent-control-plane/Cargo.toml --bin looper-server -- serve
cargo build --manifest-path crates/agent-control-plane/Cargo.toml --bin looper --bin looper-server
```

## TEST HOTSPOTS

- `tests/isolated_control_plane.rs`: broad route/session/mobile/hook/ACP coverage.
- `tests/isolated_control_plane/acp_hosts/`: Zed/Devin/probe/websocket/remote-control assertions.
- `src/mobile/api/*_tests.rs`: iPhone-facing snapshot/status/prompt behavior.
- `src/mobile/session/tests.rs`: prompt modes, lifecycle, completion, legacy import.

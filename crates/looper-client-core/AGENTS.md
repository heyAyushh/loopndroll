# AGENTS.md - Rust Client Core

## OVERVIEW

`crates/looper-client-core` owns Looper's rewrite-only client runtime: state-mini sync, local reducer projections, durable pending command state, endpoint recovery, and the UniFFI surface consumed by iOS and macOS.

This crate is a client of the Rust control plane. It must not become another backend owner.

## STRUCTURE

```text
crates/looper-client-core/
├── src/client.rs              # in-memory runtime, stream, outbox, optimistic state
├── src/session_runtime.rs     # UniFFI object and durable command entry points
├── src/local_store.rs         # JSON local snapshot and pending command persistence
├── src/session_transport.rs   # Session gRPC/state-mini transport
├── src/*_snapshot.rs          # mobile/menu projection reducers
├── src/snapshot_reducer.rs    # reusable UI/session projections
└── uniffi-bindgen.rs          # local UniFFI bindgen binary entry
```

## WHERE TO LOOK

| Task | Location | Notes |
| --- | --- | --- |
| Runtime lifecycle | `src/client.rs`, `src/session_runtime.rs` | Start/stop/observe, local replay, command flushing, ack handling. |
| Pending commands | `src/local_store.rs`, `src/command_batch.rs` | Latest-wins and retry behavior must stay durable and mutation-id based. |
| State-mini transport | `src/session_transport.rs`, `src/transport.rs` | Normal live path is Session gRPC/state-mini; HTTP is bootstrap/recovery only. |
| iOS/macOS projections | `src/mobile_snapshot.rs`, `src/menu_snapshot.rs`, `src/snapshot_reducer.rs` | Keep UI projections deterministic and testable here. |
| Public bridge surface | `src/lib.rs`, `src/model.rs`, `src/session_runtime.rs` | Anything exported here reaches generated Swift. |
| Boundary guard | `../../scripts/check-client-core-boundaries.py` | Enforces no inward server deps and no retired client runtime surfaces. |

## CONVENTIONS

- Keep Rust client-core as the source of truth for local state-mini snapshots, pending commands, mutation IDs, and optimistic reducer output.
- Keep backend/session FSM truth in `crates/agent-control-plane`; this crate mirrors server acks/rejects and owns client-side durability.
- Latest-wins command classes must be coalesced by command kind, target, and mutation identity so stale results cannot overwrite newer desired state.
- Add tests beside the reducer/runtime code when changing ordering, persistence, recovery, or projection semantics.
- Use explicit manifest commands; there is no repo-root Cargo workspace.
- Regenerate Swift bindings and the XCFramework through `scripts/build-looper-client-core-package.sh` after exported UniFFI changes.

## ANTI-PATTERNS

- Do not depend on `agent-control-plane`, SQLite, HTTP command routes, legacy event streams, or retired unary command response models.
- Do not move command lifecycle ownership back into `CompanionAppModel`, macOS clients, or generated Swift.
- Do not make HTTP the normal live sync path for session commands.
- Do not hand-edit artifacts under `swift/LooperClientCore/Sources/LooperClientCore/Generated` or `swift/LooperClientCore/Frameworks`.

## COMMANDS

```bash
cargo fmt --check --manifest-path crates/looper-client-core/Cargo.toml
cargo test --manifest-path crates/looper-client-core/Cargo.toml
bash scripts/check-client-core-boundaries.sh
bash scripts/check-client-core-boundaries.sh --strict-runtime
bash scripts/build-looper-client-core-package.sh
```

## TEST HOTSPOTS

- `src/client.rs`: stream startup, optimistic updates, command ack backlog, outbox behavior.
- `src/local_store.rs`: pending command durability, latest-wins filtering, snapshot recovery.
- `src/session_runtime.rs`: UniFFI command intent entry points and local store replay.
- `src/session_transport.rs`: Session transport and state-mini stream handling.
- `src/snapshot_reducer.rs`: assistant surface, Siri/default session, freshness, and UI projection reducers.

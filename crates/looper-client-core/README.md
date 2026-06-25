# looper-client-core

Rust client core boundary for Looper's rewrite-only realtime architecture.

This crate owns client-side state mirroring, state-mini reduction, optimistic
mutation tracking, and the outbound `Session` frame queue that iOS, macOS, and
the TUI consume. It does not depend on the server crate, SQLite, agent adapters,
HTTP command routes, or legacy event streams.

## Rust checks

```bash
cargo fmt --check --manifest-path crates/looper-client-core/Cargo.toml
cargo test --manifest-path crates/looper-client-core/Cargo.toml
bash scripts/check-client-core-boundaries.sh --strict-runtime
```

## UniFFI / XCFramework path

Build the attachable Swift package and XCFramework with:

```bash
bash scripts/build-looper-client-core-package.sh
```

The script follows the same Rust-staticlib to XCFramework pattern as
`orb-code`, generates Swift UniFFI bindings from the host library, and writes the
package artifacts under `swift/LooperClientCore`.

## Integration notes

- Protocol transport is intentionally modeled as outbound `Session` frames. E's
  protocol cut owns the generated gRPC contract.
- Session-mini replace/apply semantics live here so Swift clients can render
  immutable Rust-owned snapshots before the network confirms.
- FSM transition semantics still live in the control plane. The client core only
  mirrors server acks/rejects and keeps local pending command state.
- `scripts/check-client-core-boundaries.py --strict-runtime` prevents this
  crate from taking inward dependencies on the control plane and keeps clients
  from reintroducing removed event-stream or unary command runtime paths.

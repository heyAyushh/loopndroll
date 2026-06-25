# looper-client-core

Rust client core boundary for Looper's rewrite-only realtime architecture.

This crate owns client-side state mirroring, optimistic mutation tracking, and
the outbound `Session` frame queue that iOS, macOS, and the TUI consume. It does
not depend on the server crate, SQLite, agent adapters, HTTP command routes, or
legacy event streams.

## Rust checks

```bash
cargo fmt --check --manifest-path crates/looper-client-core/Cargo.toml
cargo test --manifest-path crates/looper-client-core/Cargo.toml
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
- Reducer and FSM semantics are intentionally placeholders at this boundary. H's
  reducer/FSM slice owns the final shared Rust modules; this crate is where they
  should be reused after integration.
- `scripts/check-client-core-boundaries.py` prevents this crate from taking
  inward dependencies on the control plane, local stores, removed event-stream
  transports, or unary command request types.

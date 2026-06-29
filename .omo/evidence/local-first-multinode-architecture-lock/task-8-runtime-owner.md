# Todo 8 Runtime Owner Evidence

Date: 2026-06-29

## Scope

Todo 8 recovered WIP was coherent. This finish pass stayed inside the existing Todo 8 WIP files plus task-8 evidence. Route/connectivity model files, macOS, server, game surfaces, `.git`, `.env`, and credential files were not touched.

## Rust-Owned Runtime And Store Boundary

- `LooperClientCoreSessionRuntime::recover_state_mini_snapshot` now asks `LooperClientCoreLocalStore` to mark the persisted last-good endpoint before recovery, so endpoint preference stays Rust-owned.
- `LooperClientCoreSessionRuntime::start` still returns `Connecting` with an empty endpoint before a stream proves readiness. `runtime_persists_first_ready_fallback_endpoint_as_last_good` proves a stale last-good endpoint does not make the runtime claim connected; the first live fallback heartbeat marks `Ready` and persists that fallback as last-good.
- `ClientCoreState` and `LooperClientCoreLocalStore` reject stale recovery snapshots per node. Mixed recovery batches update fresh nodes while preserving newer cached minis for stale nodes.
- Pending commands and local outbox state remain in Rust. `reduce_state_minis_mobile_snapshot_with_pending_commands` overlays pending command effects in Rust before Swift decodes a mobile snapshot.

## Swift Boundary Scan

- `ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift` no longer owns the pending-command reducer overlay. It calls `reduceStateMinisMobileSnapshotWithPendingCommands(...)` and decodes the Rust-emitted snapshot.
- Scan command: `rg -n "applyPendingClientCoreCommands|applyPendingMode|applyPendingArchive|applyPendingDelete|reduceStateMinisMobileSnapshotWithPendingCommands|reduce_state_minis_mobile_snapshot_with_pending_commands" ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift swift/LooperClientCore/Sources/LooperClientCore crates/looper-client-core/src/mobile_snapshot.rs`
- Result: old Swift reducer helpers are absent; only the Rust export, generated Swift binding, and Swift call site remain.

## Changed APIs

- Added Rust UniFFI export: `reduce_state_minis_mobile_snapshot_with_pending_commands`.
- Re-exported the new projection helper from `crates/looper-client-core/src/lib.rs`.
- Regenerated `swift/LooperClientCore` with `bash scripts/build-looper-client-core-package.sh`.
- Added Swift package coverage for `reduceStateMinisMobileSnapshotWithPendingCommands`.

## Focused Gates

- `cargo test --manifest-path crates/looper-client-core/Cargo.toml endpoint -- --nocapture` -> 14 passed, `exit=0`; see `task-8-endpoint.txt`.
- `cargo test --manifest-path crates/looper-client-core/Cargo.toml command -- --nocapture` -> 21 passed, `exit=0`; see `task-8-command.txt`.
- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore --filter RealtimeBridge` -> no matching tests, `exit=0`; see `task-8-bridge.txt`.
- `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore --filter mobileProjection` -> 3 passed, `exit=0`; see `task-8-bridge-closest.txt`.
- `cargo test --manifest-path crates/looper-client-core/Cargo.toml last_seq_by_node -- --nocapture` -> 3 passed, `exit=0`; see `task-8-last-seq-by-node.txt`.
- `cargo test --manifest-path crates/looper-client-core/Cargo.toml recovered_snapshot_preserves_newer_node_mini_when_global_seq_advances -- --nocapture` -> 1 passed, `exit=0`; see `task-8-recovered-snapshot.txt`.
- `cargo test --manifest-path crates/looper-client-core/Cargo.toml mobile_snapshot_applies_pending_commands_in_rust_projection -- --nocapture` -> 1 passed, `exit=0`; see `task-8-mobile-snapshot.txt`.
- `cargo fmt --manifest-path crates/looper-client-core/Cargo.toml --check` -> `exit=0`; see `task-8-cargo-fmt-check.txt`.
- `git diff --check` -> `exit=0`; see `task-8-git-diff-check.txt`.

## Dirty Tree Handling

- Initial tracked WIP matched the requested Todo 8 files and generated Swift package files.
- Post-status for the scoped files is saved in `task-8-status-post.txt`.
- Unrelated untracked files already present in the tree remain unstaged. Notable scoped-status unrelated entry: `swift/LooperClientCore/AGENTS.md`.
- Task 5 evidence already under `.omo/evidence/local-first-multinode-architecture-lock/` was not staged or modified.

## Risks And Out Of Scope

- No installed iOS/macOS proof was run; Todo 8 requested focused client-core gates only.
- No separate route/connectivity commit was merged. Commit `9c32912f` remains for the later root integration task.
- `RealtimeBridge` has no matching Swift package tests in `swift/LooperClientCore`; closest changed API coverage is the `mobileProjection` filter.

# Task 3 Repair: Desktop Limited Snapshot Boundary

## Fixed boundary

- `/desktop/snapshot?limit=30` enters `desktop_snapshot_with_thread_limit`, which uses `SnapshotInspectionMode::CachedMenu`.
- `SnapshotInspectionMode::CachedMenu` now returns `false` from `discovers_live_external_sessions` at `crates/agent-control-plane/src/control_plane.rs:136`.
- `desktop_snapshot_with_limits` now routes Grok, Claude, and Devin session loading through `external_sessions_for_snapshot` at `crates/agent-control-plane/src/control_plane.rs:1537`.
- `external_sessions_for_snapshot` returns `SnapshotExternalSessions::stale()` before calling `discover_grok_sessions`, `claude_sessions_for_snapshot`, or `devin_sessions_for_snapshot` when the mode is `CachedMenu` at `crates/agent-control-plane/src/control_plane.rs:1782`.
- The stale bundle has empty Grok, Claude, and Devin session lists plus stale Grok hook status at `crates/agent-control-plane/src/control_plane.rs:491` and `crates/agent-control-plane/src/control_plane.rs:2376`.

## Focused regression

- `desktop_limited_snapshot_skips_request_time_transcript_preview` now seeds live-looking Grok, Claude, and Devin fixture data at `crates/agent-control-plane/tests/isolated_control_plane.rs:258`.
- The test requests `/desktop/snapshot?limit=30` and asserts:
  - Codex transcript preview fields remain null.
  - `grok_build.session_count`, `grok_build.active_session_count`, `devin_session_count`, and `devin_active_session_count` stay zero.
  - The live-discovery thread ids `grok-live-leak`, `claude:claude-live-leak`, and `devin:devin-cli:brindle-cadet` are absent.
- This test would fail if the limited snapshot invoked request-time Grok, Claude, or Devin discovery.

## Diagnostics path

- Full `/desktop/snapshot` still uses `SnapshotInspectionMode::Live` through `desktop_snapshot`.
- That live path remains explicit manual diagnostics/producer inspection and is out of Todo 3 repair scope.

## Verification

- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane desktop_limited_snapshot -- --nocapture | tee .omo/evidence/local-first-multinode-architecture-lock/task-3-desktop-limited-rerun.txt`
  - Result: 1 passed, 0 failed.
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane session_mini_snapshot -- --nocapture | tee .omo/evidence/local-first-multinode-architecture-lock/task-3-session-mini-rerun.txt`
  - Result: 4 passed, 0 failed.
- `cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check`
  - Result: passed with no diff.
- `git diff --check`
  - Result: passed with no whitespace errors.

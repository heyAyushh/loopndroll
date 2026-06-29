# Task 7 Oversized Mini Repair

## Reviewer Blocker

- The single-mini `StateMiniDelta` path still used `bounded_value`/`truncate_chars` during SessionMini compaction.
- `grpc_session_stream_compacts_single_oversized_session_mini_delta` asserted a plausible truncated normal mini instead of recovery or rejection.

## Repair

- Removed SessionMini text truncation from normal compaction. Allowed mini fields are preserved as source state; unsupported embedded control/detail fields are still removed.
- Added recovery handling for a single SessionMini delta whose encoded `ServerFrame` exceeds the cap. It now emits the same small recovery-only payload used for an oversized replacement mini:
  - `controlOnly=true`
  - `reason=projection-frame-cap-exceeded`
  - `recoveryRequired=true`
  - `recovery=session-mini-snapshot`
  - no `sessions`, `title`, or `assistantPreview` false state
- Renamed the focused regression to `grpc_session_stream_recovers_single_oversized_session_mini_delta` and changed it to assert the recovery instruction.

## Evidence

- `.omo/evidence/local-first-multinode-architecture-lock/task-7-oversized-mini-repair.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-7-frame-caps-rerun.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-7-large-replacement-rerun.txt`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml compact_session_mini -- --nocapture`
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml compact_metadata -- --nocapture`
- `cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check`
- `git diff --check`

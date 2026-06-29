# Task 7 Session Frame Caps

## Cap Values

- Session `ClientFrame`/`ServerFrame` encoded protobuf cap: 512 KiB (`SESSION_CONTROL_FRAME_MAX_BYTES`).
- Replacement `StateMiniDelta` chunk payload cap: 508 KiB, reserving 4 KiB for protobuf/frame overhead.
- Command text field cap: 64 KiB.
- Control text display cap: 512 chars for already-small diagnostic text.

## Tested Cases

- `grpc_session_frame_payload_rejects_oversized_prompt_command`: over-cap prompt is rejected as an ACK with `error_code=resource_exhausted`; no state event is appended.
- `grpc_session_frame_payload_instructs_recovery_for_oversized_replacement_mini`: a malformed replacement mini that cannot fit one bounded replacement frame emits one small control-only delta with `reason=projection-frame-cap-exceeded`, `recoveryRequired=true`, and `recovery=session-mini-snapshot`; no `sessions` array is emitted, so no partial false replacement state is created.
- `grpc_session_stream_replays_large_projection_replacement_under_frame_cap`: large valid replacement projections still split into bounded replacement chunks, and every encoded state delta stays under the 512 KiB test cap.

## ACK And Hash Metadata

- No proto `CommandAck` fields were added.
- ACK frames remain compact finality metadata: accepted/rejected status, mutation id, account/node/entity ids, seq, revision, server time, replay flag, and reject fields.
- No content blob, content hash, state hash, SHA-256, or Merkle metadata was added to Session control frames. Content hashing remains data-plane only.

## Evidence

- Red/failing-first: `.omo/evidence/local-first-multinode-architecture-lock/task-7-red-frame-caps.txt`.
- Green focused gate: `.omo/evidence/local-first-multinode-architecture-lock/task-7-frame-caps.txt`.
- Adjacent replacement gate: `.omo/evidence/local-first-multinode-architecture-lock/task-7-large-replacement.txt`.

## Adversarial Classes

- malformed_input: over-cap prompt rejects cleanly with `resource_exhausted`; malformed oversized replacement emits recovery instruction.
- stale_state: replacement over cap does not emit a partial `sessions` replacement.
- dirty_worktree: pre-existing untracked evidence/worktree files were left untouched; task edits are scoped to Rust gRPC service/tests and task-7 evidence.
- misleading_success_output: final acceptance commands use `set -o pipefail` with `tee`; the initial red run is kept separately because plain `tee` can mask cargo failure.
- hung_or_long_commands: only focused cargo filters and fmt/diff checks were run; no iOS/macOS builds or installs.
- generated_files: not applicable; no generated Swift/protobuf files were edited.
- external_config: not applicable; no files outside the repo or credential/config files were written.

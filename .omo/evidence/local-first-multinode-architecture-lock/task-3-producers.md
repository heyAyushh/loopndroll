# Task 3 Producer Boundary

## Producer locations

- `crates/agent-control-plane/src/control_plane.rs:607` runs `reconcile_mobile_session_mini_projection` as the bounded producer path. It now uses the live inspection snapshot there, primes prompt delivery actions, computes `SessionMini` projection inputs, appends a replacement mobile state event, and publishes the record.
- `crates/agent-control-plane/src/control_plane.rs:631` builds projection rows in the producer before `record_mobile_event_replacing_session_minis`.
- `crates/agent-control-plane/src/control_plane.rs:1446` keeps `/desktop/snapshot?limit=30` on `SnapshotInspectionMode::CachedMenu`, so limited recovery snapshots skip request-time transcript preview parsing.

## Drain-only request and stream proof

- `crates/agent-control-plane/src/http/mobile_state.rs:54` now returns cached mini projection records only.
- `crates/agent-control-plane/src/http/mobile_state.rs:129` accepts cached records only when a produced replacement baseline exists, and returns the latest produced projection seq rather than rebuilding to the newest event seq.
- `crates/agent-control-plane/src/http/mobile_state.rs:160` returns a bounded `recovery_required` marker when the projection is missing or partial.
- `crates/agent-control-plane/src/mobile/prompt_delivery.rs:130` resolves prompt delivery from the producer-primed cache. The snapshot fallback was removed from prompt acceptance.
- Source scan found no `rebuild_mobile_session_mini_projection`, no prompt-delivery snapshot fallback, and no transcript/rollout scan calls under `src/http` or `src/grpc`.

## Tests and artifacts

- Red baseline: `task-3-red.txt` showed `/api/mobile/session-minis/snapshot` returned `200` by rebuilding on request.
- Red baseline: `task-3-red-desktop.txt` showed `/desktop/snapshot?limit=30` returned transcript preview text from request-time JSONL parsing.
- Required focused test artifact: `task-3-tests.txt`.
- Extra focused artifacts: `task-3-desktop-limited-test.txt`, `task-3-session-mini-projection-tests.txt`, `task-3-producer-reconcile-test.txt`.
- Format/whitespace gates: `task-3-cargo-fmt-check.txt`, `task-3-git-diff-check.txt`.

## Sample

- `task-3-snapshot.sample.txt` was captured against the already-running `/Applications/looper.app/Contents/MacOS/looper-server`, not a rebuilt workspace server.
- That installed binary still showed the old request-time `refresh_thread_rollout_paths` and transcript preview stacks, and `/desktop/snapshot?limit=30` timed out under the 2s curl. Treat this as baseline/blocker evidence for the installed binary, not acceptance proof for this source patch.
- Current workspace proof is the focused Rust tests above. I did not start a second workspace server because that would bind another Looper instance against user-level Codex state outside the project root.

## Cleanup

- No server process was started by this task.
- The existing installed `looper-server` process was sampled only and left running.
- No iOS, macOS, client-core, generated, Pinball, Maze, `.git`, `.env`, or credential files were touched.

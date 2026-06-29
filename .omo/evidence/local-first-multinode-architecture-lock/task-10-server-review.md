# Todo 10 Server Review

Verdict: needs-fix
codeQualityStatus: BLOCK
recommendation: REQUEST_CHANGES

Reviewed commit: `df3bec205105d6919c82dcc2165dd9e191a76ab2`
Plan: `.omo/plans/local-first-multinode-architecture-lock.md`

## Scope

Reviewed the Todo 10 server commit against the architecture-lock acceptance criteria:

- HTTP recovery/snapshot responses must expose enough `latestSeq` / `latest_seq`, `revision`, `serverTime` / `server_time`, and freshness metadata for clients to reject stale snapshots.
- Server HTTP must stay health/bootstrap/recovery/manual diagnostics/content only, with no old HTTP command/state hot path, SSE, or unary command revival.
- Strict grep and server evidence artifacts must be honest; generated Swift/iOS test hits are out of server scope.

Skill-perspective check: the named `remove-ai-slops` and `programming` skills were not available in the provided available-skill list. I applied the documented criteria from the review prompt. The diff does not add deletion-only, tautological, or implementation-constant-only tests, and it does not add obvious needless production parsing/normalization. The blocker below is a correctness/test-coverage gap: the tests cover stale projection on the snapshot route but not on the `after_seq` recovery delta route.

Web evidence check: consulted MDN HTTP caching and Date header references before replying, as required by repo instructions. MDN defines freshness/staleness around response age and notes that HTTP `Date`/response time is message-origin metadata, which is consistent with needing explicit server-time/freshness metadata in recovery responses.

## Evidence Inspected

- Commit diff for:
  - `crates/agent-control-plane/src/http/mobile_state.rs`
  - `crates/agent-control-plane/src/mobile/api/session_mini.rs`
  - `crates/agent-control-plane/src/mobile/api/snapshot.rs`
  - `crates/agent-control-plane/tests/isolated_control_plane.rs`
- Plan lines for Todo 10 and verification strategy.
- Artifacts:
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-server.md`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-strict-grep.txt`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-http-route-classification-grep.txt`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-session-mini-snapshot.txt`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-mobile-snapshot.txt`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-session-mini-unit.txt`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-cargo-fmt-check.txt`
  - `.omo/evidence/local-first-multinode-architecture-lock/task-10-git-diff-check.txt`

Focused tests rerun:

- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml session_mini_snapshot -- --nocapture` passed: 5 focused integration tests.
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml mobile_snapshot_uses_rust_auth_and_codex_threads -- --nocapture` passed: 1 focused integration test.
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml legacy_bloated_rows_replay_under_control_frame_cap -- --nocapture` passed: 1 focused unit test.

Strict grep rerun matched the artifact: only generated Swift enum cases and iOS tests mention `setAssistantSurface`; no server hits for the old SSE/event routes or HTTP `sendSessionPrompt` / `setSessionMode` / `submitNotificationReply` patterns.

## Findings

### CRITICAL

None.

### HIGH

- `crates/agent-control-plane/src/http/mobile_state.rs:133` - The `/api/mobile/session-minis?after_seq=...` recovery delta path can advertise the event-log `latestSeq` while returning stale mini projection rows. The snapshot path correctly uses the projected mini seq via `cached_mobile_session_mini_projection()` (`mobile_state.rs:162`), so `session_mini_snapshot_exposes_projection_seq_when_event_log_is_newer` proves `/api/mobile/session-minis/snapshot` is rejectable by stream-newer clients. The delta path does not use that projected seq. It reads mini rows with `mobile_session_minis_after_seq()` (`events.rs:497`), then independently reads `latest_mobile_state_event_seq()` (`mobile_state.rs:133`) and passes that newer event-log seq into `mobile_session_mini_delta()` (`mobile_state.rs:148`) even when `payload_records` are just `all_records` from the older mini projection (`mobile_state.rs:141`). If a non-projected mobile event exists after the mini projection, this response can say `replace: true` with the newer `latestSeq` while carrying older records/revision. That violates the Todo 10 stale-overwrite requirement because an HTTP recovery response can masquerade as current and overwrite or advance a client past newer stream state. Existing tests do not cover this route; the stale-projection test calls `/api/mobile/session-minis/snapshot` at `isolated_control_plane.rs:2588`, not `/api/mobile/session-minis?after_seq=...`.

  Blocker: make the `after_seq` recovery delta response use projected mini freshness consistently, or return `409 recovery_required`/snapshot recovery when the event log is ahead of the mini projection. Add a focused test that appends a newer non-projected mobile event, calls `/api/mobile/session-minis?after_seq=<projection_seq>`, and proves the response cannot advertise the newer event seq with stale mini rows.

### MEDIUM

None.

### LOW

None.

## Confirmed

- `/api/mobile/snapshot` includes top-level and nested freshness metadata: `revision`, `latestSeq` / `latest_seq`, `serverTime` / `server_time`, `snapshotKind` / `snapshot_kind`, and `freshness` (`crates/agent-control-plane/src/mobile/api/snapshot.rs:33`).
- `/api/mobile/session-minis/snapshot` includes top-level and nested freshness metadata and uses compact recovery payloads (`crates/agent-control-plane/src/mobile/api/session_mini.rs:256`).
- The snapshot route exposes projected freshness when the event log is newer than the mini projection (`crates/agent-control-plane/src/http/mobile_state.rs:162`; test at `crates/agent-control-plane/tests/isolated_control_plane.rs:2555`).
- Route classification is broadly honest for this server cutter: old desktop session-state mutation routes are disabled, mobile routes are health/connection/snapshot/session-mini/content/passkey/push surfaces, and the strict old-transport grep has no server hits.

## Blockers

- Fix the `after_seq` session-mini recovery delta freshness mismatch described in the HIGH finding before approval.

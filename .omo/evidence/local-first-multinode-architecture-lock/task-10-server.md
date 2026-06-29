# Todo 10 server/recovery evidence

## Scope

- Role: Todo 10 server/recovery cutter.
- Write boundary used: `crates/agent-control-plane/**` plus this evidence artifact.
- Explicitly not touched by this cutter: iOS/macOS Swift/UI files, generated Swift, game files, app installs, broad gates.
- Pre-existing tracked out-of-scope edits observed and not staged by this cutter:
  - `ios/LooperCompanion/App/CompanionAppModel.swift`
  - `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`

## Changed files

- `crates/agent-control-plane/src/http/mobile_state.rs`
- `crates/agent-control-plane/src/mobile/api/session_mini.rs`
- `crates/agent-control-plane/src/mobile/api/snapshot.rs`
- `crates/agent-control-plane/tests/isolated_control_plane.rs`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-server.md`

## Evidence outputs

- `.omo/evidence/local-first-multinode-architecture-lock/task-10-session-mini-snapshot.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-mobile-snapshot.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-session-mini-unit.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-cargo-fmt-check.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-git-diff-check.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-strict-grep.txt`
- `.omo/evidence/local-first-multinode-architecture-lock/task-10-http-route-classification-grep.txt`

## Behavior

- `/api/mobile/snapshot` now carries top-level `latest_seq` / `latestSeq`, `revision`, `server_time` / `serverTime`, `snapshot_kind` / `snapshotKind`, and a `freshness` object sourced from `desktop-mobile-snapshot`.
- `/api/mobile/session-minis/snapshot` and `/api/mobile/session-minis?after_seq=...` now carry top-level `latest_seq` / `latestSeq`, `revision`, `server_time` / `serverTime`, `snapshot_kind` / `snapshotKind`, and a `freshness` object sourced from `mobile-session-mini-projection`.
- `seq_gap` and `recovery_required` responses now also include `revision`, `serverTime`, and the same freshness block so clients can reject stale HTTP recovery snapshots against newer stream state.
- A focused stale-projection test appends a newer mobile event without updating the mini projection and proves HTTP recovery reports the older projected seq/revision, not the newer non-projected event revision.

## Strict runtime grep

- Command:
  `rg -n "text/event-stream|MobileEventStream|LooperRealtimeStateMiniSynchronizer|/mobile/events|/desktop/events|HTTP.*sendSessionPrompt|HTTP.*setSessionMode|HTTP.*submitNotificationReply|loadSnapshot\\(\\).*sendSessionPrompt|setAssistantSurface" ios macos swift crates/agent-control-plane crates/looper-client-core`
- Output path: `.omo/evidence/local-first-multinode-architecture-lock/task-10-strict-grep.txt`
- Result: 10 hits, all outside this cutter boundary:
  - `swift/LooperClientCore/Sources/LooperClientCore/Generated/looper_client_core.swift` enum cases for `setAssistantSurface` (generated Swift, out of scope).
  - `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift` assertions proving assistant surface commands are not pending (iOS tests, out of scope).
- Server result: no `crates/agent-control-plane` hit for old SSE/event routes or HTTP `sendSessionPrompt` / `setSessionMode` / `submitNotificationReply`.

## HTTP classification

- Health:
  - `/health`
  - `/api/mobile/health`
- Pairing/auth handoff:
  - `/desktop/pairing`
  - `/desktop/pairing-orbs/:orb_id`
  - `/api/mobile/connection-code`
  - `/api/mobile/connection-orb.png`
  - `/api/mobile/connection-orbs/:orb_id`
  - `/api/mobile/passkeys/registration-challenge`
  - `/api/mobile/passkeys/register`
  - `/api/mobile/passkeys/authentication-challenge`
  - `/api/mobile/passkeys/authenticate`
  - `/api/mobile/passkeys/:credential_id`
- Explicit snapshot/bootstrap/recovery/manual diagnostics:
  - `/desktop/snapshot`
  - `/desktop/mobile-state`
  - `/desktop/sessions/:thread_id`
  - `/api/mobile/snapshot`
  - `/api/mobile/session-minis`
  - `/api/mobile/session-minis/snapshot`
- Bounded data-plane content:
  - `/api/mobile/sessions/:thread_id/content`
- Settings/desktop diagnostics not hot session truth:
  - `/status/control-plane`, `/automations`, `/goal`, `/goals`, `/assistant-adapters`, `/desktop/acp-targets`, `/codex/servers`, `/codex/compactions`, `/sync/manifest`, `/threads`, `/threads/:thread_id`, `/threads/:thread_id/capabilities`
  - Mobile connection/device management routes mutate auth/push connection records, not visible session state.
  - Hook and ACP host routes are existing local desktop integration surfaces, not mobile HTTP session-state truth.
- Disabled old HTTP mutation/control remnants:
  - `crates/agent-control-plane/src/http/session_actions.rs` returns `410 Gone` with `http_session_state_mutation_disabled` and recovery path `/api/mobile/session-minis/snapshot`.
  - Disabled desktop settings/session routes include default prompt, scope, assistant surface, global preset, global notification, notification upsert/delete, completion check upsert/delete, session notifications, session completion check, archive, mute, and delete. They do not mutate session truth; hot commands must use the Session stream.
- Classification grep path:
  `.omo/evidence/local-first-multinode-architecture-lock/task-10-http-route-classification-grep.txt`

## Tests and gates

- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml session_mini_snapshot -- --nocapture`
  - Output: `.omo/evidence/local-first-multinode-architecture-lock/task-10-session-mini-snapshot.txt`
  - Result: passed, 5 focused integration tests.
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml mobile_snapshot_uses_rust_auth_and_codex_threads -- --nocapture`
  - Output: `.omo/evidence/local-first-multinode-architecture-lock/task-10-mobile-snapshot.txt`
  - Result: passed, 1 focused integration test.
- `cargo test --manifest-path crates/agent-control-plane/Cargo.toml legacy_bloated_rows_replay_under_control_frame_cap -- --nocapture`
  - Output: `.omo/evidence/local-first-multinode-architecture-lock/task-10-session-mini-unit.txt`
  - Result: passed, 1 focused unit test.
- `cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check`
  - Output: `.omo/evidence/local-first-multinode-architecture-lock/task-10-cargo-fmt-check.txt`
  - Result: passed, empty output.
- `git diff --check`
  - Output: `.omo/evidence/local-first-multinode-architecture-lock/task-10-git-diff-check.txt`
  - Result: passed, empty output.

## Not run

- No installs.
- No broad iOS/macOS/client-core gates.
- No Swift/UI/generated/game edits by this cutter.

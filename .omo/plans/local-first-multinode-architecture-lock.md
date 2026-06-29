# local-first-multinode-architecture-lock - Work Plan

## TL;DR (For humans)

**What you'll get:** Looper becomes a local-first realtime app built around an account projection over multiple authoritative Looper nodes. Each Mac/Linux node owns the sessions it runs, its ordered event log, and its compact ACK finality certificates; iOS/macOS merge those per-node replicas locally. Taps, tab switches, mode changes, prompt sends, route changes, and notification replies render from local state first and reconcile through Rust-owned per-node Session runtimes.

**Why this approach:** The current failure class is multi-owner state drift: HTTP snapshots, Swift view caches, Rust projections, AppIntents, and macOS refresh logic can each claim truth. The architecture lesson is to remove whole latency classes: keep control/finality frames tiny, move discovery/content to producer/data-plane paths, and never run repair work on the hot path.

**What it will NOT do:** It will not preserve old HTTP command/state hot paths, add a second realtime socket, move full transcripts through Session frames, or treat “stream connected” as proof that projection data is fresh.

**Effort:** Large
**Risk:** High - crosses Rust server, Rust client-core, generated UniFFI, iOS, macOS, diagnostics, and install proof.
**Decisions to sanity-check:** Adopt node-authoritative event sourcing with account-level routing; add data-plane content chunks with ranges, SHA-256 chunk hashes, and Merkle roots per content revision; keep HTTP only for pairing, health, and explicit recovery; require profiler/ETTrace proof for switcher flicker.

Your next move: approve this plan for execution in the main thread, or ask for a high-accuracy plan review first. Full execution detail follows below.

---

> TL;DR (machine): Large/high-risk architecture lock; deliver federated single-writer RSMs, compact finality ACKs, bounded data plane with Merkle-verified chunks, Rust client-core ownership, honest iOS/macOS freshness, and installed-app proof.

## Scope

### Must have

- Name and implement the architecture as **federated single-writer replicated state machines with compact finality certificates, a separated data plane, and local-first account projections**.
- The Account Plane owns identity, device registry, APNs token registry, node registry, route directory, and optional relay/queueing. It routes; it does not decide session truth.
- Each Looper node is the only writer of durable truth for sessions it owns: commands append ordered per-node events; projections are derived from those events.
- `CommandAck` is the finality certificate from the owner node: `accepted`, `client_mutation_id`, `account_id`, `node_id`, `entity_id`, `ack_seq`, `revision`, `server_time`, and reject fields are enough for clients to unblock or roll back.
- Session stream remains the only hot control channel, scoped per live node: commands up, ACKs/deltas/events/heartbeats down. One stream means one stream per node, not one global account socket.
- Full transcript/log/output/search/attachments move through a bounded data plane: latest tail, page before/after cursor, search context, or blob by id.
- Data-plane content chunks include `account_id`, `node_id`, `session_id`, `revision`, `offset`, `length`, `sha256`, `next_cursor`, and optional Merkle proof against the transcript/log revision root.
- iOS/macOS render local replicas first; route/session/freshness labels are honest and cannot claim connected/fresh until Rust client-core proves the live endpoint and latest seq.
- SwiftUI/AppKit own lifecycle only: create/start/stop/cancel/observe; no Swift reducer, no Swift transport truth, no HTTP command/state truth.
- Existing Pinball/Maze/game surfaces are out of scope and must not be touched.
- Every wave includes agent-executable QA with artifact paths. Final proof includes installed macOS app, installed iPhone app, profiler/ETTrace or OSLog evidence for switcher behavior, and p95 latency measurements.

### Must NOT have (guardrails, anti-slop, scope boundaries)

- No extra hot socket per node beyond the per-node Session stream.
- No SSE or event-stream revival.
- No unary gRPC command compatibility.
- No HTTP mode/prompt/reply/session-state hot path.
- No request-time transcript/rollout/session discovery in Session stream pollers, snapshot handlers, or UI refresh paths.
- No “stream live”/“synced X ago” display that implies state freshness without projection proof.
- No full transcript/log/search payload in a Session frame.
- No account-plane ACK finality: account routing may return queued/routed/degraded, but accepted/rejected finality comes only from the owner node.
- No hand edits to generated UniFFI/protobuf output.
- No product-code edits to `ios/LooperCompanion/UI/Pinball`, `ios/LooperCompanion/UI/Maze`, or related game-only support files.
- No final “done” from unit tests alone.

## Verification strategy

> Zero human intervention - all verification is agent-executed.

- Test decision: tests-after for existing behavior seams plus failing-first repros before each behavior change where a seam exists. Use profiler/OSLog real-surface proof for UI flicker and installed-app latency.
- Evidence root: `.omo/evidence/local-first-multinode-architecture-lock/`
- Required live proof commands:
  - Server health: `curl --silent --show-error --max-time 2 -w 'HTTP %{http_code} time=%{time_total} size=%{size_download}\n' http://127.0.0.1:8765/health`
  - Mini recovery: `curl --silent --show-error --max-time 2 -w 'HTTP %{http_code} time=%{time_total} size=%{size_download}\n' http://127.0.0.1:8765/api/mobile/session-minis/snapshot`
  - Desktop snapshot recovery: `curl --silent --show-error --max-time 2 -w 'HTTP %{http_code} time=%{time_total} size=%{size_download}\n' 'http://127.0.0.1:8765/desktop/snapshot?limit=30'`
  - Server sample during recovery: `sample <looper-server-pid> 5 -file .omo/evidence/local-first-multinode-architecture-lock/server-snapshot.sample.txt`
  - iOS diagnostics doctor: `bash scripts/ios-diagnostics.sh doctor`
  - iOS switcher OSLog: `bash scripts/ios-diagnostics.sh oslog --timeout 30s --category AssistantSurface --output-dir .omo/evidence/local-first-multinode-architecture-lock/ios-oslog`
  - iOS ETTrace: `bash scripts/ios-diagnostics.sh ettrace --simulator --launch --verbose`
  - iOS simulator browser proof: start the documented simulator browser flow, open `http://localhost:3200/` in Codex in-app Browser, capture the live simulator screenshot artifact.
  - Physical iPhone proof only at final boundary: build/install once, launch once, then record `devicectl` install/launch JSON and latency logs.
- Focused gates:
  - `cargo test --manifest-path crates/agent-control-plane/Cargo.toml <filter> -- --nocapture`
  - `cargo test --manifest-path crates/looper-client-core/Cargo.toml <filter> -- --nocapture`
  - `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore --filter <filter>`
  - `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter <filter>`
  - `swift test --package-path macos/LooperMenuBar --filter <filter>`
  - `pnpm run check:client-core`
  - strict runtime grep:
    `rg -n "text/event-stream|MobileEventStream|LooperRealtimeStateMiniSynchronizer|/mobile/events|/desktop/events|HTTP.*sendSessionPrompt|HTTP.*setSessionMode|HTTP.*submitNotificationReply|loadSnapshot\\(\\).*sendSessionPrompt|setAssistantSurface" ios macos swift crates/agent-control-plane crates/looper-client-core`

## Execution strategy

### Parallel execution waves

- **Wave 0: Evidence baseline and architecture lock.** Read current dirty state, capture live server latency, state which worktree files are off-limits, and update architecture docs/lints before code cuts.
- **Wave 1: Node finality/control plane.** Collapse command mutation paths into per-node event append + owner-node ACK certificate + async delivery. Move discovery/reconciliation out of request/stream pollers and into producers.
- **Wave 2: Projection and snapshot boundaries.** Make `SessionMini` projection complete, monotonic, small, precomputed, and keyed by `account_id/node_id/session_id`. Keep snapshots recovery-only, range-limited, and never allowed to overwrite newer per-node stream state.
- **Wave 3: Data plane.** Add chunk/cursor/SHA-256/Merkle APIs for transcript/log/search/detail content and keep Session streams capped to compact control frames.
- **Wave 4: Rust client-core ownership.** Make one account runtime own per-node runtimes: endpoint race, reconnect/backoff, outbox routing, optimistic mutations, ACK reconciliation, state-mini reducers, stale snapshot rejection, and local durable stores.
- **Wave 5: iOS/macOS presentation lock.** SwiftUI/AppKit only bind state and start/cancel tasks. Fix tab switcher to be local-only and instant. Fix route/readiness/freshness labels to be truthful.
- **Wave 6: Deletion/lint/proof.** Remove stale paths and enforce dependency/runtime scans. Run final installed proof once.

### Dependency matrix

| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | none | 2, 3, 4, 5, 6, 7 | none |
| 2 | 1 | 3, 4, 8, 10 | 5, 6 |
| 3 | 1, 2 | 4, 8, 10 | 5, 6 |
| 4 | 2, 3 | 8, 10, 12 | 5, 6, 7 |
| 5 | 1 | 6, 9, 10 | 2, 3, 4 |
| 6 | 1, 5 | 9, 10, 11 | 2, 3, 4 |
| 7 | 1 | 12 | 2, 3, 4, 5 |
| 8 | 4 | 10, 12 | 9 |
| 9 | 6 | 10, 12 | 8 |
| 10 | 8, 9 | 11, 12 | none |
| 11 | 10 | 12 | none |
| 12 | 2-11 | final | none |

## Todos

> Implementation + Test = ONE todo. Never separate.

- [x] 1. Baseline live truth and freeze stale-owner taxonomy.
  What to do / Must NOT do: Capture current live server timings, installed app versions, phone local store shape if needed, and current dirty files. Write a short architecture note naming the bug class as multi-owner state drift over federated single-writer RSMs. Do not edit product code or install anything in this todo.
  Parallelization: Wave 0 | Blocked by: none | Blocks: all other todos
  References: `docs/architecture/decisions.md:8-30`, `docs/architecture/decisions.md:327-350`, current dirty state from `git status --short`
  Acceptance criteria (agent-executable): `.omo/evidence/local-first-multinode-architecture-lock/baseline.md` records `/health`, `/desktop/mobile-state`, `/api/mobile/session-minis/snapshot`, `/desktop/snapshot?limit=30`, server CPU sample, dirty worktree exclusions, and the stale-owner taxonomy.
  QA scenarios (name exact tool + invocation): happy: run the four `curl --max-time 2` commands listed in Verification strategy and save stdout to `.omo/evidence/local-first-multinode-architecture-lock/baseline-http.txt`; failure: if any timeout occurs, run `sample <looper-server-pid> 5 -file .omo/evidence/local-first-multinode-architecture-lock/baseline-server.sample.txt` and record the stack owner.
  Commit: Y | `docs(architecture): record local-first state-machine lock`

- [x] 2. Convert node command handling into finality-certificate helpers.
  What to do / Must NOT do: Route every hot command through one owner-node helper that validates FSM, appends a per-node event, records a `CommandAck`, and schedules delivery asynchronously. ACK must be the compact finality certificate; delivery is a later event. Do not call desktop/mobile snapshots or transcript discovery on the command ACK path. Account-plane routing may queue or forward, but may not emit accepted/rejected finality.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 3, 4, 8, 10
  References: `docs/architecture/decisions.md:262-287`, `crates/agent-control-plane/src/grpc/service.rs:557-582`, `crates/looper-client-core/src/model.rs:129-166`
  Acceptance criteria (agent-executable): focused Rust tests prove mode, prompt, notification reply, Siri current/default, archive/delete/mute/settings commands all return accepted/rejected ACKs with `account_id`, `node_id`, `client_mutation_id`, `ack_seq`, `revision`, `server_time`, and do not call `desktop_mobile_snapshot()`.
  QA scenarios: happy: `cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane mobile_events:: -- --nocapture | tee .omo/evidence/local-first-multinode-architecture-lock/task-2-mobile-events.txt`; failure: add/keep a test that injects an illegal FSM command and expects a rejected ACK with stable code/reason/current state.
  Commit: Y | `refactor(server): make command acks finality certificates`

- [x] 3. Move discovery/reconciliation into producers, not request or stream pollers.
  What to do / Must NOT do: Ensure Codex transcript/session discovery, rollout path repair, ACP observation, hook events, and prompt delivery cache rebuilds run in bounded producer paths that append events/projection records. Session stream pollers and HTTP snapshot handlers may only drain already-produced records or return bounded recovery data.
  Parallelization: Wave 1 | Blocked by: 1, 2 | Blocks: 4, 8, 10
  References: `docs/architecture/decisions.md:111-130`, `docs/architecture/decisions.md:216-258`, `crates/agent-control-plane/src/codex.rs:689-718`, `crates/agent-control-plane/src/control_plane.rs:1451-1485`, `crates/agent-control-plane/src/http/mobile_state.rs:138-167`
  Acceptance criteria (agent-executable): server sample taken during `/desktop/snapshot?limit=30` and `/api/mobile/session-minis/snapshot` contains no dominant `refresh_thread_rollout_paths`, JSONL scan, transcript preview scan, or session discovery stack under the request handler.
  QA scenarios: happy: `sample <pid> 5 -file .omo/evidence/local-first-multinode-architecture-lock/task-3-snapshot.sample.txt` while curling snapshot; PASS if sample does not show request-time rollout/transcript scan; failure: unit/integration test forces missing projection and expects bounded `RecoveryRequired` or cached projection, not a sync rebuild on the hot path.
  Commit: Y | `perf(server): keep discovery off recovery requests`

- [x] 4. Lock `SessionMini` projection as the only home/card truth.
  What to do / Must NOT do: Make `SessionMini` complete enough for home/menu cards: account id, node id, session id, mode, replyability, blocked goal, queue count, lifecycle, notification state, assistant surface, freshness source, route endpoint, and revision. Replacement deltas must be monotonic per node and range/chunked if too large. Do not let HTTP full snapshots overwrite newer `seq`.
  Parallelization: Wave 2 | Blocked by: 2, 3 | Blocks: 8, 10, 12
  References: `docs/architecture/decisions.md:113-123`, `docs/architecture/decisions.md:245-255`, `crates/agent-control-plane/src/grpc/service.rs:663-792`, `crates/looper-client-core/src/model.rs:218-263`, `crates/looper-client-core/src/client.rs:3205-3238`
  Acceptance criteria (agent-executable): client-core rejects stale recovered snapshots, applies replacement deltas with `replace=true`, preserves newer local minis, exposes freshness/liveness separately, and stores/apply cursors as `last_seq_by_node`.
  QA scenarios: happy: `cargo test --manifest-path crates/looper-client-core/Cargo.toml state_mini -- --nocapture | tee .omo/evidence/local-first-multinode-architecture-lock/task-4-client-core-state-mini.txt`; failure: focused test feeds older HTTP snapshot after newer stream delta and asserts no UI-visible rewind.
  Commit: Y | `fix(sync): make minis monotonic projection truth`

- [x] 5. Add bounded data-plane content slices.
  What to do / Must NOT do: Introduce transcript/log/detail/search content APIs that fetch visible ranges only: latest tail, page before/after cursor, search context, and attachment/blob by id. Each chunk has `account_id`, `node_id`, `session_id`, `revision`, `offset`, `length`, `sha256`, `next_cursor`, and optional Merkle proof against the content revision root. Do not move full content through `Session` or home/card state.
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 6, 9, 10
  References: `docs/architecture/decisions.md:216-258`, `crates/agent-control-plane/src/transcript_preview.rs`, `ios/LooperCompanion/Services/CompanionSessionDetailCoordinator.swift:1-8`, `docs/architecture/decisions.md:231-233`
  Acceptance criteria (agent-executable): detail view can render cached chunk first, then fetch missing range; content chunk tests verify SHA-256, offset, length, cursor, optional Merkle proof/root, and stale revision rejection.
  QA scenarios: happy: `curl --silent --show-error --max-time 2 'http://127.0.0.1:8765/<new-content-route>?session_id=<fixture>&range=tail&limit=65536' -o .omo/evidence/local-first-multinode-architecture-lock/task-5-tail.json` and assert chunk metadata; failure: request stale revision and assert explicit conflict/retry response.
  Commit: Y | `feat(content): add bounded session content chunks`

- [x] 6. Move detail/search/AppIntents to local-first content and mini resolvers.
  What to do / Must NOT do: iOS detail, Spotlight, and AppIntents resolve entities from Rust client-core local state first, then fetch content slices only for visible detail or search context. HTTP snapshot/detail fallback must not become authoritative or hide degraded recovery.
  Parallelization: Wave 3/5 | Blocked by: 1, 5 | Blocks: 9, 10, 11
  References: `docs/architecture/decisions.md:198-209`, `ios/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift:54-119`, `ios/LooperCompanion/Services/CompanionSessionDetailCoordinator.swift:1-8`, `ios/LooperCompanion/Services/CompanionSessionMiniController.swift:99-143`
  Acceptance criteria (agent-executable): AppIntents entity/default/current/detail paths still work when HTTP snapshot is unavailable but local minis/chunks exist; stale HTTP content cannot replace newer local state.
  QA scenarios: happy: `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter Siri --parallel | tee .omo/evidence/local-first-multinode-architecture-lock/task-6-siri.txt`; failure: focused test with unavailable HTTP service and populated local store must still resolve session entity and explicitly mark degraded content gaps.
  Commit: Y | `fix(ios): make intents and detail local first`

- [x] 7. Enforce Session frame caps and ACK/hash metadata.
  What to do / Must NOT do: Keep every Session frame below a hard cap. Add optional `state_hash` or projection checksum only if it helps detect drift without expanding frame size materially. Do not hash full content, compute Merkle trees, or block ACK on content hashing. Merkle belongs to the data plane.
  Parallelization: Wave 1/2 | Blocked by: 1 | Blocks: 12
  References: `docs/architecture/decisions.md:245-249`, `crates/agent-control-plane/src/grpc/service.rs:785-792`, `crates/looper-client-core/src/model.rs:129-166`
  Acceptance criteria (agent-executable): oversized command/text/state payloads reject with typed error; replacement deltas chunk within cap; ACK remains small and does not require content hashes.
  QA scenarios: happy: `cargo test --manifest-path crates/agent-control-plane/Cargo.toml frame_payload -- --nocapture | tee .omo/evidence/local-first-multinode-architecture-lock/task-7-frame-caps.txt`; failure: test emits over-cap projection and asserts `resource_exhausted` plus recovery instruction, not truncation.
  Commit: Y | `fix(realtime): cap control frames explicitly`

- [x] 8. Make Rust client-core the sole account runtime/store owner.
  What to do / Must NOT do: Rust client-core owns the account runtime and all per-node runtimes: endpoint happy-eyeballs, last-good persistence, reconnect/backoff, command routing to owner nodes, durable outboxes, ACK reconciliation, reducer mirrors, stale recovery rejection, and local snapshots. Swift wrappers may only call `start`, `stop`, `observe`, `setMode`, `sendPrompt`, `submitNotificationReply`, route preference, Siri/default/session settings, and content fetch methods.
  Parallelization: Wave 4 | Blocked by: 4 | Blocks: 10, 12
  References: `docs/architecture/decisions.md:173-212`, `crates/looper-client-core/src/session_runtime.rs:38-159`, `crates/looper-client-core/src/client.rs:913-962`, `crates/looper-client-core/src/session_transport.rs:74-120`, `crates/looper-client-core/src/session_transport.rs:272-340`
  Acceptance criteria (agent-executable): no iOS/macOS production code mutates session truth without Rust client-core; endpoint switch adopts proven live endpoint before UI says connected; pending commands route to the owner node and drain or report durable degraded state; one node offline does not hide or mark other nodes' sessions offline.
  QA scenarios: happy: `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore --filter RealtimeBridge | tee .omo/evidence/local-first-multinode-architecture-lock/task-8-bridge.txt`; failure: test stale last-good endpoint with reachable fallback and assert first-ready endpoint wins and persists.
  Commit: Y | `refactor(client-core): own session runtime truth`

- [x] 9. Make iOS/macOS presentation honest and flicker-free.
  What to do / Must NOT do: Assistant tab switch is local-only and cannot enqueue server commands. SwiftUI/AppKit must not clear local minis on route changes, screen changes, or HTTP failures. UI shows cached/local/degraded/fresh states separately and never displays old-first then latest. Do not redesign visual style or touch game surfaces.
  Parallelization: Wave 5 | Blocked by: 6 | Blocks: 10, 11, 12
  References: `docs/architecture/decisions.md:294-323`, `ios/LooperCompanion/App/CompanionAppModel.swift:450-492`, `ios/LooperCompanion/App/CompanionAppModel.swift:494-513`, `ios/LooperCompanion/Services/CompanionSessionMiniController.swift:36-72`, `macos/LooperMenuBar/Sources/LooperMenuBarCore/MenuRefreshCoordinator.swift:98-180`
  Acceptance criteria (agent-executable): 100 rapid assistant tab switches produce zero stream restarts, zero HTTP command/state calls, no list collapse, no stale label lies, and no ETTrace main-thread hitch above the accepted threshold.
  QA scenarios: happy: `bash scripts/ios-diagnostics.sh oslog --timeout 30s --category AssistantSurface --output-dir .omo/evidence/local-first-multinode-architecture-lock/task-9-oslog` plus ETTrace capture; PASS if logs show local selection events only and no `setAssistantSurface`/stream restart; failure: automated switch loop toggles Codex/Zed/Claude/Grok 100x and asserts no minis disappear and no route badge claims connected before live endpoint proof.
  Commit: Y | `fix(ios): make assistant switching local-first`

- [x] 10. Delete or demote old HTTP/snapshot truth paths.
  What to do / Must NOT do: Keep HTTP only for `/health`, pairing/auth handoff, full snapshot on first install/corruption/seq gap/manual diagnostics, and bounded data-plane content if chosen. Delete or demote any remaining mutable visible-session actions from HTTP truth. Full snapshot recovery must be small, range-aware, and freshness-gated.
  Parallelization: Wave 6 | Blocked by: 8, 9 | Blocks: 11, 12
  References: `docs/architecture/decisions.md:48-52`, `docs/architecture/decisions.md:327-337`, `crates/agent-control-plane/src/http/mod.rs:728-765`, `crates/agent-control-plane/src/http/mobile_state.rs:56-77`, `crates/agent-control-plane/src/http/mobile_state.rs:138-167`
  Acceptance criteria (agent-executable): strict runtime grep is clean for old transports and command paths; HTTP snapshot success cannot overwrite newer stream seq; HTTP failure cannot make UI say disconnected when local minis exist.
  QA scenarios: happy: run strict runtime grep from Verification strategy and save output to `.omo/evidence/local-first-multinode-architecture-lock/task-10-strict-grep.txt`; failure: focused test applies snapshot with lower `latest_seq` after stream delta and asserts reject/degraded marker.
  Commit: Y | `refactor(transport): demote http to recovery only`

- [ ] 11. Run profiler-led installed/simulator UX proof.
  What to do / Must NOT do: Use OSLog, ETTrace, perf-loop, Codex in-app Browser simulator proof, and only then one physical iPhone install. Do not repeatedly rebuild/install while coding. Always wait for profiler export or record why it failed.
  Parallelization: Wave 6 | Blocked by: 9, 10 | Blocks: 12
  References: `.agents/skills/ios-perf-diagnostics/SKILL.md:21-69`, `scripts/ios-diagnostics.sh:21-48`, `scripts/ios-diagnostics.sh:83-153`
  Acceptance criteria (agent-executable): evidence directory includes OSLog, ETTrace/perf artifact, simulator browser screenshot, macOS install/launch proof, iPhone install/launch proof, p95 mode/prompt/switch timings, and server snapshot/health timings.
  QA scenarios: happy: execute diagnostics commands from Verification strategy and install once using existing repo iOS/macOS install scripts; failure: if ETTrace or device logs fail, capture exact command/error and fall back to OSLog + perf-loop only after recording the blocker.
  Commit: N | proof-only unless scripts/docs change.

- [ ] 12. Add architecture/dependency guardrails.
  What to do / Must NOT do: Add or tighten lints so regressions fail fast: no generated-file hand edits, no old transport strings, no Swift reducers owning Session truth, no Session frame over cap, no content blob in control frames, no product code touching game surfaces for realtime cuts.
  Parallelization: Wave 6 | Blocked by: 2-11 | Blocks: final
  References: `docs/architecture/decisions.md:381-391`, `package.json` check scripts, `scripts/check-client-core-boundaries.sh`, strict grep command in Verification strategy
  Acceptance criteria (agent-executable): `pnpm run check:client-core` and new/updated architecture guard pass; intentionally planted fixture violation fails in the guard test or documented dry-run.
  QA scenarios: happy: `pnpm run check:client-core | tee .omo/evidence/local-first-multinode-architecture-lock/task-12-client-core-check.txt`; failure: run guard in strict mode against a known forbidden sample and save failing output.
  Commit: Y | `ci(architecture): guard realtime ownership`

## Final verification wave

> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.

- [ ] F1. Plan compliance audit
  - Check every todo acceptance criterion has evidence under `.omo/evidence/local-first-multinode-architecture-lock/`.
  - Check no task touched Pinball/Maze/game-only files.
  - Check no implementation step kept a compatibility path forbidden in Must NOT have.

- [ ] F2. Code quality review
  - Run focused changed-surface tests and final `pnpm run check:client-core`.
  - Run `git diff --check`.
  - Verify generated Swift was regenerated, not hand-edited.

- [ ] F3. Real manual QA
  - Installed macOS app: `/Applications/looper.app` launches and live server answers `/health`, `/api/mobile/session-minis/snapshot`, and bounded `/desktop/snapshot?limit=30` quickly.
  - iPhone: installed app renders cached account sessions immediately, then per-node live endpoint proof updates route without clearing sessions from other nodes.
  - Assistant switch loop: 100 switches in 2-3 seconds equivalent produce no stream restart and no old-first/latest jump.
  - Mode/prompt/reply: UI unblocks on accepted ACK, delivery appears later.

- [ ] F4. Scope fidelity
  - Confirm HTTP remains only health/pairing/bootstrap/recovery/manual diagnostics/content data plane.
  - Confirm Session stream carries only compact control frames.
  - Confirm data plane chunks are range/cursor/SHA-256 based and can be verified against a content-revision Merkle root when provided.
  - Confirm stale/fresh/liveness labels are semantically distinct.

## Commit strategy

- Commit each acceptance boundary, not each extraction.
- Use Conventional Commits:
  - `docs(architecture): record local-first state-machine lock`
  - `refactor(server): make command acks finality certificates`
  - `perf(server): keep discovery off recovery requests`
  - `fix(sync): make minis monotonic projection truth`
  - `feat(content): add bounded session content chunks`
  - `fix(ios): make intents and detail local first`
  - `fix(realtime): cap control frames explicitly`
  - `refactor(client-core): own session runtime truth`
  - `fix(ios): make assistant switching local-first`
  - `refactor(transport): demote http to recovery only`
  - `ci(architecture): guard realtime ownership`
- Final commit footer: `Plan: .omo/plans/local-first-multinode-architecture-lock.md`

## Success criteria

- Every user action that mutates visible session state renders from local state first and reconciles through Rust client-core by `client_mutation_id`.
- Hot command ACK is one warm Session-stream RTT and contains finality metadata.
- No stream restart or HTTP hot-path call occurs on assistant tab switch.
- No route/Tailscale/LAN label claims connected before Rust client-core has a proven live endpoint.
- Cached minis never collapse to a partial list while recovery is stalled.
- HTTP snapshot/data-plane results cannot overwrite newer stream `seq`.
- `/desktop/snapshot?limit=30`, `/api/mobile/session-minis/snapshot`, and `/health` are responsive while streams are connected.
- Full transcript/log/search/detail content is fetched by bounded chunk/range/cursor with SHA-256 and optional Merkle proof, never through home/card minis or one giant Session frame.
- Account Plane routes identity, devices, APNs, node registry, and optional relay/queueing; it never emits final accepted/rejected command ACKs.
- Multi-node state uses `last_seq_by_node`; a single global cursor is forbidden unless a future account relay becomes a true ordered aggregate.
- Final proof includes simulator browser, OSLog/ETTrace or perf-loop, macOS installed app, iPhone installed app, strict grep, and p95 latency artifact.

# Realtime refactor — diagnosis and phased plan (2026-07)

Audit of the prompt-send path, quinn/h3 transport, macOS hang, and iOS reliability.
Phase 0 is being executed as five parallel work items; later phases land separately.

## Root causes found

### 1. Prompt send: three layers of "accepted", none mean "delivered"

- `LooperClientCoreSessionRuntime::send_prompt` returns `accepted: true` after a local
  outbox write (`crates/looper-client-core/src/session_runtime.rs`). UI treats it as sent.
- The background flush waits for acks with a 2 s timeout × 5 retries
  (`crates/looper-client-core/src/client.rs`). If the stream is mid-reconnect (app switch,
  network flap), every retry burns; the failure lands only in `state.last_error` and the
  command sits in the outbox with no further retry until an unrelated command flushes.
- The server acks, then delivers on a spawned thread
  (`crates/agent-control-plane/src/mobile/prompt_delivery.rs`). A delivery failure emits a
  `SessionChanged` event with detail `prompt-delivery-failed`, which the session reducer
  maps to no FSM event (`crates/agent-control-plane/src/control_plane/reducer.rs`), so the
  session state stays `Dispatched` forever and every subsequent prompt is rejected
  `session_busy`. One failed delivery wedges the session.

### 2. macOS hang: whole-store rewrites under a shared mutex

The client-core local store is a single JSON blob rewritten in full, under the state
mutex, on every delta and every streamed text chunk
(`crates/looper-client-core/src/local_store.rs`). The production file
(`~/Library/Application Support/looper/looper-realtime-state-minis.json`) measured
12.7 MB; the menu bar app reads snapshots synchronously on the main thread and blocks
behind those writes. The server sqlite measured 134 MB with no event retention.

### 3. Transport: h3 used as a single pipe, multiplexing hand-rolled on top

State deltas, text chunks, heartbeats, commands, and acks all ride one gRPC bidi stream.
The client re-implements request/response correlation (ack receiver lease, ack backlog,
bounded channels that can stall the drive loop). HTTP/3 already provides cheap
independent streams; commands belong on unary RPCs. quinn runs with default config: no
QUIC keep-alive, no tuned idle timeout, no 0-RTT resume, no path migration — LAN↔Tailscale
switches are handled by full teardown via NWPathMonitor instead, which is the visible
connection "flip" on iOS.

### 4. No end-to-end coverage

No test wires the real client-core against the real server. All the above failure modes
live in exactly those seams.

## Phase 0 (in flight, five parallel items)

1. **fsm-wedge** — `PromptDeliveryFailed` FSM event; delivery failure returns the session
   to `ModeArmed` instead of wedging in `Dispatched`.
2. **sqlite-retention** — event store retention (per-entity cap + age cutoff), batched
   prune at startup and on the scheduler, space reclaim.
3. **store-split** — local store: synchronous durability only for the command outbox;
   debounced coalesced persistence for state/text chunks, serialization and IO outside
   the mutex, hot file split from bulky session detail, size cap.
4. **prompt-status** — auto-reflush of the pending outbox when the stream returns, slow
   periodic backstop, and iOS UI that renders pending/undelivered prompts honestly.
5. **e2e-harness** — real server + real client-core over h3/h2 loopback: happy-path
   prompt, mode round-trip, resume after listener kill, h3→h2 fallback, plus `#[ignore]`d
   tests encoding the target behaviors of items 1 and 4.

## Phase 1 — transport

- Commands become unary gRPC calls on a shared h3 connection (response = ack); deletes the
  ack lease/backlog/timeout machinery.
- Session stream becomes a server-stream carrying deltas/chunks/heartbeats only.
- One persistent quinn endpoint: QUIC keep-alive, tuned idle timeout, 0-RTT for instant
  foreground reconnect, path migration for LAN↔Tailscale instead of teardown. h2 remains
  a fallback tier.
- Server moves command handling off the stream loop (no sync SQLite inside async).

## Phase 2 — clients

- Client-core owns the connection state machine and exposes data freshness; UI connection
  pill decouples from transport churn (grace period before showing offline).
- Split `CompanionAppModel` into connection controller / session store / prompt outbox /
  presenters; replace JSON-string FFI round-trips with typed calls.
- macOS subscribes to the same client-core stream as iOS; handoff publishes from store
  changes, not menu-refresh outcomes.

## Phase 3 — e2e as the gate

Extend the harness: steer-while-running, queue-while-stopped, delivery-failure recovery,
network flap, backgrounding. Transport and UI changes gate on it.

## Accelerated execution plan for Phases 1–3 (one-day target)

Phase 0 landed as five parallel codex jobs (gpt-5.5 xhigh, fast_mode, multi_agent), each
in its own worktree, reviewed and gated by the orchestrator. Phases 1–3 run the same way
with these changes, because build/test wall-clock dominates:

### Build/test speed config (do FIRST, ~15 min)

- Dev profile for `agent-control-plane` and `looper-client-core`:
  `split-debuginfo = "unpacked"` and `debug = "line-tables-only"` (link/dSYM cost drops
  hard on the large crates).
- Use `cargo nextest` for suites (per-test parallelism plus automatic flake retries; the
  isolated_control_plane suite produced 13 load-induced false failures under parallel
  codex load without it).
- Keep the shared `CARGO_TARGET_DIR=/Users/ay/.cache/looper-cargo-target`; pre-warm each
  job worktree with `cargo build --tests` at creation, before codex starts.

### Verification tiers

- Per job: codex runs filtered gates; orchestrator does `cargo check` + line-by-line diff
  review only.
- ONE combined gate after all merges: nextest on both crates + e2e suite + one simulator
  test run + one swift package test. No per-branch full suites.
- Known pre-existing failures on main (13 tests: grok/devin/claude-code snapshot suites,
  handoff page, zed routes) are NOT regressions; compare failure sets, don't chase.

### Serial monsters, batched

- UniFFI regen + xcframework rebuild exactly once, after every FFI-surface change merged.
- iOS simulator boots once at the combined gate.

### Schedule

1. Hour 0–1 — build-speed config, then the Phase 1 CONTRACT job alone (proto: unary
   command RPCs, session server-stream). Serial on purpose: everything hangs off it; if
   the contract smells wrong, stop and fix before fanning out.
2. Hour 1–5 — four parallel codex jobs against the new contract:
   a. server unary handlers + command executor off the stream loop,
   b. client-core transport swap (delete ack lease/backlog/flush-timeout machinery),
   c. quinn tuning (QUIC keep-alive, idle timeout, 0-RTT resume, path migration;
      also fixes the e2e-documented h3 listener-restart recovery gap),
   d. Phase 2 wins-only: connection pill grace period + handoff publishing from store
      changes (defer the CompanionAppModel split).
3. Hour 5–7 — merge in dependency order (`cargo check` per merge), extend e2e harness and
   un-ignore the two target-behavior tests (auto-reflush after reconnect, no wedge after
   delivery failure).
4. Hour 7–8 — single FFI regen, combined full gate, macOS package + iPhone build once.

Deferred by this plan: CompanionAppModel split (Phase 2 refactor luxury), per-branch
bisectability. Risk to watch: a contract mistake discovered mid-fan-out cascades; the
hour-1 review of the contract job is the checkpoint that protects the day.

# Looper Architecture Decisions

**Status:** Adopted target architecture. Implement in the order in §9.
**Audience:** Any agent or human touching transport, session state, streaming, or the
mobile/desktop clients. Read this fully before changing those surfaces. This document is
self-contained — do not rely on chat history or prior context.

> **One-line summary:** One duplex stream, one event log, one reducer. The Mac is the only
> writer of truth; clients apply optimistic guesses and reconcile against the Mac. The phone
> unblocks on *accepted*, never on *agent-done*. Wrong actions are rejected politely, never crash.

---

## 0. Why this exists (the problem)

Looper today carries **two live transports** for the same control plane (HTTP routes and gRPC)
and still carries stale documentation about an SSE path that is no longer present in source.
It also carries **three independent client reducers** (`ios/LooperCompanion`,
`macos/LooperMenuBar`, and the Rust TUI). Clients race multiple URLs with serial timeouts and
fall back between transports. The result:

- Mode switch / prompt send take 15–20s on the phone (measured baseline, 2026-06-23).
- "Edge cases" are really **unnamed illegal state transitions** and **reducer drift** between
  the three client copies.
- Every change must be reconciled across hot gRPC, cold HTTP, and three clients unless the hot
  command/state path is reduced to one Session stream.

The gRPC contract in `crates/agent-control-plane/proto/looper/v1/control_plane.proto` already
has the right primitives (`client_mutation_id`, `ack_seq`, `revision`, `idempotent_replay`,
seq-based `StateMiniDelta`). We are committing to that model fully and deleting the redundancy.

**Latency physics (state honestly, do not over-promise):** an action costs
`network RTT + server handling + app overhead`. We only own the last two; drive them to ~0.

| Path | RTT reality | Our overhead target |
| --- | --- | --- |
| LAN | 0.3–2 ms | < 1 ms (meets a 10 ms action budget) |
| Tailscale P2P (WireGuard direct) | 5–40 ms | 0 ms added |
| Tailscale DERP relay / public proxy | 30–150 ms | 0 ms added |

"Zero on LAN and Tailscale" means **zero added overhead**, plus *perceived* zero via optimistic
UI. On the public internet, 10 ms is physically impossible; the floor is the wire.

---

## ADR-001 — Transport: one bidirectional gRPC stream

**Decision.** The single hot control channel between any client and the control plane is **one
long-lived bidirectional gRPC stream**. HTTP survives only for (a) pairing/bootstrap,
(b) health probes, and (c) full snapshots for bootstrap/recovery. No source-level SSE transport
exists; do not add one. Unary command RPCs are removed in the same rewrite cut; there is no
backward-compatibility mode for old command transports.

**The stream.** Add to the proto:

```proto
rpc Session(stream ClientFrame) returns (stream ServerFrame);

message ClientFrame {
  oneof f {
    Command command;   // SetSessionMode / SendSessionPrompt / SubmitNotificationReply
    Resume  resume;    // { int64 after_seq }  — replay everything after this seq
  }
}

message ServerFrame {
  oneof f {
    CommandAck     ack;          // durable, sub-ms: "accepted into log"
    StateMiniDelta state_delta;  // durable: FSM/session change (in event log)
    TextChunk      text_chunk;   // ephemeral: coalesced assistant output, replayable by seq
    MobileEvent    event;        // durable: prompt-delivered, check-failed, etc.
    Heartbeat      heartbeat;    // keepalive
  }
}
```

Reuse the existing `CommandAck`, `StateMiniDelta`, `MobileEvent`, and `Command`-shaped messages;
do not invent parallel ones.

**Why.** A command becomes a frame on an already-open stream → **1 RTT, no per-call
TCP/TLS/HTTP2 handshake**. Commands, acks, state, events, and text are sequenced on **one pipe**,
so the snapshot-vs-hot-stream reconcile race cannot exist. Reconnect is `Resume{after_seq}`.

**Network layer.** Tailscale is the transport. WireGuard gives one flat, authenticated,
encrypted L3 network identical on LAN, P2P-remote, and DERP-relay; the same HTTP/2 channel works
everywhere. **Do not build NAT traversal or a bespoke relay.** For non-Tailscale internet,
terminate at one proxy speaking gRPC or the Connect protocol (Connect is friendlier through dumb
proxies) and treat it as the slow path.

**Connect path.** Use **happy-eyeballs, not serial racing**: open the stream to the cached
last-good endpoint immediately while discovering others concurrently; first to reach `READY`
wins, cancel the rest. No serial per-URL timeouts.

**Do**
- Keep one stream per client, kept warm with gRPC keepalive while foregrounded.
- Persist the last-good endpoint (Keychain/UserDefaults) and connect to it first.
- Delete obsolete transports instead of wrapping them. Rewrite clients to `Session`; do not keep
  compatibility shims for previous HTTP command paths.

**Don't**
- Add a new socket/channel for any feature, including streaming text. Everything rides `Session`.
- Add SSE or any second hot stream. Do not add HTTP routes for session commands or state.

**Where.** Proto: `crates/agent-control-plane/proto/looper/v1/control_plane.proto`.
Server: `crates/agent-control-plane/src/grpc/service.rs`.
Client: rewrite client surfaces to the Rust client core (ADR-004); the former
`swift/LooperRealtime` bridge is retired once clients compile against `swift/LooperClientCore`.

---

## ADR-002 — State: one event log, one reducer, single writer

**Decision.** The control plane is **event-sourced**. There is one append-only log with a global
monotonic `seq`. **State = `fold(events)`.** The server (Mac) is the **only writer**. Clients
never author truth; they apply optimistic guesses and reconcile.

**The reducer is written once.** `fn fold(state, event) -> state` lives in **one Rust module** and
is the *same code* the server uses to build state and the client core (ADR-004) uses to mirror it.
Reducer drift becomes impossible by construction.

**Do**
- Mutate state only by appending an event with a new `seq`. Nothing else writes state.
- Reuse the existing `StateMiniDelta.seq` / `latest_seq` as the canonical sequence.

**Don't**
- Compute state ad hoc in route handlers, clients, or the TUI.
- Let any client write to the store or to agent config directly.

**Where.** New module e.g. `crates/agent-control-plane/src/control_plane/reducer.rs` and
`.../events.rs`. Extract logic currently implicit in `src/control_plane.rs` (2488 lines).

---

## ADR-003 — Session lifecycle is an explicit finite state machine

**Decision.** Session lifecycle is a single Rust `enum` FSM with **explicit legal transitions in
one module**. Any command illegal for the current state returns a **rejected `CommandAck`** —
never a panic, never a silent no-op, never an "edge case."

```
IDLE                ──arm mode──▶ MODE_ARMED
MODE_ARMED          ──send prompt──▶ PROMPT_PENDING(cmid)
PROMPT_PENDING      ──mac accepts──▶ DISPATCHED        (ack to client NOW, sub-ms)
DISPATCHED          ──agent picks up──▶ AGENT_RUNNING  (async, later)
AGENT_RUNNING       ──agent stops──▶ STOP_REQUESTED
STOP_REQUESTED      ──mode=infinite──▶ CONTINUATION_PENDING ──new prompt──▶ DISPATCHED
                    ──mode=checks────▶ CHECKS_RUNNING
                    ──mode=await─────▶ WAIT_REPLY
                    ──mode=off───────▶ DONE
CHECKS_RUNNING      ──pass──▶ DONE   |  ──fail──▶ CONTINUATION_PENDING
WAIT_REPLY          ──human reply──▶ DISPATCHED
```

The transition function is the single authority:
`fn next(state: SessionState, cmd: Command) -> Result<SessionState, Reject>`.

**Why.** Most current "edge cases" are unnamed illegal transitions. Naming them converts crashes
and undefined behavior into deterministic, testable rejections.

**Do**
- Put every transition in one file. Add a transition only by editing the FSM, with a test.
- Return a typed `Reject` (reason + current state) that the client can show.

**Don't**
- Branch on session status in route handlers or clients. They consume FSM output; they don't
  re-implement it.

**Where.** `crates/looper-session-core/src/lib.rs`, re-exported through
`crates/agent-control-plane/src/control_plane/session_fsm.rs` for server callers.

---

## ADR-004 — One client core in Rust, shared by all three surfaces

**Decision.** SwiftUI owns the app/scene lifecycle: object creation, environment injection,
foreground activation, suspension, and task cancellation stay in the SwiftUI `App`/root scene.
Below that lifecycle boundary, the client core (connect, the duplex stream, the reducer mirror,
optimistic-mutation tracking, reconnect/backoff, and the FSM mirror) is written **once in Rust**
and shipped to Swift via **UniFFI**, reusing the existing Rust-staticlib → XCFramework pipeline
already used by `orb-code` (`scripts/build-orb-code-ios-package.sh`).

```
SwiftUI App/scene lifecycle
   owns: create client manager · active/suspended transitions · dependency injection
Rust client core (tonic + reducer mirror + optimistic tracker + FSM mirror + reconnect)
   exposes: start(endpoints) · stop() · setMode(thread, preset) · sendPrompt(...) · observe() -> state stream
   ┌──────────────┬──────────────┐
  TUI (native)   macOS (UniFFI)  iOS (UniFFI)   ← VIEW ONLY
```

The reducer compiled into the client core is the **same code** as the server reducer (ADR-002).

**Why.** Today `CompanionAppModel.swift` (3119 lines), macOS `ControlPlaneClient.swift`
(2509 lines), and `CompanionModels.swift` (2494 lines) each re-implement connect + reconcile +
reconnect + mutation tracking — three reducers, three drift sources. One Rust core deletes
~6000 lines of drift-prone client logic and tunes the latency path once.

**Do**
- Shrink Swift to an `@Observable` wrapper over the core's emitted immutable state snapshots.
- Drive the wrapper from SwiftUI lifecycle hooks (`App`, `scenePhase`, `.task`, cancellation);
  do not start streams from leaf views or duplicate lifecycle handling across views/services.
- Rewrite iOS first onto the Rust core, then macOS, then remove the parallel Swift gRPC client.
- Prefer compile-time removal of old routes and reducers over runtime flags. A flag is allowed only
  for internal development while the same PR removes the old path before merge.

**Don't**
- Put route logic, SQL, reconcile/merge logic, reconnect/outbox loops, or session-control state in
  Swift or the TUI.
- Hand-edit generated UniFFI bindings or generated protobuf/gRPC output.

**Where.** New crate e.g. `crates/looper-client-core`. Consumers: `ios/LooperCompanion`,
`macos/LooperMenuBar`, `crates/agent-control-plane/src/tui`.

---

## ADR-005 — Streaming assistant output rides the same pipe

**Decision.** Assistant text streams as `TextChunk` frames on the **same `Session` stream**. No
second socket, no SSE.

Rules:
1. **Server fan-out.** The Mac reads agent output **once** and broadcasts to all open streams
   (phone, macOS UI, TUI). No per-client tailing of the same source.
2. **Append-only, seq-numbered.** Each chunk carries a `seq`. Late join / reconnect →
   `Resume{after_seq}` replays missed chunks; clients never lose the middle of a message.
3. **Coalesce.** Batch tokens into one frame every **~50–100 ms**. Do not emit a frame per token
   (battery, UI thrash). Humans cannot perceive faster.
4. **Backpressure honest.** On a slow client, rely on HTTP/2 flow control; collapse to "latest
   state of the message so far" rather than buffering unboundedly on the Mac.
5. **Text is not state.** `TextChunk` is ephemeral and replay-recoverable; `StateMiniDelta` /
   `MobileEvent` are durable and live in the event log. Same pipe, different durability rules.

**Where.** `crates/agent-control-plane/src/grpc/service.rs`, `src/grpc/events.rs`,
`src/transcript_preview.rs`.

---

## ADR-006 — ACK-first: two clocks, never block on the slow one

**Decision.** Distinguish two events for every command:

- **ACCEPTED** — written to the log. Sub-ms. **The client UI unblocks here.**
- **DELIVERED** — the agent actually received it (e.g. `ResumeCodex` process spawn, ACP
  handshake). 100s of ms to seconds. Surfaced later as a `MobileEvent` ("delivered ✓").

The UI must **never wait on DELIVERED.** The entire "15s send" feeling is waiting on the wrong
clock.

**Hot-path fix (do this regardless of the rest).** `send_session_prompt` in
`crates/agent-control-plane/src/mobile/prompt_delivery.rs` currently calls
`desktop_mobile_snapshot()` **and** `mobile_session_service().state()` (two store reads) **per
prompt** before choosing the delivery action. Move that off the hot path:

- Precompute the `PromptDeliveryAction` (`src/mobile/api.rs`) per session, cache it, invalidate on
  session-change events.
- On a command: validate the FSM transition → append event → return `accepted` ack. Then dispatch
  delivery asynchronously and emit `PromptDelivered` later.

Dispatch on the hot path becomes a map lookup + log append + ack = sub-ms.

**Do**
- Reconcile optimistic mutations by `client_mutation_id`; roll back on reject; use
  `idempotent_replay` for safe retries.

**Don't**
- Hide pending work without a reliable ack/eventual-consistency path (see Anti-cheating, §10).

---

## ADR-007 — Connect lifecycle: never *look* disconnected

**Decision.** The phone is not "always connected" (iOS suspends background sockets — accept this).
It is **never *visibly* disconnected.**

- **Foreground:** stream held open with gRPC keepalive (`keepAliveTime ~15s`, `permitWithoutCalls`).
  Every action is 1 RTT.
- **Background:** the socket dies — do not fight it with VoIP/PushKit or audio/location background
  modes (App Store rejection risk). The **Tailscale Network Extension keeps the WireGuard tunnel
  alive** while the app is suspended, so the expensive L3 setup is already done on return.
- **Return / cold launch:**
  1. Render from **persisted local state** at 0 ms (no spinner) while the stream catches up.
  2. **Cache last-good endpoint**; connect to it first (happy-eyeballs fallback in parallel).
  3. **Pre-warm** `connect()` in app `init`/root `.task`, before the user navigates.
  4. **TLS 1.3 session resumption** (persist the ticket) → 1-RTT reconnect, not full handshake.
  5. `Resume{after_seq}` replays only missed deltas/chunks.
- **Background events on Mac:** use **silent push** (APNs `content-available`) as a *signal* to
  wake a short window and pre-warm or notify — not as a data channel (APNs has a per-hour budget).

**Latency contract after this work:**

| Moment | Today | Target |
| --- | --- | --- |
| Cold launch → usable UI | seconds (spinner) | **0 ms** (cached state) |
| Cold launch → first action ready | ~20 s | **~1 RTT** (warm tunnel + TLS resume + pre-warm) |
| Foreground from background | seconds | **~1 RTT** (`Resume`) |
| Action while foregrounded | ~15 s | **1 RTT** (single-digit ms on LAN/Tailscale) |

**Where.** iOS `scenePhase` handling in `ios/LooperCompanion/App`; transport config in the Rust
client core (ADR-004).

---

## 8. Invariants (enforce in CI with a dependency lint)

1. **Dependencies point inward:** clients → client core → control-plane core → agent adapters.
   Clients never touch the DB or agents; agents never touch clients.
2. **The `Session` duplex stream is the only hot client↔core channel.** HTTP = pairing bootstrap,
   health, and full snapshot recovery only. No SSE.
3. **The reducer exists once** (core) and is mirrored once (client core) — same Rust code.
4. **Every agent integration is one `AgentHost` trait impl + one conformance test.** The dirty
   external-config patching (`~/.codex/hooks.json`, `~/.claude/settings.json`, `~/.grok/...`,
   `~/.config/devin/...`) is quarantined behind that trait — the only place allowed to be ugly.
5. **One writer:** only the control-plane core appends events. State is never mutated elsewhere.

Target coupling graph:

```
        AGENT ADAPTERS  (the only dirty edge)        Codex · Claude · Grok · Devin · Zed
          one `trait AgentHost` + conformance test
                        │  inbound only, via trait
        CONTROL PLANE CORE                            single writer · event log · seq · FSM · reducer · SQLite
                        │  ONE duplex gRPC stream (Command ↑ / Ack+Delta+Event+TextChunk ↓)
        CLIENT CORE (Rust, UniFFI)                    reducer mirror · optimistic tracker · reconnect
          ┌───────────┬───────────┐
        TUI         macOS         iOS                 ← view only
```

---

## 9. Implementation order (do not skip; each step ships independently)

1. **ACK-first + cached delivery action** (ADR-006). Server-only, no client change. Unblocks
   latency immediately and proves the number. **Measure before/after.**
2. **`Session` duplex stream** (ADR-001) becomes the only command/state transport. Rewrite iOS to
   use it directly, delete unary command RPCs in the same cut, then fix macOS/TUI against
   the new contract.
3. **Session FSM module** (ADR-003): extract implicit transitions from `control_plane.rs` into
   the shared FSM exposed through `session_fsm.rs`; route commands through `next()`.
4. **Rust client core via UniFFI** (ADR-004): iOS first, then macOS, then retire the
   parallel Swift gRPC client.
5. **Dependency lint in CI** (§8) to keep the graph honest.

Steps 1–2 alone hit the latency goal. Steps 3–5 remove the edginess permanently.

**Acceptance per step** (no "done" without this):
- Step 1: p95 prompt-accept measured on the installed app, recorded in
  `docs/qa/mobile-realtime-latency.md`, with the exact server commands/output.
- Step 2: iOS performs the full workflow over the duplex stream; unary command RPCs deleted; no
  runtime compatibility flag remains; tests green.
- Step 3: an illegal command returns a typed reject with a test; no status branching remains in
  handlers/clients.
- Step 4: `CompanionAppModel.swift` reduced to a view wrapper; one reducer in the repo.
- Step 5: CI fails on an inward-dependency violation.

---

## 10. Anti-cheating rules (carry over from GOAL latency standards)

- Do **not** count server-only, model-only, or mock-only timings as app completion. Measure the
  installed app against a live server.
- Do **not** remove auth checks, snapshot correctness, or event reliability to win latency.
  Endpoint fallback is allowed for reachability; transport fallback to HTTP command paths is not.
- Do **not** fake speed by hiding pending work without a reliable ack / eventual-consistency path.
- Do **not** accept one lucky run; use p95 from ≥10 clean runs (≥3 if manual-only).
- Do **not** mark complete while the app still does serial multi-second waits on a stale URL.
- Do **not** hand-edit generated protobuf/gRPC files, or silently edit user agent-config files
  without a matching product path and verification.

---

## 11. Glossary (for fast onboarding)

- **Accepted vs Delivered** — accepted = in the log (sub-ms, UI unblocks); delivered = agent got
  it (async, shown later). See ADR-006.
- **`client_mutation_id`** — client-generated id tagging an optimistic mutation so the client can
  match the server's ack/delta back to its local guess and reconcile or roll back.
- **`seq` / `after_seq`** — global monotonic event number; `Resume{after_seq}` replays everything
  since the client's last seen seq.
- **Happy-eyeballs** — open candidate endpoints concurrently, take the first `READY`, cancel the
  rest. Replaces serial URL racing.
- **Optimistic UI** — apply the change locally at 0 ms, then confirm/roll-back on the server's
  authoritative answer.
</content>
</invoke>

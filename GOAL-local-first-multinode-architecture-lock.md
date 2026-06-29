# Local-first multinode architecture lock

Plan: `.omo/plans/local-first-multinode-architecture-lock.md`

## Outcome

Deliver federated single-writer replicated state machines with compact finality certificates, a separated data plane, and local-first account projections across Rust server, Rust client-core, UniFFI, iOS, and macOS. iOS/macOS must not lie about connectivity, stale sessions, route/Tailscale/LAN switching, or freshness. HTTP stays recovery/bootstrap only; Session stream is the only hot control channel.

Root execution model: this Codex thread is the orchestrator for `.omo/plans/local-first-multinode-architecture-lock.md`. Product implementation, QA, and review work is delegated to bounded subagents with disjoint ownership. The root touches only `.omo` state, evidence, plan checkboxes, dispatch, and verdicts.

## Baseline (2026-06-29)

- Branch: `main` at `da7515a3` (after notification-target Session cut + stale connectivity fixes).
- Partial progress outside plan checkboxes: strict transport grep mostly clean; assistant tab switch local-only; Siri reads local snapshot; macOS enrichment refresh on SessionMini path; `pnpm run check:client-core` passes.
- Known gaps: plan todos 1–12 unchecked; `LooperCurrentSessionResolver.swift` missing on main (blocks `check-ios.sh`); server command path still split across handlers/FSM/reducer; data-plane chunks not implemented; final installed proof not run.
- iOS proof simulator: `Looper Siri iOS27b2` (`F12A26E9-9FBC-4EA1-B72B-8EF098D5B93C`), `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`.
- Evidence root: `.omo/evidence/local-first-multinode-architecture-lock/`
- Prior audits: `.omo/teams/019ef45f-d013-72a1-814e-614b65ed5e99/artifacts/`

## Constraints (must NOT)

- No SSE/unary gRPC command revival; no HTTP mode/prompt/reply/session-state hot path.
- No full transcript/log/search through Session frames or home/card minis.
- No hand edits to generated UniFFI/protobuf output.
- No Pinball/Maze/game-only file edits.
- No completion from unit tests alone; require installed/simulator/browser/profiler proof at final boundary.
- No compatibility shims that preserve forbidden transport paths.

## Primary verifier

All 12 plan todos checked with evidence under `.omo/evidence/local-first-multinode-architecture-lock/`, plus final wave F1–F4 approved.

## Supporting checks

```bash
pnpm run check:client-core
cargo test --manifest-path crates/looper-client-core/Cargo.toml
cargo test --manifest-path crates/agent-control-plane/Cargo.toml
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore
swift test --package-path macos/LooperMenuBar
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild test \
  -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion \
  -destination 'platform=iOS Simulator,name=Looper Siri iOS27b2,OS=27.0' \
  -only-testing:LooperCompanionTests
bash scripts/check-ios.sh
git diff --check
```

Strict runtime grep (plan Verification strategy) must be clean; artifact saved to `task-10-strict-grep.txt`.

## Iteration loop

1. Read plan; pick first unchecked todo whose dependencies are met.
2. Failing-first proof at seam (test or manual QA scenario) before production change.
3. Implement bounded slice; commit at acceptance boundary with conventional message.
4. Record evidence in `.omo/evidence/local-first-multinode-architecture-lock/` and `.omo/start-work/ledger.jsonl`.
5. Mark plan checkbox; continue until todos 1–12 and F1–F4 complete.

## Approval gates

- Physical iPhone install/release: only at todo 11 final boundary.
- macOS `/Applications` install: only at final proof boundary.
- No weakening tests, narrowing scope, or swapping mocks to pass.

## Completion proof

- Every todo acceptance criterion has artifact path.
- F3: installed macOS app + Looper Siri simulator + optional physical iPhone; 100 assistant switches without stream restart; mode/prompt/reply unblock on ACK.
- F4: HTTP recovery-only; Session compact frames; honest stale/fresh/liveness labels; `last_seq_by_node` semantics.
- OSLog/ETTrace or perf-loop artifacts for switcher behavior.
- p95 latency + health/snapshot timing artifacts.

## Current next action

Todos 1-10 are complete and reviewed. Execute Todo 11 next:

- Run profiler-led simulator and installed proof.
- Use OSLog, ETTrace/perf-loop, Codex in-app Browser simulator proof, macOS install/launch proof, and one physical iPhone install/launch proof.
- Record p95 mode/prompt/switch timings plus server health/snapshot timings.

Do not repeat install/build loops. Run the final proof sequence once per surface unless a concrete blocker requires a retry.

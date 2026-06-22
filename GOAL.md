# Looper No-Edges ACP, Hooks, Routes, Timing, and Classification Goal

## Outcome

Make the Looper control plane prove, end to end, that ACP client hosts and targets work across Rust APIs, hooks, routes, macOS/iOS clients, and live device/runtime surfaces without stale labels, missing edges, timing/classification drift, or unverifiable desktop UI claims.

## Baseline

- Current branch: `cx/looper-mobile-realtime-smooth`.
- Rust control-plane routes already include `/desktop/acp-targets`, `/desktop/acp-client-hosts`, `/desktop/acp-client-hosts/:client_id`, `/desktop/acp-client-hosts/:client_id/probe`, `/desktop/acp-client-hosts/:client_id/install`, `/acp/client-hosts/:client_id`, hook mutation routes, desktop session routes, mobile snapshot/events routes, and gRPC mobile services.
- ACP host modules exist for Devin and Zed under `crates/agent-control-plane/src/acp_client_host`.
- Hook ownership exists for Codex, Grok, Claude, and Devin under Rust modules.
- iOS app proof now uses the built-in Xcode 27 simulator path; physical phone install is explicitly out of scope for the current acceptance pass.
- Simulator install/launch proof exists for `dev.looper.app.ios`.
- Zed and Devin can be inspected through Computer Use.
- `/Applications/looper.app` currently times out through Computer Use because the macOS app is menu-bar only. That is not an acceptable completion exception. Completion requires either a Computer Use-visible Looper desktop surface or a product-owned diagnostic surface that exposes the same ACP, route, timing, and classification state.

## Required Surface

- ACP targets:
  - Zed and Devin client hosts are visible through the generic `/desktop/acp-client-hosts` API and host-specific endpoints.
  - Zed ACP targets are read-only and probeable without false install affordances.
  - Devin exposes install/probe/runtime/session metadata without executing untrusted third-party commands.
  - Legacy Devin ACP bridge routes remain compatible or are explicitly removed with migration proof.
- Hooks:
  - Register, unregister, unregister-live, and target-specific hook routes cover Codex, Grok, Claude, and Devin where supported.
  - Owned Looper hooks are mutated; user hooks and foreign handlers are preserved.
  - Hook contract endpoints describe the actual local relay behavior.
- Routes:
  - Rust HTTP routes, macOS client paths, iOS client paths, and mobile gRPC/HTTP route semantics match.
  - Remote callers cannot hit install or destructive local-only routes.
  - Route errors are explicit, typed enough for clients, and covered by tests.
- Timing:
  - `created_at_ms`, `updated_at_ms`, `latest_message_at_ms`, event cursors, mobile revisions, and cache freshness are monotonic where required and do not reorder active sessions incorrectly.
  - UI refresh and route caches have named durations and do not hide real changes past their freshness contract.
- Classification:
  - Sessions launched by ACP, hooks, mobile prompts, local agents, subagents, and imported desktop clients are labeled consistently in Rust snapshots and Swift models.
  - `source`, `originator`, assistant surface, capabilities, and prompt-delivery target remain consistent across desktop snapshot, mobile snapshot, session detail, and Siri/App Intents where applicable.
- E2E edges:
  - API, CLI, macOS menu, Computer Use-visible Looper desktop diagnostics, iOS companion simulator install/launch, and live local server behavior are all verified.
  - Missing platform edges are work items unless they are impossible because of a current external state outside the repo and no product-side fallback can be implemented.
  - A menu-bar capture timeout is product work, not a blocker, until Looper has an inspectable diagnostic surface or equivalent.

## Verifiers

Primary verifier:

```sh
cargo test --manifest-path crates/agent-control-plane/Cargo.toml
```

Supporting verifiers:

```sh
cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check
cargo clippy --manifest-path crates/agent-control-plane/Cargo.toml --all-targets --all-features -- -D warnings
swift test --package-path macos/LooperMenuBar
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'generic/platform=iOS' build
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=27.0' -only-testing:LooperCompanionTests test
git diff --check
```

Runtime verifiers:

```sh
pnpm run dev:server
curl --fail http://127.0.0.1:8765/desktop/acp-client-hosts
curl --fail http://127.0.0.1:8765/desktop/acp-targets
curl --fail http://127.0.0.1:8765/status/control-plane
looper acp hosts --format json
looper acp probe zed --format json
looper acp probe devin --format json
xcrun simctl install <simulator-udid> <built Looper.app>
xcrun simctl launch <simulator-udid> dev.looper.app.ios
```

Computer-use verifier:

```text
Use computer-use to inspect running Zed, Devin, and a Looper-owned visible desktop surface. The Looper surface must show ACP hosts, ACP targets, hook/route health, timing freshness, and classification labels well enough for the accessibility tree or screenshot to confirm coherence.
```

## Iteration Loop

1. Inspect one surface or edge.
2. Add or tighten the smallest code/test change that makes that edge mechanically true.
3. Run the narrow verifier for the touched surface.
4. Record evidence and the next uncovered edge.
5. Repeat until the primary verifier, supporting verifiers, runtime verifiers, and computer-use verifier pass. Do not convert a repo-solvable visibility gap into an external blocker.

## Anti-Cheating Rules

- Do not weaken, skip, or delete existing coverage to get green.
- Do not replace real ACP, hook, route, timing, or classification behavior with UI-only labels.
- Do not mark complete from unit tests alone while a live server, macOS app, Zed, Devin, or simulator verifier is available.
- Do not mark complete while Looper itself is invisible to Computer Use if a diagnostic surface can be implemented in the macOS app.
- Do not execute untrusted commands from external ACP/host metadata.
- Do not mutate `.git`, `.env`, credential files, or files outside the project root.
- Do not hide a stale or blocked edge as unsupported unless the code and docs agree.

## Approval Gates

- Physical iPhone install is not part of the current acceptance path; use the built-in simulator unless the user explicitly reopens phone delivery.
- Replacing `/Applications/looper.app`, publishing, notarizing, TestFlight/App Store distribution, deleting user data, changing credentials, or destructive cleanup requires fresh explicit approval.

## Blocker Standard

Only mark blocked after the same external blocker recurs for three goal turns and no safe repository or verifier work remains. A failing test, confusing route, slow app, missing label, missing diagnostic surface, or incomplete edge is work, not a blocker.

## Completion Proof

The goal is complete only when:

- The primary verifier passes.
- Supporting verifiers relevant to touched Rust/Swift files pass.
- Live HTTP/CLI probes prove ACP hosts, ACP targets, hook status, route safety, and mobile snapshot consistency.
- Computer-use inspection succeeds for Zed, Devin, and a Looper-owned visible desktop surface. A Looper menu-bar timeout alone is not sufficient completion proof.
- The iOS app is rebuilt, installed, and launched on the built-in Xcode simulator if iOS or shared mobile models changed.
- Final evidence names the exact commands, important outputs, touched paths, and any intentionally deferred non-goals.

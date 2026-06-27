# Looper Mobile Realtime Latency Goal

## Outcome

Make the Looper mobile companion realtime path at least 10x faster for the
reported workflow and prove it through the installed app surface.

The workflow is:

- Connect the iOS companion to the local Rust control plane.
- Open a session and switch it to a send-capable mode.
- Send a prompt to that session.

## Baseline

User-reported baseline on June 23, 2026:

- App connect takes about 20 seconds.
- Switching to send mode takes about 15 seconds.
- Sending a message takes about 15 seconds.
- Total perceived workflow time is about one minute.

Current repo state:

- The Rust control plane under `crates/agent-control-plane` is the backend source
  of truth.
- iOS uses `ios/LooperCompanion` plus the shared Rust client-core Swift package
  under `swift/LooperClientCore`.
- The current working tree already contains uncommitted latency-path edits for
  gRPC keepalive, warmed Swift realtime connections, route racing, fast HTTP
  fallback timeouts, and prompt ACK-first behavior.
- Those edits are not sufficient completion proof until the installed app surface
  is driven and timed.

## Target

The goal is complete only when the same workflow is measured at or below:

- Connect ready: 2.0 seconds.
- Mode switch accepted and reflected in UI state: 1.5 seconds.
- Prompt send ACK and UI unblocked: 1.5 seconds.
- Total connect plus mode switch plus prompt ACK: 6.0 seconds.

Use p95 from at least 10 clean runs when the verifier can automate repeated
runs. For manual-only evidence, record at least 3 clean consecutive runs and
keep the stronger automated timing harness as remaining work unless it is
impossible with current Xcode tooling.

## Constraints

- Keep edits inside `/Users/ay/Documents/looper`.
- Do not mutate `.git`, `.env`, credential files, user credentials, or external
  config files.
- Do not weaken tests, hide failures, narrow the measured workflow, or replace
  the real server/app path with mocks.
- Do not treat build output, unit tests, or simulator launch alone as completion.
- Prefer local-first Rust control-plane behavior and native SwiftUI/iOS clients.
- Use current Xcode beta for iOS proof:
  `/Applications/Xcode-beta.app/Contents/Developer`.

## Primary Verifier

Install and launch the iOS companion, connect it to a live local Rust control
plane, perform the reported workflow, and record measured timings for connect,
mode switch, and prompt send ACK.

Preferred simulator verifier:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  xcodebuild -project ios/LooperCompanion.xcodeproj \
  -scheme LooperCompanion \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=27.0' \
  test
```

The simulator test suite or a dedicated checked-in latency harness must drive
the app UI or app model against a live local server and emit the measured timing
lines.

Physical iPhone proof is stronger when available, but it requires explicit
approval before installing to a device.

## Supporting Checks

Run the relevant checks for every touched surface:

```sh
cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check
cargo test --manifest-path crates/agent-control-plane/Cargo.toml grpc_mobile -- --nocapture
cargo check --manifest-path crates/agent-control-plane/Cargo.toml
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer bash scripts/check-ios.sh
git diff --check
```

If macOS shared realtime or menu-bar integration changes, also run:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path macos/LooperMenuBar
```

## Iteration Loop

1. Measure the installed-app workflow and record exact timing evidence.
2. Identify the slowest segment by timestamp, diagnostics log, or profiler data.
3. Change one meaningful latency owner: transport warmup, route selection,
   server dispatch, snapshot/detail refresh, UI state application, or fallback
   behavior.
4. Run the narrow verifier for that owner.
5. Re-run the primary workflow timing.
6. Record evidence and the next slowest segment in `docs/qa/mobile-realtime-latency.md`.
7. Repeat until the primary verifier and supporting checks pass.

## Anti-Cheating Rules

- Do not count server-only, model-only, or mock-only timings as app completion.
- Do not remove route fallbacks, auth checks, snapshot correctness, or mobile
  event reliability to win latency.
- Do not increase perceived speed by hiding pending work without a reliable
  ACK or eventual consistency path.
- Do not accept one lucky run. Use p95 from repeated clean runs when automation
  is possible.
- Do not mark complete while the app still performs serial multi-second route
  waits on a stale primary URL.
- Do not mark complete unless prompt send remains correct for the target
  assistant surface.

## Approval Gates

Ask before:

- Installing to a physical iPhone.
- Replacing `/Applications/looper.app`.
- Publishing, notarizing, TestFlight/App Store distribution, or network-hosted
  deployment.
- Deleting simulator data, DerivedData, user data, external config, or anything
  outside the project root.

## Blocker Standard

Only mark blocked after the same external blocker recurs for three consecutive
goal turns and no safe repository-side measurement, simulator proof, or latency
owner remains. A slow app, failing test, missing instrumentation, or absent
timing harness is work, not a blocker.

## Completion Proof

Before `update_goal(status="complete")`, provide:

- The exact changed paths.
- The exact build/test commands and passing outputs.
- A checked-in latency evidence file at `docs/qa/mobile-realtime-latency.md`.
- Installed-app workflow evidence naming simulator/device, server endpoint,
  sample count, connect p95, mode-switch p95, prompt-ACK p95, and total p95.
- A note for any intentionally deferred non-goal.

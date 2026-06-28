---
name: ios-session-sync-debugging
description: Debug and fix Looper iOS Session sync, assistant switcher, Siri/default-session, local-first command lifecycle, latest-wins ordering, and stale async result bugs. Use when iOS taps or App Intents reach the app but state, pending commands, selected assistant surface, or Rust client-core outbox behavior is wrong.
---

# Looper iOS Session Sync Debugging

Use this skill when the iOS bug is about command ordering or state ownership, not render speed. Typical examples: assistant switcher rapid taps, wrong selected surface, stale async results overwriting a newer desired state, Siri/default-session routing drift, pending command count mismatches, or local-first state-mini/outbox confusion.

## Architecture Rules

- Keep Rust/LooperClientCore as the source of truth for durable command state, pending commands, mutation IDs, and state-mini snapshots.
- Keep HTTP as bootstrap/recovery only. Normal live command/state behavior should use Session gRPC through `swift/LooperClientCore`.
- Give iOS one sync/data owner for command lifecycle. Prefer `SessionSyncEngine` or an existing client manager in `ios/LooperCompanion/Services/`.
- Keep `CompanionAppModel` as a presenter/view-state coordinator. It may project desired UI state immediately and apply accepted results, but it should not own queues, debouncing, serialization, or stale-result policy.
- Do not fix overlapping command bugs with another ad hoc `Task` patch in `CompanionAppModel`.

## First Pass

1. Reconcile current evidence before coding.
2. Check whether the tap/action reached the app. If browser/serve-sim or OSLog already proves the tap, do not blame the browser.
3. Inspect the command lifecycle seam:
   - `ios/LooperCompanion/App/CompanionAppModel.swift`
   - `ios/LooperCompanion/Services/CompanionSessionMiniController.swift`
   - `ios/LooperCompanion/Services/CompanionSessionMiniLocalStore.swift`
   - `swift/LooperClientCore/Sources/LooperClientCore/LooperClientCoreSessionManager.swift`
   - `crates/looper-client-core/src/local_store.rs`
   - `crates/looper-client-core/src/client.rs`
4. Confirm whether Rust/client-core already has latest-wins semantics before adding Swift policy.
5. If the bug is command lifecycle overlap, create or update the iOS sync engine seam instead of expanding the presenter.

## Latest-Wins Contract

For assistant-surface and similar singleton commands:

- Tap updates desired view state immediately when a dispatch can be accepted.
- Dispatch is coalesced and serialized.
- Each command has a generation or mutation identity.
- Stale results cannot overwrite a newer desired state.
- Repeated taps for the already desired surface are no-ops.
- Pending command state is visible in logs and tests.
- The runtime/client-core pending command should collapse to the latest surface.

## Test Pattern

Add a deterministic failing-first unit test before the fix. Do not rely on real gRPC speed to expose overlap; real runtimes can accept too fast.

Use a fake command dispatcher that records dispatched surfaces and pending commands. The red failure should show one of:

- stale first result returned to the caller
- both old and new surfaces dispatched
- pending command surface is not the latest surface
- visible selected surface does not update to the latest desired surface

Then add or update the app-model test to assert:

- selected surface changes immediately to the latest desired surface
- pending command count is visible during dispatch
- both rapid selection tasks resolve successfully when coalesced to the latest command
- runtime pending commands contain one `setAssistantSurface` with the latest `assistantSurface`

Useful tests:

- `ios/LooperCompanionTests/CompanionSessionMiniLocalFirstTests.swift`
- `ios/LooperCompanionTests/CompanionSessionRuntimeCommandCoreTests.swift`

Use XcodeBuildMCP with the Swift Testing `()` selector form for exact function filters, for example:

```bash
-only-testing:LooperCompanionTests/CompanionSessionMiniLocalFirstTests/testRapidAssistantSurfaceSwitchesResolveToLatestSelection()
```

Without `()`, Xcode may select the suite but run zero Swift Testing tests.

## Runtime Proof

Behavior proof must include the installed simulator app, not just compile/tests.

1. Use the iOS debugger agent to build, install, and launch the app on the configured simulator.
2. Use `ios-simulator-browser` / `serve-sim` for browser-visible proof when the user points at the in-app browser.
3. Capture `AssistantSurface` OSLog lines around the flow:
   - `Selection requested`
   - `Selection coalesced` when reproducing true overlap
   - `Selection dispatch`
   - `Selection applied`
   - `Selection stale` / `failed` only when relevant
4. Report the exact simulator, app bundle, test selector, and log path.

The browser CUA helper can serialize coordinate taps. If it cannot create true rapid overlap, say so directly and use deterministic unit tests for overlap proof while using browser taps to prove the installed app path.

## Performance Boundary

Use `$ios-perf-diagnostics` only after behavior is fixed or when the question is genuinely about latency, hitches, hangs, or render invalidation.

For assistant switcher bugs:

- ETTrace spans around 150-200 ms usually indicate render cost is not the primary problem.
- Prefer command lifecycle logs and latest-wins tests before profiling.
- Keep ETTrace framework wiring temporary unless the user explicitly asks to keep it.

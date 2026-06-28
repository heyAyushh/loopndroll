---
name: looper-realtime-cutter
description: Use for Looper realtime/state architecture-lock work where iOS, macOS, Rust client-core, or agent-control-plane can lie about connectivity, stale sessions, SessionMini visibility, endpoint switching, or command ACKs. Starts from decisions.md and local/device truth, fans out bounded cutters, runs cheap gates while coding, and installs only once at the final proof boundary.
---

# Looper Realtime Cutter

Use this skill when the bug crosses transport, state-mini projection, client-core local store, iOS/macOS readiness, or Codex session visibility. It exists to prevent the slow loop where each symptom is fixed in isolation and then rediscovered on the phone.

## Non-Negotiables

- Read `docs/architecture/decisions.md` before touching transport, state, client-core, iOS, or macOS.
- Treat Rust `crates/agent-control-plane` and `crates/looper-client-core` as the state/command owners.
- Treat SwiftUI/AppKit as lifecycle and presentation owners only.
- HTTP is bootstrap, health, pairing, manual diagnostics, and recovery snapshot only. It is not hot command/state truth.
- The hot path is one `Session` stream with compact ACKs, command intent, state-mini deltas, and local store reconciliation.
- Never call the iPhone UI proof done from simulator-only evidence when the failing surface is a paired physical phone.
- Do not run broad iOS/macOS/full client-core gates while coding. Run cheap focused checks, then run acceptance gates once at the boundary.

## First Truth Pass

Do one broad pass before editing. Do not start fixing the first file you find.

1. Capture current repo and session truth:

```bash
git status --short
git log --oneline -8
sqlite3 ~/.codex/state_5.sqlite "SELECT id,title,source,agent_nickname,agent_role,datetime(recency_at_ms/1000,'unixepoch') FROM threads ORDER BY recency_at_ms DESC LIMIT 20;"
```

2. If the bug is missing/stale Codex sessions, compare the Codex source of truth against the phone local store:

```bash
sqlite3 ~/.codex/state_5.sqlite "SELECT COUNT(*), SUM(CASE WHEN archived THEN 1 ELSE 0 END), SUM(CASE WHEN archived THEN 0 ELSE 1 END) FROM threads;"
```

Pull the app support store only when device proof is needed:

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer xcrun devicectl device copy from \
  --device <device-id> \
  --domain-type appDataContainer \
  --domain-identifier dev.looper.app.ios \
  --source "Library/Application Support" \
  --destination .build/device-pull-current/ApplicationSupport \
  --remove-existing-content \
  --timeout 60
```

3. Count the pulled `looper-realtime-state-minis.json` by assistant surface and status before touching UI code.

4. Run the strict stale-transport scan before and after the cut:

```bash
rg -n "text/event-stream|MobileEventStream|LooperRealtimeStateMiniSynchronizer|/mobile/events|/desktop/events|HTTP.*sendSessionPrompt|HTTP.*setSessionMode|HTTP.*submitNotificationReply|loadSnapshot\\(\\).*sendSessionPrompt" ios macos swift crates/agent-control-plane crates/looper-client-core || true
```

## Teammode Cutters

Use bounded cutters when the surface is wider than one file. Each cutter returns either `commit hash + focused gate` or one blocker with exact file/line. No broad reports.

### Server Projection Cutter

Scope:

- `crates/agent-control-plane/src/mobile/api/session_mini.rs`
- `crates/agent-control-plane/src/grpc/service.rs`
- `crates/agent-control-plane/src/http/mobile_state.rs`
- focused isolated control-plane tests

Prompt:

```text
Read docs/architecture/decisions.md first. Fix only server SessionMini projection/event emission for the reported stale/missing mini class. Do not touch iOS/macOS/client-core. Use Codex sqlite/session truth as the source, preserve compact frames, and return commit hash + focused Rust gate or one exact blocker.
```

### Client-Core Runtime Cutter

Scope:

- `crates/looper-client-core/src/client.rs`
- `crates/looper-client-core/src/local_store.rs`
- `crates/looper-client-core/src/session_runtime.rs`
- `crates/looper-client-core/src/mobile_snapshot.rs`
- `swift/LooperClientCore` only when UniFFI regeneration is required

Prompt:

```text
Read docs/architecture/decisions.md first. Fix only Rust client-core Session runtime/store ownership: endpoint adoption, ACK reconciliation, local snapshot projection, pending command drainage, and replacement deltas. Swift remains a thin lifecycle wrapper. Use cheap cargo tests while coding and return commit hash + focused gate or one exact blocker.
```

### iOS Surface Cutter

Scope:

- `ios/LooperCompanion/App/CompanionAppModel.swift`
- `ios/LooperCompanion/Services/`
- `ios/LooperCompanion/AppIntents/`
- focused app/core tests

Prompt:

```text
Read docs/architecture/decisions.md first and use $ios-session-sync-debugging if command ordering is involved. Fix only iOS stale surface truth: local minis render first, Swift calls Rust client-core intent APIs, no HTTP hot command/state truth, no optional runtime no-op. Do not run full simulator suites while coding. Return commit hash + focused test or one exact blocker.
```

### macOS Surface Cutter

Scope:

- `macos/LooperMenuBar/Sources/LooperMenuBarCore/`
- menu refresh, route readiness, notification reply redraw
- focused SwiftPM tests

Prompt:

```text
Read docs/architecture/decisions.md first. Fix only macOS stale visible truth: SessionMini remains primary row/menu truth, route/readiness labels must be live/proven or explicitly stale, notification reply redraw cannot lie before stream echo. No new HTTP hot path. Return commit hash + focused SwiftPM gate or one exact blocker.
```

### Device Truth Cutter

Scope:

- read-only device store/log pulls under `.build/`
- no screenshots unless the user asks
- no code edits

Prompt:

```text
Read docs/architecture/decisions.md first. Pull only app support/log evidence needed to prove current installed app truth. Compare phone local store, Codex sqlite truth, latest seq, per-surface counts, pending commands, and stream/runtime restart markers. No screenshots, no installs, no edits. Return one concise evidence artifact.
```

## Patch Rules

- Map all stale surfaces in the boundary first, then patch. Do not fix one symptom and rebuild.
- Batch UniFFI/Rust export changes before regenerating `swift/LooperClientCore`.
- Commit only acceptance boundaries, not renames or scratch cleanup.
- If a command starts taking repeated waiting, narrow it or stop and record the exact blocker.
- Do not weaken tests to fit the current architecture. The architecture is the lock.

## Cheap Gates

Use only gates that match changed files until final acceptance:

```bash
cargo test --manifest-path crates/looper-client-core/Cargo.toml <filter> -- --nocapture
cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane <filter> -- --nocapture
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path swift/LooperClientCore --filter <filter>
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift test --package-path ios/LooperCompanionCore --filter <filter>
swift test --package-path macos/LooperMenuBar --filter <filter>
```

Final boundary gates, not every edit:

```bash
pnpm run check:client-core
git diff --check
```

Install once only when the build boundary is green and the user asked for installed proof.

## Stop Conditions

Stop and report a blocker only when:

- the needed device is unavailable,
- signing/build tooling cannot produce an installable app,
- a concurrent worktree change directly conflicts with the files you must edit,
- or the smallest focused gate fails outside your changed boundary.

Otherwise keep cutting until the boundary is coherent.

# Looper Realtime Cutter Workflow

This workflow came from the 2026-06-28 realtime cleanup session where the slow loop was not one hard bug. It was repeated rediscovery across server projection, Rust client-core local store, iOS presentation, macOS readiness, physical-device truth, and Codex session truth.

## Trigger

Use this when any of these are true:

- iOS/macOS says connected but shows stale, missing, or local-only sessions.
- Assistant surface switching, Siri current/default session, prompt send, or notification reply feels delayed or unreliable.
- Tailscale/LAN/localhost route switching changes visible readiness.
- Phone local store disagrees with `~/.codex/state_5.sqlite`.
- A fix starts needing repeated installs or broad gates before the code boundary is understood.

## Operating Model

1. Read `docs/architecture/decisions.md`.
2. Inspect the current checkout and Codex session truth once.
3. Split work into bounded cutters:
   - server projection/event emission,
   - Rust client-core runtime/store,
   - iOS surface,
   - macOS surface,
   - read-only device truth.
4. Each cutter returns only a commit hash plus focused gate, or one blocker with exact file/line.
5. Merge only commits that remove stale surfaces.
6. Run final gates once.
7. Install once when installed-app proof is the requested surface.

## Truth Sources

- Codex session truth: `~/.codex/state_5.sqlite` and `~/.codex/sessions`.
- Phone local realtime store: `Library/Application Support/looper-realtime-state-minis.json`.
- Architecture truth: `docs/architecture/decisions.md`.
- Client-core truth: `crates/looper-client-core/src/client.rs`, `local_store.rs`, `session_runtime.rs`, `mobile_snapshot.rs`.
- Server projection truth: `crates/agent-control-plane/src/mobile/api/session_mini.rs`, `grpc/service.rs`, `http/mobile_state.rs`.

## Acceptance

- No stale hot transport symbols in iOS/macOS/Swift/Rust.
- No HTTP hot command/state mutation path.
- Swift actions call Rust client-core intent APIs.
- Local minis render before network.
- Route switch proves the new live Session endpoint before claiming connected.
- Missing-session issues are checked against Codex sqlite and the phone local store before UI work.
- Focused tests pass for changed surfaces.
- Installed app proof is run once when the phone/mac app is the requested surface.

## Anti-Patterns

- Starting with a full build before the ownership boundary is known.
- Fixing one stale UI label while server/client-core store truth is still wrong.
- Treating the iPhone screenshot as truth when the local store and Codex sqlite disagree.
- Re-running full iOS/macOS gates after every edit.
- Adding another Swift coordinator, HTTP fallback, or socket instead of simplifying the existing Session/runtime path.

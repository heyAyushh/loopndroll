# Project Learnings

> Managed by `/learn`. Append-only - latest entry wins on conflicts.

## Patterns

### looper-realtime-cutter-boundaries
- **Insight:** For Looper realtime bugs, split work into bounded server projection, Rust client-core runtime/store, iOS surface, macOS surface, and device-truth cutters; each returns a commit plus focused gate or one exact blocker.
- **Confidence:** 9/10
- **Source:** codex-workflow-miner
- **Files:** .agents/skills/looper-realtime-cutter/SKILL.md, docs/architecture/realtime-cutter-workflow.md
- **Date:** 2026-06-29

### looper-cheap-gates-before-final-proof
- **Insight:** During Looper architecture-lock cuts, run only focused checks for changed files while coding, then run `pnpm run check:client-core`, strict grep, and install proof once at the acceptance boundary.
- **Confidence:** 9/10
- **Source:** codex-workflow-miner
- **Files:** .agents/skills/looper-realtime-cutter/SKILL.md
- **Date:** 2026-06-29

## Pitfalls

### looper-phone-ui-is-not-session-truth
- **Insight:** When iPhone sessions are stale or missing, compare `~/.codex/state_5.sqlite` and the phone `looper-realtime-state-minis.json` before touching SwiftUI; the screenshot can be a stale projection, not the truth.
- **Confidence:** 9/10
- **Source:** codex-workflow-miner
- **Files:** .agents/skills/looper-realtime-cutter/SKILL.md
- **Date:** 2026-06-29

### looper-one-by-one-fixes-waste-time
- **Insight:** Realtime regressions should be mapped across server, client-core, iOS, macOS, and device store in one pass before editing; isolated symptom fixes caused repeated installs and slow progress.
- **Confidence:** 9/10
- **Source:** codex-workflow-miner
- **Files:** docs/architecture/realtime-cutter-workflow.md
- **Date:** 2026-06-29

## Preferences

### looper-installed-proof-only-on-boundary
- **Insight:** The user wants physical iPhone/macOS install proof only after the relevant code boundary compiles and focused gates pass, not repeated installs after every small edit.
- **Confidence:** 10/10
- **Source:** codex-workflow-miner
- **Files:** .agents/skills/looper-realtime-cutter/SKILL.md
- **Date:** 2026-06-29

## Architecture

### looper-session-stream-is-hot-control-plane
- **Insight:** The hot Looper path is one Rust-owned `Session` stream carrying compact ACKs, command intents, state-mini deltas, and local-store reconciliation; HTTP stays bootstrap/recovery/diagnostics only.
- **Confidence:** 10/10
- **Source:** codex-workflow-miner
- **Files:** docs/architecture/decisions.md, .agents/skills/looper-realtime-cutter/SKILL.md
- **Date:** 2026-06-29

## Tools

### looper-device-store-counts
- **Insight:** For phone state regressions, use `xcrun devicectl device copy from ... Library/Application Support` and count `looper-realtime-state-minis.json` by surface/status before debugging UI filters.
- **Confidence:** 9/10
- **Source:** codex-workflow-miner
- **Files:** .agents/skills/looper-realtime-cutter/SKILL.md
- **Date:** 2026-06-29

# Looper Agent Notes

## Repository

Looper keeps Codex chats moving after Codex stops. The active product split is:

- `looper-server`: Rust daemon and source of truth for state, hooks, sessions, scheduler, auth, mobile API, persistence, notifications, and events.
- `looper`: primary Rust terminal command for inline attach plus server control, hooks, sessions, settings, notifications, checks, connections, pairing, and shutdown.
- `looper-cli`: bundled compatibility binary for script callers; product docs and help should prefer `looper`.
- `macos/LooperMenuBar`: native macOS menu bar client that launches and controls the bundled Rust server; it does not open a separate terminal window.
- `ios/LooperCompanion`: native iPhone client pointed at the Rust mobile API.

## Backend Direction

- Rust is the backend and control-plane source of truth.
- Do not add core backend, auth, mobile API, hook, notification, or session-control behavior outside `crates/agent-control-plane`.
- Keep macOS, iOS, and TUI surfaces as clients of the Rust HTTP/SSE API.
- Keep server lifecycle local-first. Tailscale-style reachable URLs are a boundary for remote control; hosted/cloud paths are not the default.

## Code Style

- Rust changes must pass `cargo fmt --check --manifest-path crates/agent-control-plane/Cargo.toml`.
- Swift changes must pass the relevant SwiftPM or Xcode build target.
- No magic numbers: use named constants with descriptive names.
- Prefer meaningful names, small functions, and narrow typed contracts.
- Comments should explain non-obvious intent or side effects, not restate code.
- Do not leave compatibility shims or dead paths behind once their replacement is verified.

## Folder Structure

- `crates/agent-control-plane`: Rust server, CLI, TUI, hook runtime, mobile API, auth, notifications, and tests.
- `crates/orb-code`: orb-code generator/decoder used by pairing flows.
- `macos/LooperMenuBar`: native macOS menu bar package.
- `ios/LooperCompanion`: native iPhone companion app.
- `scripts/build-macos-menu-bar-package.sh`: packages the native menu bar app with bundled Rust binaries.

## Debugging

- Start the Rust server with `pnpm dev` or `pnpm run dev:server`.
- Start the Rust terminal surface inline with `pnpm run dev:tui` or `looper`.
- Check terminal backend and local server readiness with `pnpm run doctor`.
- Start the iPhone-facing API with `pnpm run dev:ios-api`.
- Run repo checks with `pnpm check`.
- SQLite control-plane state: `~/Library/Application Support/looper/agent-control-plane.sqlite`.
- Managed Codex hook/config files touched by the app: `~/.codex/hooks.json` and `~/.codex/config.toml`.

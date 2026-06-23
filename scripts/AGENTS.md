# AGENTS.md - Scripts

## OVERVIEW

`scripts/` contains repo-local build, install, release, generation, validation, and lint-guard tooling. These scripts are product surfaces, not disposable helpers.

## WHERE TO LOOK

| Task | Script | Notes |
| --- | --- | --- |
| macOS package | `build-macos-menu-bar-package.sh` | Builds Swift package and Rust release binaries, assembles signed app. |
| macOS Xcode build/install | `build-macos-menu-bar-xcode.sh` | Requires XcodeGen and uses Xcode project path. |
| release | `release-macos.sh` | Reads `.env`, checks `gh`, signing, notarization, dirty tree, artifacts. |
| iOS gate | `check-ios.sh` | Regenerates project, runs Swift/Xcode checks, validates App Intents metadata. |
| Xcode 27 beta 2 proof | `prove-xcode27-beta2.sh` | Fetches official release notes, verifies selected beta toolchain/runtime, `devicectl` JSON stdout, Siri surface proof, and guarded AI/debug capabilities. |
| Siri runtime proof | `prove-ios-siri-runtime.sh` | Builds, installs, launches Looper on an iOS simulator, validates App Intents metadata, opens Siri, and optionally attempts physical iPhone proof. |
| CLI install | `install-looper-cli.sh` | Installs `looper`, `looper-cli`, `looper-server`; guarded against outside-root prefix. |
| Swift gRPC generation | `generate-swift-grpc.sh` | Writes generated Swift files from Rust proto. |
| OrbCode XCFramework | `build-orb-code-ios-package.sh` | Recreates `ios/OrbCodeKit/Frameworks/OrbCodeFFI.xcframework`. |
| lint rule guard | `lint-rule-guard/`, `run-lint-rule-guard.sh` | Standalone Rust helper under scripts. |

## CONVENTIONS

- Keep scripts idempotent where possible and fail fast with `set -euo pipefail`.
- Resolve paths from the script location, not the caller's shell cwd.
- Treat `.env`, signing credentials, provisioning profiles, and notarization variables as local secret inputs; never print or rewrite them.
- For scripts that touch `/Applications`, device installs, external config, or generated source, report exact target paths before running unless the user explicitly asked for that install/generation step.
- Prefer existing repo scripts over ad hoc command sequences.

## ANTI-PATTERNS

- Do not add silent `rm -rf` cleanup. Use narrow paths and print target lists.
- Do not make release scripts ignore dirty trees by default.
- Do not write outside the project root unless the script already has an explicit flag/target for that behavior.
- Do not regenerate Swift gRPC or OrbCode frameworks without reviewing the resulting tracked diff.

## COMMANDS

```bash
bash scripts/build-macos-menu-bar-package.sh --no-install
bash scripts/build-macos-menu-bar-xcode.sh --no-install
bash scripts/check-ios.sh
bash scripts/prove-xcode27-beta2.sh
bash scripts/prove-ios-siri-runtime.sh --simulator-only
bash scripts/install-looper-cli.sh
bash scripts/release-macos.sh
bash scripts/generate-swift-grpc.sh
bash scripts/build-orb-code-ios-package.sh
cargo build --release --manifest-path scripts/lint-rule-guard/Cargo.toml
```

## NOTES

- `install-looper-cli.sh` defaults to `build/bin` and refuses outside-root prefixes unless `--allow-outside-project` is passed.
- `release-macos.sh` may tag/push and upload GitHub releases; do not run it casually.
- `check-ios.sh` is the App Intents/Siri metadata gate.

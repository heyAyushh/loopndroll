# Looper Siri/App Intents Goal

## Outcome

Make Looper sessions and Looper actions available across Siri, Spotlight, Shortcuts, and in-app onscreen awareness using App Intents end to end.

## Baseline

- `ios/LooperCompanion/AppIntents/LooperSiriSessionSupport.swift` already defines `LooperSessionEntity` as `AppEntity, IndexedEntity`.
- `ios/LooperCompanion/Services/SessionSpotlightIndexer.swift` already indexes session search items with `CSSearchableIndex`.
- Session list/search/detail views already carry entity identifiers or user activities in some places.
- Existing intents cover open, summarize, search, ask, default/current session, contextual prompts, and prompt suggestions.
- Missing or incomplete work must be treated as product gaps, not as docs-only work.

## Required Feature Surface

- App entities:
  - Keep `LooperSessionEntity` as the primary indexed entity for Spotlight and Siri resolution.
  - Add focused related/transient entities only when they improve Siri understanding without bloating the model.
  - Keep relationships explicit enough for Siri to understand session, assistant surface, prompt, project, status, and latest result context.
- Spotlight:
  - Donate indexed entities and searchable items when sessions are created, updated, hidden, archived, or deleted.
  - Delete stale identifiers when sessions disappear or change surface identifiers.
- Intents:
  - Open a session through a real `OpenIntent`/system-open equivalent that deep-links to the selected session.
  - Keep action intents thin and backed by Looper services.
  - Add create/update/delete-style Looper actions where they map to real Looper domain behavior, such as creating/sending prompts, changing default/current session, and deleting sessions.
  - For update semantics, preserve the distinction between no change, explicit clear, and new value where the platform API supports it.
- Onscreen awareness:
  - Use entity identifiers on session list/search rows.
  - Use user activity on detail views so Siri can resolve natural references like the current Looper session.
- Snippets:
  - Return custom SwiftUI snippet views for Siri result surfaces where supported, especially session summary/search/prompt results.
- Whole product:
  - iOS companion, App Intents metadata, Spotlight, navigation, macOS handoff/menu state, and Rust APIs must agree on identifiers and route semantics.

## Verifiers

Primary verifier:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'generic/platform=iOS' build
```

Supporting verifiers:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app xcodebuild -project ios/LooperCompanion.xcodeproj -scheme LooperCompanion -destination 'generic/platform=iOS' -showBuildSettings
DEVELOPER_DIR=/Applications/Xcode-beta.app swift test --package-path ios/LooperCompanionCore
DEVELOPER_DIR=/Applications/Xcode-beta.app swift test --package-path macos/LooperMenuBar
cargo fmt --manifest-path crates/agent-control-plane/Cargo.toml --check
cargo test --manifest-path crates/agent-control-plane/Cargo.toml
git diff --check
```

Runtime completion proof when device is available:

```sh
xcrun devicectl list devices
xcrun devicectl device install app --device <paired-device-id> <built LooperCompanion.app>
xcrun devicectl device process launch --device <paired-device-id> dev.looper.app.ios
```

## Anti-Cheating Rules

- Do not remove existing Siri/App Intents coverage to make builds pass.
- Do not weaken tests or metadata extraction.
- Do not replace real Spotlight/App Intents behavior with fake UI-only rows.
- Do not mark complete from build-only proof when phone/runtime proof is available.
- Do not write credentials, `.env`, or destructive cleanup outside the requested app install/build scope.

## Approval Gates

- Installing or replacing `/Applications/looper.app` or installing to a physical iPhone is allowed only when explicitly requested in the current turn or already needed for runtime proof and the device is attached.
- Publishing, notarizing, App Store/TestFlight distribution, deleting user data, or changing credentials requires separate explicit approval.

## Completion Proof

The goal is complete only when:

- The requested App Intents/Spotlight/OpenIntent/onscreen/snippet surfaces are implemented or a platform-limited item is documented with code-level fallback.
- The primary verifier passes.
- Supporting verifiers relevant to changed files pass.
- App Intents metadata/build output proves the intents and entities are included.
- If a paired iPhone is available, the app is installed and launched with the new build.

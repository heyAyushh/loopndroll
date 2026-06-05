# Looper iOS Companion Design

Date: 2026-04-17
Status: Proposed

## Summary

Build a native iPhone companion app for Looper. The Mac app remains the system of record for Codex sessions, hooks, notifications, and completion checks. The iPhone app connects to the Mac app over Tailscale for live reads and mutations, and receives Apple push notifications through a small Rust relay running on the Codex app server.

The iPhone app is not a standalone replacement for the Mac runtime. It is a remote control surface for the existing product.

## Goals

- Let a paired iPhone see the current Looper state from the Mac app.
- Let the iPhone manage nearly all user-facing Looper controls except Mac-only hook setup.
- Deliver native iPhone push notifications when Codex stops.
- Support configurable stop-event actions from the phone.
- Make the phone experience feel native, including haptics, background-safe navigation, and SwiftUI-first interaction patterns.

## Non-goals

- Running Codex or Looper hook logic on iPhone.
- Mirroring the desktop renderer or sharing the React UI.
- Replacing Telegram or Slack integrations.
- Exposing local Mac files, shell access, or Codex configuration editing directly to the phone.
- Making the push relay the source of truth for sessions.

## Product Shape

### Onboarding and pairing

The iPhone app opens into a focused pairing flow:

1. Welcome screen explains that Looper runs on Mac and the phone is a companion.
2. The user taps `Scan Mac QR`.
3. The Mac app shows a custom QR that contains:
   - Mac device identifier
   - Tailscale hostname or stable Tailnet address
   - one-time pairing nonce
   - short-lived public handshake material
   - server environment marker for push registration
4. The phone scans the QR, connects to the Mac over Tailscale, completes an app-level pairing handshake, then registers its APNs token.
5. The Mac stores the paired device record and optionally enables push delivery for that phone.

Pairing must require explicit scan-based consent. Tailnet presence alone is not enough.

### Main app structure

The iPhone app uses a two-tab shell:

- `Sessions`
- `Settings`

This keeps v1 aligned with the current Mac app information architecture and avoids inventing a third destination before there is distinct product value for it.

### Sessions tab

The top section shows the currently selected Mac connection:

- connection status
- last sync time
- global mode
- global notification target
- global completion check summary

Below that is the session list:

- active sessions first
- archived sessions behind a segmented control or filter
- each row shows session ref, title, effective mode, waiting/active/stopped state, and relative activity time

### Session detail

The detail screen exposes:

- session title and ref
- current status
- latest assistant message preview
- per-session mode controls
- attached notifications
- completion check selection
- wait-for-reply-after-checks toggle
- archive, unarchive, and delete
- configurable stop-event quick actions

Opening a push notification should deep-link directly into the relevant session detail screen.

### Settings tab

The iPhone settings surface includes:

- default continue prompt
- notifications management
- completion checks management
- paired Mac information
- trusted device management
- push notification preferences
- stop-event action preferences

Mac-only features such as hook registration should appear as read-only status with a `Manage on Mac` affordance.

## Architecture

### Mac app responsibilities

The existing Mac app remains responsible for:

- SQLite storage
- Codex hook orchestration
- Telegram and Slack integrations
- session registration and loop state changes
- completion-check execution

It gains four new responsibilities:

1. A mobile-safe control API.
2. Pairing issuance and paired-device trust management.
3. APNs relay publishing for stop events.
4. Device-aware settings for iPhone push behavior and quick actions.

### iPhone app responsibilities

The iPhone app owns:

- pairing and re-pairing
- device push token registration
- local navigation and deep links
- live reads and user-triggered mutations against the Mac API
- haptic feedback and iOS-native interaction behavior

### Push relay responsibilities

The Rust relay on the Codex app server is intentionally narrow:

- accept authenticated stop-event submissions from the Mac app
- map paired phone registrations to APNs device tokens
- send Apple push notifications
- return delivery results to the Mac app

The relay must not store full transcript history or become a session database.

### Transport split

- Live control: direct over Tailscale between iPhone and Mac.
- Push delivery: Mac app to relay over the public internet, then relay to APNs.

This keeps private control traffic peer-to-peer while still satisfying Apple push requirements.

## Data and API boundary

The current desktop code mixes domain state with Electrobun RPC. For iOS support, the Mac app should introduce a separate service boundary that reuses Looper domain logic but is independent of the renderer transport.

### New boundary on Mac

Add a `mobile-control-service` layer in the Bun side that exposes stable request/response models for:

- snapshot fetch
- session list and session detail
- global setting mutations
- session setting mutations
- notification CRUD
- completion-check CRUD
- paired device CRUD
- push preference updates
- one-shot stop-event actions

This service should call the same underlying Looper mutation functions already used by the desktop renderer.

### Shared models

Create a shared contract package or module for mobile-facing types. It should be narrower than the current desktop RPC schema:

- `MobileSnapshot`
- `MobileSessionSummary`
- `MobileSessionDetail`
- `MobileGlobalSettings`
- `MobileNotification`
- `MobileCompletionCheck`
- `PairedDevice`
- `PushPreferences`
- `StopEventActionPreference`

The iPhone app should not receive desktop-only window or updater state.

## Pairing design

### QR payload

The custom QR should contain a compact signed payload, not plain raw JSON without integrity protection. It needs:

- version
- Mac identifier
- Tailscale address
- nonce
- issued-at timestamp
- expiry timestamp
- ephemeral public key or handshake token
- signature from the Mac app

### Pairing flow

1. Mac app generates a short-lived QR payload.
2. Phone scans and validates payload freshness.
3. Phone opens a Tailscale connection to the Mac pairing endpoint.
4. Phone and Mac exchange proof material and establish trust.
5. Phone sends APNs token and device metadata.
6. Mac saves the paired device, chosen quick-action defaults, and relay registration state.

If pairing fails, the app must explain whether the failure is:

- expired QR
- Tailscale unreachable
- Mac app not listening
- trust verification failed
- push registration failed

## Push notification design

### Stop-event content

Push content should stay compact:

- session ref
- session title
- stop reason category
- short assistant preview
- Mac identifier
- session identifier

The full session detail is fetched from the Mac app after the user opens the notification.

### Quick actions

Push actions are configurable in Settings. The defaults should be:

- `Open Session`
- `Continue`

Other allowed actions:

- `Reply`
- `Archive`
- `Mute this session`

The settings screen should let the user choose which actions appear on stop-event notifications. Unsupported combinations should be prevented in the UI rather than handled as runtime surprises.

### Delivery behavior

The Mac app publishes push jobs only for paired phones that have push enabled. If relay delivery fails, the session still stops normally and remains visible in live status.

## Haptics and interaction polish

Haptics are part of v1, not post-launch polish.

Use them sparingly:

- success haptic on completed pairing
- warning haptic on destructive confirmation prompts
- light impact on applying a mode or quick action
- error haptic on failed mutations or unreachable Mac state

Do not trigger haptics on every row tap or passive refresh. The goal is signal, not noise.

## Error handling

### Connectivity states

The iPhone app should model these states explicitly:

- paired and connected
- paired but Mac unreachable
- paired but Tailscale unavailable
- paired but authentication rejected
- not paired

The user should always know whether the problem is the phone, the network, or the Mac host.

### Mutation behavior

Mutations should be optimistic only when rollback is simple and obvious. For destructive actions such as delete, the UI should wait for server confirmation.

When a mutation fails:

- show inline error state for the affected control
- emit an error haptic
- keep the last known good state visible

### Push fallback

If push is disabled or broken, the app should still work as a live remote client. Push is an enhancement, not a requirement for control.

## Testing strategy

### Mac app

- unit tests for pairing token generation and validation
- unit tests for device trust checks
- unit tests for mobile-facing request validation
- integration tests for mobile service mutations against the existing database logic
- integration tests for relay submission payloads

### iPhone app

- Swift unit tests for pairing payload decoding and validation
- Swift unit tests for API client request signing and error mapping
- SwiftUI preview coverage for key screens and state variants
- UI tests for:
  - first-time pairing
  - push deep link to session detail
  - session mode change
  - archive and delete confirmations
  - offline and reconnect states

### End-to-end verification

Before calling the feature done:

- pair a real iPhone with a local Mac dev build
- verify live session list loads over Tailscale
- verify session mutations reflect in the Mac app
- trigger a stop event and confirm APNs delivery
- open the notification and land in the correct session
- verify configured quick actions behave correctly

## Delivery plan

### Phase 1

- extract mobile-safe contracts
- add Mac mobile control service
- implement pairing issuance and trusted-device storage
- build iPhone onboarding, sessions, and settings shells

### Phase 2

- add push relay integration
- add APNs device registration
- add configurable stop-event quick actions
- add deep links from notifications

### Phase 3

- tighten haptics, error states, and reconnect behavior
- expand test coverage
- ship TestFlight build

## Open decisions resolved in this design

- Product type: iPhone companion app, not standalone Looper runtime
- Scope: near-full parity for user-facing management features
- Live transport: Tailscale
- Trust model: any Tailnet peer can connect only after app-level pairing
- Push model: Looper Rust relay on the Codex app server
- Pairing model: custom QR flow

## Finishing criteria

This design is complete when implementation can proceed without needing further product decisions for:

- app structure
- pairing
- push delivery
- transport split
- settings surface
- session controls
- error handling expectations
- testing expectations

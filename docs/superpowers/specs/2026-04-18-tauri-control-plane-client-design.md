# Tauri Control Plane Client Design

Date: 2026-04-18
Status: Proposed

## Summary

Build a clean-room desktop client around the existing Loopndroll workflow, but move the product onto a new architecture:

- a Tauri desktop app for the operator experience
- a local Rust supervision service as the execution-side source of truth
- a small cloud control plane for identity, billing, trusted devices, push routing, and remote command relay
- an iPhone companion app as a trusted tail-end device

The new product should preserve the useful workflow shape of the current app:

- see active and historical Codex-supervised sessions
- install and inspect hooks
- configure continue prompts, completion checks, and notifications
- receive notifications when hooks fire
- steer or stop work remotely

It should not reproduce the current UI expression, layout styling, or other unlicensed visual details. This is a workflow-compatible rebuild, not a visual clone.

## Goals

- Preserve the current operator workflow while rebuilding the product on a Rust-first foundation.
- Keep Codex session truth local to the machine where Codex is running.
- Support paid accounts without turning the backend into a copy of user work.
- Treat iPhone as a first-class trusted companion device.
- Support passkey, Apple, Google, and email-based account access under one user identity.
- Use Dodo Payments for checkout, subscription lifecycle, and customer self-service.
- Deliver hook notifications to desktop and iPhone reliably.
- Allow a narrow remote action set from companion devices:
  - `continue`
  - `stop`
  - `steer`
  - `set mode`

## Non-goals

- Storing full Codex transcripts in the cloud by default.
- Building a full cloud session-sync platform in v1.
- Making Telegram the root identity system.
- Requiring Tailscale for the core product flow.
- Reproducing the previous client UI closely enough to raise copying concerns.
- Turning the iPhone app into a full remote Codex client in v1.

## Product Principles

### Local execution truth

The machine running Codex remains the execution source of truth. It owns:

- session registration
- hook events
- stop decisions
- completion-check execution
- local history
- command results

### Small cloud layer

The cloud exists to coordinate product features that cannot stay purely local:

- identity
- billing
- device registration
- push routing
- remote command delivery
- lightweight telemetry

The cloud should know that work is happening, not contain the work itself.

### Trusted device model

Users have one account and many trusted devices. A Mac running Codex and an iPhone companion are both device identities under the same user account.

### Workflow parity, not visual copying

The rebuild should preserve functional parity where it matters, but intentionally establish a distinct visual system and component language.

## Recommended Architecture

Three viable product shapes were considered:

1. pure local-first
2. cloud control plane
3. full sync SaaS

The recommended choice is **cloud control plane**.

This gives the product:

- paid accounts
- trusted device linking
- mobile notifications
- remote actions
- multi-device account access

without forcing full transcript storage or a cloud session mirror.

## System Overview

### 1. Tauri desktop client

The desktop app is the main operator surface. It should present:

- home dashboard
- session list and detail surfaces
- session mode controls
- notification attachment controls
- completion-check assignment
- hook registration and diagnostics
- source health and event tail views

The Tauri shell should stay thin. Business logic belongs in Rust-side services, not in the web layer.

### 2. Local Rust supervision service

The local Rust service runs alongside the desktop app and owns:

- Codex diagnostics ingestion
- hook registration and inspection
- session state derivation
- completion-check execution
- remote command execution
- local notification preparation
- local persistence

This service is the local source of truth for operational state.

### 3. Cloud control plane

The cloud service owns:

- user accounts
- identity providers and linked login methods
- trusted device registry
- Dodo billing state and entitlements
- push notification fanout
- remote command queue and delivery tracking
- privacy settings
- coarse telemetry

The cloud should not require raw transcripts to operate.

### 4. iPhone companion

The iPhone app is a trusted secondary device. It should:

- receive hook and session notifications
- display session references and coarse status
- show which desktop is active
- issue simple remote actions
- reflect command delivery state

It should not become the source of truth for session content.

## Identity Model

### User identity

Each user has one internal account record.

Supported sign-in and linking methods:

- passkey
- Sign in with Apple
- Sign in with Google
- email login or recovery

All of these attach to the same account instead of creating separate user records.

### Email requirement

Email should be present for receipts, billing communication, and recovery.

If a user signs in with Apple or Google and the returned identity does not provide a usable long-term contact email, the product should collect one during onboarding before subscription-dependent features are enabled.

### Device identity

Each trusted device should have:

- device ID
- platform
- device public key or equivalent credential
- push token when relevant
- registration timestamp
- last-seen timestamp
- trust state

Devices belong to the account, not the other way around.

## Billing Model

Billing uses **Dodo Payments**.

### Checkout

Use Dodo hosted checkout for plan purchase and upgrade flows.

### Customer self-service

Use Dodo hosted customer portal for:

- payment method updates
- invoice access
- subscription management
- cancellation

### Source of truth

Dodo webhooks are the billing source of truth. The app success screen is not authoritative.

The cloud control plane should update entitlements only after verified Dodo webhook events are received and accepted.

### Minimal billing storage

The cloud database should store:

- internal user ID
- Dodo customer ID
- Dodo subscription ID
- plan
- subscription status
- renewal state
- billing timestamps

It should not store Codex transcripts because billing exists.

## Storage Boundary

### Local-only by default

The following data stays local unless the user explicitly opts into a future backup feature:

- raw Codex sessions
- prompts and replies
- hook payloads
- completion-check command output
- local event timeline
- local history cache

### Cloud-held by default

The cloud stores:

- account identity
- login method links
- device registry
- Dodo entitlement state
- notification routing data
- remote command envelopes
- privacy settings
- coarse usage telemetry

### Optional future backup

If history backup is added later, it should be opt-in and limited in scope. A future backup feature should default to encrypted summaries or snapshots rather than always-on raw transcript sync.

## Event and Command Flow

### Desktop to cloud flow

When something important happens on the desktop, the local Rust service creates:

1. a local state update for the desktop UI
2. a small cloud event envelope for routing and companion-device awareness

The cloud event envelope may contain:

- account ID
- device ID
- session reference
- event type
- coarse mode
- timestamp
- coarse status
- short redacted summary when needed

It should not contain full transcripts or command output by default.

### iPhone to desktop flow

When the iPhone issues a remote action, the flow is:

1. iPhone sends a signed request to the cloud control plane
2. cloud authorizes the request against account and device state
3. cloud delivers a command envelope to the active desktop
4. desktop acknowledges receipt
5. desktop executes locally
6. desktop reports outcome back to the cloud
7. iPhone receives updated command state

Allowed v1 commands:

- `continue`
- `stop`
- `steer`
- `set mode`

### Networking posture

Core product flow should work without Tailscale.

If Tailscale is available, a future direct path can be added for faster delivery between trusted devices. That is an optimization, not a prerequisite.

## Desktop UX Scope

The first Tauri client should cover:

- home dashboard
- session list
- global mode controls
- per-session mode controls
- notification configuration
- completion-check configuration
- hook installation and health
- source diagnostics
- live event tail

The UX should intentionally diverge visually from the current app while preserving operator muscle memory around workflow.

## iPhone Companion Scope

The iPhone app should support:

- account sign-in
- trusted-device registration
- push notifications for hook and session events
- active desktop visibility
- remote actions
- recent notification history
- coarse diagnostics for delivery failures

It should not aim for:

- full transcript browsing
- detailed local command output browsing
- full configuration parity with desktop in v1

## Cloud Data Model

### Required entities

- `users`
- `auth_identities`
- `passkey_credentials`
- `devices`
- `device_sessions`
- `subscriptions`
- `notification_endpoints`
- `remote_commands`
- `remote_command_attempts`
- `privacy_settings`
- `usage_events`

### Suggested meanings

- `users`: canonical product account
- `auth_identities`: Apple, Google, email, and similar external login links
- `passkey_credentials`: public-key credentials for WebAuthn or native passkey flows
- `devices`: trusted hardware endpoints
- `device_sessions`: currently active device auth sessions
- `subscriptions`: Dodo-backed entitlement records
- `notification_endpoints`: APNs and similar routing handles
- `remote_commands`: normalized user intent envelopes
- `remote_command_attempts`: delivery and execution trace without transcript storage
- `privacy_settings`: telemetry and future backup choices
- `usage_events`: coarse product analytics only

## Security and Privacy

### Authentication

- Prefer passkeys as the first-class sign-in method.
- Use system-browser flows for desktop OAuth-based sign-in.
- Use native Apple sign-in on iPhone.
- Verify all third-party identity tokens server-side.

### Device trust

- Every device must be explicitly registered.
- Remote actions require an authenticated user session and a trusted device.
- Sensitive account actions should support step-up auth when needed.

### Billing

- Verify Dodo webhooks before updating entitlements.
- Separate entitlement reads from payment mutation logic.

### Data minimization

- Do not upload raw transcripts by default.
- Do not upload completion-check output by default.
- Do not retain more notification summary text than necessary for the companion experience.

## Failure Handling

### Desktop offline

If the desktop is offline:

- hook handling continues locally
- local UI remains accurate
- cloud-directed commands remain queued or expire based on policy
- iPhone shows delivery state, not fake success

### Cloud unavailable

If the cloud is unavailable:

- local Codex supervision still functions
- desktop continues to manage hooks and completion checks
- billing refresh and remote companion features degrade gracefully

### Dodo webhook delay

If billing webhooks are delayed:

- the system should avoid prematurely granting or revoking access
- entitlement transitions should be driven by verified webhook processing
- temporary pending states should be explicit in the account model

### Push delivery failure

If push fails:

- retry routing where appropriate
- keep a recent event record in the cloud for in-app fetch on next open
- expose delivery diagnostics in the iPhone companion

## Testing Strategy

### Local service

- unit tests for session normalization and command policy
- integration tests for hook handling and completion-check loops
- API tests for diagnostics and remote command endpoints

### Cloud control plane

- auth flow tests for passkey, Apple, Google, and email linkage
- webhook verification tests for Dodo events
- entitlement transition tests
- device registration and trust tests
- remote command authorization tests

### Desktop client

- UI tests for session controls and settings
- integration tests against a mocked local Rust service
- manual validation of hook installation and diagnostics views

### iPhone companion

- device registration tests
- push handling tests
- remote action flow tests
- degraded-state handling tests when the desktop is unreachable

## Rollout Plan

### Phase 1

- local Rust supervision service
- Tauri desktop shell
- workflow-compatible desktop controls
- local diagnostics and event tail

### Phase 2

- cloud account system
- passkey support
- Apple, Google, and email linking
- Dodo checkout and webhook-driven entitlements

### Phase 3

- iPhone trusted-device pairing
- push notifications
- remote action relay

### Phase 4

- Tailscale-assisted direct delivery as an optimization
- optional encrypted backup or summary sync if product demand justifies it

## Open Decisions Resolved In This Spec

- Architecture: cloud control plane, not full sync SaaS
- Desktop shell: Tauri
- Execution truth: local desktop
- iPhone role: trusted companion with simple remote actions
- Identity: first-party account with passkey, Apple, Google, and email
- Billing: Dodo Payments
- Telegram: optional linked channel later, not the root identity
- Session storage: local-first, not cloud-mirrored by default

## Finish Line For Planning

Planning should treat this project as a sequence of subprojects:

1. local Rust supervision service and Tauri desktop surface
2. cloud identity and billing control plane
3. iPhone trusted-device companion

Each phase should preserve the local-first storage boundary defined here.

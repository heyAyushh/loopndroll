# Rust Codex Diagnostics Agent Design

Date: 2026-04-18
Status: Proposed

## Summary

Build a standalone Rust diagnostics agent that reads Codex state directly, normalizes Codex thread and rollout activity into a single event timeline, and attributes each session to its caller. This is the first subproject in the broader migration away from the current Bun-based Looper backend.

The agent is read-only in v1. It does not mutate Codex threads, replace the desktop app, or ship iPhone pairing and notification flows yet. Its job is to give the project a trustworthy event spine and enough attribution data to understand what is running, who started it, and whether it is a main thread or a sub-agent.

## Goals

- Read Codex diagnostics directly without depending on the current Looper Bun backend.
- Build one normalized event model across Codex SQLite state, Codex log rows, and Codex rollout JSONL files.
- Identify the current Codex app-server runtime, active threads, and per-thread activity.
- Attribute each thread to:
  - root thread
  - parent thread when applicable
  - main-thread versus sub-agent launch kind
  - caller category
  - caller process details when they can be captured reliably
- Expose the diagnostics data through a local API and a local CLI.
- Capture new thread starts quickly enough that caller attribution is still meaningful.

## Non-goals

- Sending control commands back into Codex in v1.
- Replacing the existing Electrobun desktop shell in v1.
- Replacing iPhone pairing, APNs delivery, or Tailscale flows in v1.
- Writing into Codex SQLite databases or Codex rollout files.
- Depending on Codex hooks as the primary source of truth.
- Reconstructing every historical caller process with exact certainty after the fact.

## Why This Comes First

The current repo only sees a narrow slice of Codex behavior through three hooks: `SessionStart`, `Stop`, and `UserPromptSubmit`. Codex itself already stores much richer state and event data under `~/.codex/`, including thread records, thread spawn edges, log rows, and rollout files. The first Rust deliverable should consume that richer surface directly before any control-plane rewrite begins.

Without this agent, a later Rust server would still be guessing about:

- what Codex actually emits
- which events matter
- how sub-agents are represented
- which runtime details are durable versus ephemeral
- how to attribute a thread to the thing that launched it

## Codex Sources

The Rust agent should discover Codex runtime files dynamically instead of hardcoding a single versioned filename.

### Required inputs

- `~/.codex/state_*.sqlite`
- `~/.codex/logs_*.sqlite`
- `~/.codex/sessions/**/*.jsonl`

### Important state tables

- `threads`
- `thread_spawn_edges`
- `thread_dynamic_tools`
- `remote_control_enrollments`
- `stage1_outputs`

### Important log store

- `logs`

### Important rollout file records

- `session_meta`
- `turn_context`
- `event_msg`
- `response_item`

The agent should treat Codex files as append-only diagnostic inputs and never assume an internal schema is stable forever. Every reader needs explicit version-tolerant parsing and graceful degradation when fields are missing.

## Product Shape

The first Rust deliverable consists of four parts.

### 1. Diagnostics daemon

A local Rust process runs on the Mac and owns Codex diagnostics ingestion.

Responsibilities:

- discover Codex runtime files
- snapshot existing state on startup
- watch for new rows and new rollout appends
- normalize raw Codex artifacts into a local event store
- compute thread attribution
- answer local API and CLI queries

### 2. Local event store

The daemon keeps its own read-only derived store so the UI does not need to re-scan Codex artifacts on every query.

This store should be SQLite-backed for v1 because:

- the input sources are already SQLite and JSONL
- local queryability matters more than horizontal scale
- it keeps deployment and recovery simple

### 3. Local API

The daemon exposes a localhost-only HTTP API for local clients.

Recommended v1 transport:

- HTTP JSON for point-in-time reads
- server-sent events for live tailing

This is faster to ship than local gRPC and still keeps the domain model transport-independent. The internal service boundary should stay clean enough that a later gRPC surface can be added without rewriting normalization logic.

### 4. Local CLI

A small Rust CLI talks to the daemon for debugging and scripted inspection.

Examples:

- `codex-diagnostics sources`
- `codex-diagnostics threads list`
- `codex-diagnostics threads inspect <thread-id>`
- `codex-diagnostics events tail`
- `codex-diagnostics attribution <thread-id>`

## Architecture

### Component breakdown

#### Source discovery

Find the newest matching Codex files under `~/.codex/`, verify they are readable, and expose a source health summary:

- current state DB path
- current logs DB path
- sessions root path
- Codex app-server process pid when found
- whether live watching is enabled

#### Snapshot readers

Separate readers ingest each source:

- `state_reader` for `threads`, `thread_spawn_edges`, and related metadata
- `logs_reader` for `logs`
- `rollout_reader` for JSONL rollout files

Each reader emits raw source-specific records into a normalizer boundary instead of leaking source-specific shapes further into the system.

#### Live watchers

The daemon watches for:

- new or updated thread rows
- new or updated log rows
- new rollout files
- appended rollout lines
- Codex app-server process appearance and disappearance

This is required for useful live attribution. Historical backfill alone is not enough.

#### Normalizer

The normalizer maps raw source records into canonical domain objects:

- `CodexThreadRecord`
- `CodexTurnRecord`
- `NormalizedEvent`
- `AttributionRecord`
- `ProcessObservation`

#### Attribution engine

This component derives:

- root thread id
- parent thread id
- launch kind
- caller category
- caller process details
- confidence level

#### Query service

This component serves:

- thread list
- thread detail
- event timeline
- attribution detail
- source health
- runtime diagnostics

## Canonical Data Model

### Thread record

Each thread entry should include:

- `thread_id`
- `title`
- `cwd`
- `source`
- `model`
- `reasoning_effort`
- `created_at`
- `updated_at`
- `archived`
- `rollout_path`

### Normalized event

Each event should include:

- `event_id`
- `thread_id`
- `turn_id` when available
- `occurred_at`
- `source_kind`: `state`, `logs`, or `rollout`
- `event_category`
- `event_type`
- `raw_offset` or source cursor
- `payload_json`

### Attribution record

Each thread should have one current attribution record:

- `thread_id`
- `root_thread_id`
- `parent_thread_id`
- `launch_kind`
- `caller_category`
- `caller_thread_id`
- `caller_process_pid`
- `caller_process_executable`
- `caller_process_command`
- `caller_process_tty`
- `raw_source`
- `raw_originator`
- `attribution_confidence`
- `derived_at`

### Runtime health

The daemon should also expose:

- Codex app-server pid when found
- Codex app-server binary path
- active source paths
- last successful ingest times
- last ingest error per source
- lag between source append and normalized-store write

## Event Taxonomy

The v1 normalized taxonomy should be explicit and narrow. Do not store only opaque blobs.

### Thread lifecycle

- `thread.started`
- `thread.status_changed`
- `thread.archived`
- `thread.unarchived`
- `thread.closed`
- `thread.name_updated`
- `thread.compacted`

### Turn lifecycle

- `turn.started`
- `turn.completed`
- `turn.plan_updated`
- `turn.diff_updated`

### Messages and reasoning

- `message.user`
- `message.agent`
- `reasoning.delta`
- `reasoning.summary_delta`

### Tooling and execution

- `tool.function_call.started`
- `tool.function_call.completed`
- `tool.mcp.progress`
- `command.output_delta`
- `command.terminal_interaction`
- `file_change.output_delta`

### Hooks and diagnostics

- `hook.started`
- `hook.completed`
- `token_usage.updated`
- `dynamic_tool.request`
- `dynamic_tool.response`
- `web_search.completed`

### Realtime

- `realtime.started`
- `realtime.item_added`
- `realtime.transcript_delta`
- `realtime.transcript_done`
- `realtime.output_audio_delta`
- `realtime.sdp`
- `realtime.error`
- `realtime.closed`

If a raw Codex event does not fit the initial taxonomy, the daemon should still store it as:

- `event.unknown`

with the original raw type preserved in `event_type`.

## Session Attribution Design

### Launch kind

Each thread must be marked as one of:

- `main`
- `subagent`

Rule:

- if the thread appears as a child in `thread_spawn_edges`, it is a `subagent`
- otherwise it is a `main` thread unless later evidence proves otherwise

### Root and parent thread

The attribution engine walks `thread_spawn_edges` to compute:

- direct `parent_thread_id`
- transitive `root_thread_id`

This should be cached in the derived store so UI queries do not need recursive traversal at read time.

### Caller category

Each thread should be categorized as:

- `user`
- `parent_agent`
- `ide`
- `cli`
- `desktop_app`
- `remote_control`
- `service`
- `unknown`

### Category derivation rules

Apply rules in priority order:

1. If there is a `parent_thread_id`, category is `parent_agent`.
2. If the thread start is associated with a remote-control enrollment, category is `remote_control`.
3. If thread metadata or rollout metadata identifies a known desktop originator, category is `desktop_app`.
4. If the thread `source` indicates IDE launch, category is `ide`.
5. If the thread `source` indicates shell execution, category is `cli`.
6. If the thread is later launched by a hosted control plane, category is `service`.
7. Otherwise category is `unknown`.

The raw fields that informed the decision must be stored alongside the derived category.

### Caller process capture

This is required in v1, but it is not always historically recoverable.

The daemon should maintain a live process observer that captures a best-effort snapshot near thread creation time:

- pid
- parent pid
- executable path
- argv
- tty
- start time

This snapshot is attached to the thread when there is enough temporal evidence to correlate it with the thread start.

### Confidence levels

Every attribution decision needs a confidence label:

- `exact`
- `high`
- `best_effort`
- `unknown`

Use:

- `exact` when live process observation and Codex metadata agree on the caller
- `high` when parent-thread or source/originator data is sufficient without process capture
- `best_effort` when attribution is inferred from partial evidence
- `unknown` when the evidence is not defensible

### Important limitation

The daemon must surface a clear limitation in the API and CLI:

Exact OS-process attribution is usually only possible for sessions observed live. Historical threads may still have strong thread ancestry and caller-category data, but the original launcher process may no longer be knowable.

## API Surface

### Required endpoints

- `GET /health`
- `GET /sources`
- `GET /threads`
- `GET /threads/:thread_id`
- `GET /threads/:thread_id/events`
- `GET /threads/:thread_id/attribution`
- `GET /events/tail`

### Response behavior

- `GET /threads` returns recent threads with attribution summary fields inline.
- `GET /threads/:thread_id` returns the merged thread record plus source-specific raw facts.
- `GET /threads/:thread_id/events` returns canonical normalized events ordered by timestamp and source cursor.
- `GET /threads/:thread_id/attribution` returns the derived attribution record plus evidence used.
- `GET /events/tail` streams normalized events over SSE.

### API security

The v1 agent listens on loopback only.

- bind to `127.0.0.1`
- no public listener
- no Tailscale exposure yet
- no write endpoints yet

## CLI Surface

### Required commands

- `codex-diagnostics health`
- `codex-diagnostics sources`
- `codex-diagnostics threads list`
- `codex-diagnostics threads inspect <thread-id>`
- `codex-diagnostics threads events <thread-id>`
- `codex-diagnostics attribution <thread-id>`
- `codex-diagnostics tail`

The CLI should prefer the daemon API when the daemon is available and fall back to direct local reads only for diagnostics and development.

## Failure Handling

### Missing files

If a Codex source file is missing:

- mark the source as degraded
- keep serving whatever other sources are available
- do not crash the daemon

### Schema drift

If a Codex table or JSONL field changes:

- preserve unknown fields in raw payload storage
- downgrade only the affected normalization branch
- report the mismatch in source health

### Partial corruption

If a rollout line or log row cannot be parsed:

- store a parse-error diagnostic event
- continue ingestion from later records

### Codex app-server not running

If Codex app-server is not currently running:

- keep historical reads available
- mark live process observation as unavailable
- keep watcher loops alive for when Codex starts later

## Testing

### Unit tests

- source discovery with multiple versioned SQLite filenames
- rollout JSONL parsing
- normalized taxonomy mapping
- attribution rule ordering
- confidence scoring

### Fixture tests

Use captured fixture sets for:

- single main thread
- parent thread plus sub-agent children
- remote-control metadata present
- missing originator metadata
- mixed historical backfill and live updates

### Integration tests

- start the daemon against fixture Codex directories
- verify `GET /threads`
- verify `GET /threads/:thread_id/events`
- verify `GET /threads/:thread_id/attribution`
- append rollout lines and confirm live tail updates

### Manual verification

On a real machine with Codex Desktop:

- confirm the daemon finds the active Codex app-server
- confirm it discovers current threads from `~/.codex/state_*.sqlite`
- confirm it tails a live rollout file
- confirm it marks a spawned child thread as `subagent`
- confirm it reports caller category and confidence without claiming false certainty

## Rollout

### Phase 1

Ship the daemon and CLI in read-only mode with:

- source discovery
- snapshot reads
- rollout tailing
- normalized event timeline
- session attribution

### Phase 2

Wire the existing desktop UI to the Rust local API for diagnostics views.

### Phase 3

Add control-plane mutations and service sync only after the diagnostics model has proven stable.

## Open Decisions Already Resolved

- This first Rust deliverable is a diagnostics agent, not the whole platform rewrite.
- The hosted service is not part of this first subproject.
- The agent reads Codex directly rather than depending on Bun hooks.
- Session attribution is a first-class requirement in v1.
- The local API is loopback-only and read-only in v1.

## Success Criteria

The first Rust diagnostics agent is done when it can:

- identify the active Codex runtime and source files
- list current and recent Codex threads
- show a normalized event timeline for a thread
- tell whether a thread is a main thread or a sub-agent
- compute root and parent thread relationships
- categorize who called the thread with a confidence label
- show caller process details when they were captured live
- keep working even when some Codex data sources are missing or partially malformed

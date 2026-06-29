# Task 4 Code Review

## Scope

- Reviewed `crates/looper-client-core/src/client.rs` only for the rejected Todo 4 replacement path.
- No iOS, macOS, server, generated, or game files were changed.
- `docs/architecture/decisions.md` ADR-004 keeps client-core as the shared state/reconcile owner; this repair stays in Rust client-core.

## Monotonic Per-Node Rules

- `replace=true` deltas may contain sections for more than one node.
- A replacement section is fresh only when that node's incoming replacement mini seq is greater than the `last_seq_by_node` cursor captured when the replacement was staged.
- Finality clears and upserts only fresh node sections.
- Stale node sections in a mixed replacement do not delete existing local minis and do not advance that node cursor.
- Empty replacement batches still use the replacement delta seq as the default-node clear cursor.

## Stale Snapshot Guard

- Recovery and local snapshots still merge through the snapshot preservation path.
- Snapshot merge keeps a local mini when its seq is newer than the incoming node partition.
- Stream replacement finality now mirrors that guard instead of clearing every covered node before stale filtering.

## `last_seq_by_node` And Freshness

- `last_seq_by_node` remains the monotonic cursor source for per-node stale detection.
- Pending replacements carry the staged cursor snapshot, so later ACKs do not invalidate already-staged fresh replacement chunks.
- Fresh replacement sections advance only their own node cursor.
- A mixed batch can advance node B while preserving node A's newer mini and cursor.
- Heartbeat liveness remains separate from mini payload freshness.

## Slop And Overfit Review

- No plausible-but-false state: replacement finality is per covered node, so a stale section cannot erase a newer local mini just because another node in the batch is fresh.
- No overfitted test-only logic: freshness flows through `last_seq_by_node` and the staged pending-replacement cursor snapshot, not a branch keyed to one test shape.
- Monotonic per-node invariant: a node cursor advances only from a fresh section for that node; stale sections preserve both the existing mini and cursor.
- `last_seq_by_node` remains the stale detector; the replacement delta seq and heartbeat liveness do not prove per-node payload freshness.
- The stale snapshot guard stays aligned across recovery snapshots, local snapshots, and stream replacement finality.
- No broad UI/generated/game touches: the Todo 4 product diff is confined to Rust client-core, and this repair touches only Task 4 evidence files.

## Review Result

- The rejected all-or-nothing replacement behavior is repaired in client-core.
- Focused adversarial coverage is in `state_mini_replacement_delta_preserves_newer_node_in_mixed_batch`.

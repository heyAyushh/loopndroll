# Task 4 SessionMini Truth

## Fields

Client-core now normalizes every stored/applied `ClientStateMini` payload with the home/menu card truth fields:

- `accountId`
- `nodeId`
- `sessionId` and `id`
- `assistantSurface`
- `effectiveMode`
- `replyable` and `canSendPrompt`
- `blockedGoal`
- `queueCount`
- `lifecycle`
- `notificationStatus`
- `freshnessSource`
- `routeEndpoint`
- `revision`

Missing node and account values deterministically fall back to `local-node` and `local-account`. Missing revisions fall back to `mini:<seq>`. Missing notification state is marked `known=false` instead of being shown as a false "off" state. Missing session IDs still reject through `ClientCoreError::EmptySessionId`.

## Monotonic Rules

- Client-core tracks node cursors internally as `last_seq_by_node`.
- The durable JSON store persists those cursors as `lastSeqByNode`.
- Stream deltas are stale only for the node partition they claim, not because another node has a higher aggregate `latest_seq`.
- `replace=true` state-mini deltas stage until heartbeat finality, then replace only the covered node partition.
- Recovery snapshots replace only node partitions they prove. If local state has a newer mini for that node, it is preserved.

## Stale Snapshot Guard

HTTP recovery snapshots can advance aggregate `latest_seq`, but they cannot rewind UI-visible minis from a node that already has a newer stream-applied mini. Missing or stale node partitions are preserved, so the list does not collapse during recovery stalls.

## Freshness and Liveness

`freshnessSource` belongs to the mini payload and is updated by stream/recovery/local projection writes. Heartbeats update route liveness (`endpoint_url`, heartbeat `latest_seq`, server time) without rewriting mini freshness or advancing the projection cursor.

## Tests and Artifacts

- `task-4-client-core-state-mini.txt`: `cargo test --manifest-path crates/looper-client-core/Cargo.toml state_mini -- --nocapture`
- `task-4-last-seq-by-node.txt`: `cargo test --manifest-path crates/looper-client-core/Cargo.toml last_seq_by_node -- --nocapture`
- `task-4-freshness-liveness.txt`: `cargo test --manifest-path crates/looper-client-core/Cargo.toml freshness -- --nocapture`
- `task-4-cargo-fmt-check.txt`: `cargo fmt --manifest-path crates/looper-client-core/Cargo.toml --check`
- `task-4-git-diff-check.txt`: `git diff --check`

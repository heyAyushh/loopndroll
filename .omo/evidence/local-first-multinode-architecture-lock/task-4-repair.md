# Task 4 Repair

## Reviewer Blocker

`replace=true` state-mini batches were staged as one replacement. At heartbeat finality, client-core removed every covered node before upserting incoming replacement minis. A mixed batch could therefore delete or rewind a newer local node mini while correctly updating another node.

## Repair

- Compute replacement freshness per covered node at finality.
- Compare each node's incoming replacement mini seq against the `last_seq_by_node` snapshot captured when the replacement was staged.
- Retain stale node partitions untouched.
- Clear/upsert only fresh node partitions.
- Advance only fresh node cursors.
- Preserve the existing continuation behavior where an interleaved ACK does not invalidate an already-staged replacement.

## Mixed-Node Preservation Proof

Added adversarial test:

- Local node A: seq 30, title `newer a`.
- Local node B: seq 10, title `old b`.
- Incoming replacement:
  - node A: seq 20, title `stale a`.
  - node B: seq 31, title `fresh b`.

Expected and covered result:

- node A remains `newer a`.
- node B becomes `fresh b`.
- `last_seq_by_node["node-a"]` remains 30.
- `last_seq_by_node["node-b"]` advances to 31.

## Verification Artifacts

- `task-4-state-mini-rerun.txt`
- `task-4-last-seq-rerun.txt`
- `task-4-cargo-fmt-check-rerun.txt`
- `task-4-git-diff-check-rerun.txt`

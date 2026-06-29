# Task 5 Data Plane

## Route

- `GET /api/mobile/sessions/:thread_id/content`
- Auth: existing mobile bearer token path.
- Implemented ranges: `range=tail` and `range=after&cursor=after:<revision>:<offset>`.
- Reserved but explicitly rejected in this cut: `range=search`, `before`, and blob/detail-specific ranges return `unsupported_range`.

## Bounds

- Default chunk size: 64 KiB.
- Maximum chunk size: 512 KiB.
- `limit=0` or `limit>524288` returns `400 invalid_limit`.
- Stale explicit `revision` or stale cursor revision returns `409 stale_revision`.
- Malformed cursors return `400 invalid_cursor`.

## Metadata

Each response chunk carries:

- `account_id`
- `node_id`
- `session_id`
- `revision`
- `offset`
- `length`
- `sha256`
- `next_cursor`
- `merkle_root`
- `merkle_proof`

`merkle_root` and `merkle_proof` are `null` placeholders in this cut. The chunk `sha256` covers exactly the returned byte range.

## Control Plane Boundary

The `Session` stream still carries compact control/state frames only. Transcript bytes are fetched through this authenticated HTTP data-plane route, and `SessionMini`/home/card projections do not call or wait on this route. The only gRPC edits in this worktree are compile unblocks for existing compact ACK metadata fields; no transcript/log/detail content is added to Session frames.

## Evidence

- Focused tests: `.omo/evidence/local-first-multinode-architecture-lock/task-5-content-tests.txt`
- Manual curl body: `.omo/evidence/local-first-multinode-architecture-lock/task-5-tail.json`
- Manual curl status/metadata check: `.omo/evidence/local-first-multinode-architecture-lock/task-5-tail-curl.txt`
- Fmt check: `.omo/evidence/local-first-multinode-architecture-lock/task-5-fmt-check.txt`
- Diff check: `.omo/evidence/local-first-multinode-architecture-lock/task-5-git-diff-check.txt`
- Post-work dirty status: `.omo/evidence/local-first-multinode-architecture-lock/task-5-post-status.txt`

## Adversarial Classes

- `stale_state`: covered by `mobile_session_content_rejects_stale_revision`; stale `revision` returns `409 stale_revision`.
- `malformed_input`: covered by `mobile_session_content_rejects_malformed_input`; invalid cursor, oversized limit, and unsupported range return clean `400` errors.
- `dirty_worktree`: pre-work status was already dirty with many unrelated `.omo` artifacts; post-work status captured in `task-5-post-status.txt`; staging is restricted to task-5 files.
- `misleading_success_output`: exact test output, curl status, and metadata assertion output are saved under the evidence paths above.
- `hung_or_long_commands`: curl uses `--max-time 2`; tests are filtered to `mobile_session_content`.
- `other`: iOS/macOS installs, Pinball/Maze surfaces, and generated files are not applicable to this server-only content slice.

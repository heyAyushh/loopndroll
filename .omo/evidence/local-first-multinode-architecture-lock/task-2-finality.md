# Task 2 finality certificates

## Surface

Hot Session-stream command ACKs now carry compact finality metadata:

- `accepted`
- `account_id`
- `node_id`
- `client_mutation_id`
- `entity_id`
- `ack_seq`
- `revision`
- `server_time`
- stable reject fields: `error_code`, `reject_reason`, `current_state`

Accepted ACK means the owner node durably recorded the command ACK into `mobile_command_log` plus `mobile_state_event_log`. It does not mean prompt delivery or agent pickup completed.

## Command coverage

- Mode: `SetSessionMode`
- Prompt: `SendSessionPrompt`
- Notification reply: `SubmitNotificationReply`
- Siri: `SetSiriCurrentSession`, `SetSiriDefaultSession`
- Session state: `SetSessionArchived`, `DeleteSession`, `MuteSession`
- Settings/routes/checks: `SaveDefaultPrompt`, `SetGlobalPreset`, `SetGlobalNotification`, `SetDefaultNotificationTargets`, `SetScope`, `SetGlobalCompletionCheck`, `UpsertNotificationRoute`, `SetSessionNotifications`, `SetSessionCompletionCheck`

## Proof

- Red seam: `.omo/evidence/local-first-multinode-architecture-lock/task-2-red-illegal-fsm.txt`, exit non-zero before production edits.
- Illegal-FSM typed reject: `.omo/evidence/local-first-multinode-architecture-lock/task-2-illegal-fsm.txt`, exit 0.
- Focused requested gate: `.omo/evidence/local-first-multinode-architecture-lock/task-2-mobile-events.txt`, exit 0.
- Client-core ACK mapping sanity: `.omo/evidence/local-first-multinode-architecture-lock/task-2-client-command-ack.txt`, exit 0.
- Required fmt check: `.omo/evidence/local-first-multinode-architecture-lock/task-2-cargo-fmt-check.txt`, exit 0 with no stdout.
- Task-file rustfmt check: `.omo/evidence/local-first-multinode-architecture-lock/task-2-rustfmt-task-files.txt`, exit 0 with no stdout.
- Whitespace gate: `.omo/evidence/local-first-multinode-architecture-lock/task-2-git-diff-check.txt`, exit 0 with no stdout.
- No snapshot rebuild in ACK helper path: `.omo/evidence/local-first-multinode-architecture-lock/task-2-no-snapshot-ack-scan.txt`
- No SSE/HTTP hot command revival in touched server files: `.omo/evidence/local-first-multinode-architecture-lock/task-2-no-hot-http-sse-scan.txt`

## Adversarial classes

- `stale_state`: PASS. Touched command ACK helpers do not call `desktop_mobile_snapshot()`.
- `dirty_worktree`: PASS with scope caveat. Pre-edit status had no tracked product modifications and many unrelated untracked files. Post-commit status has no tracked changes; unrelated untracked files remain untouched.
- `misleading_success_output`: PASS. Focused test output was captured through `tee`; exit status was checked by `set -o pipefail`.
- `malformed_input`: PASS. Illegal prompt without mode returns rejected ACK with stable `mode_required`, reason containing `current_state=idle`, and `current_state=idle`.
- `hung_or_long_commands`: PASS. Focused gates used only Rust test filters; no command exceeded expectations.
- Other adversarial classes: not applicable to this CLI/data-shaped server boundary; no installs, device work, external API export, or destructive operations were used.

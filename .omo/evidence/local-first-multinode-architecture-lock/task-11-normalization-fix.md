# Task 11 normalization fix

Date: 2026-06-30
Repo: `/Users/ay/Documents/looper`
Scope: Rust client-core local store normalization only.

## Sample root cause

- Source artifact: `.omo/evidence/local-first-multinode-architecture-lock/task-11-menubar.sample.txt`
- Installed app path in sample: `/Applications/looper.app/Contents/MacOS/LooperMenuBar`
- Hot stack: `LooperClientCoreSessionRuntime.new` -> `LooperClientCoreLocalStore::new` -> `normalize_state_minis_for_source`
- The sample spent the busy main-thread startup path in `normalize_state_minis_for_source`, then `same_state_mini_key`, `state_mini_node_id`, `payload_object`, and `serde_json` parsing.
- Root cause: normalization deduped minis by linearly scanning already-normalized minis for every input mini. Each comparison reparsed JSON to recover account/node identity, so a large cache with many unique or stale minis becomes quadratic and allocation-heavy before the menu bar can start the bundled server.

## Changed files

- `crates/looper-client-core/src/state_mini.rs`
  - Added `StateMiniKey` and cached sort keys.
  - Replaced the nested `Vec::position` dedupe in `normalize_state_minis_for_source` with a `HashMap<StateMiniKey, usize>` index.
  - Kept deterministic output ordering with `sort_by_cached_key`, so account/node JSON is parsed once per surviving mini for sort-key generation instead of inside every comparator call.
- `crates/looper-client-core/src/local_store.rs`
  - Replaced the same nested key scan in `merge_snapshot_minis_preserving_newer` with a one-time `HashMap<StateMiniKey, usize>` over surviving local minis.
  - Added `local_store_load_normalizes_many_stale_minis_without_quadratic_scan`, which writes 2,048 stale/fresh mini pairs, opens `LooperClientCoreLocalStore::new`, and verifies only the fresh mini per key survives and is persisted.

## Verification output

- Focused Rust test: `.omo/evidence/local-first-multinode-architecture-lock/task-11-normalization-fix-cargo-test.txt`
- Format check: `.omo/evidence/local-first-multinode-architecture-lock/task-11-normalization-fix-fmt-check.txt`
- Diff whitespace check: `.omo/evidence/local-first-multinode-architecture-lock/task-11-normalization-fix-diff-check.txt`

Commands run:

```bash
cargo test --manifest-path crates/looper-client-core/Cargo.toml local_store_load_normalizes_many_stale_minis_without_quadratic_scan -- --nocapture
cargo fmt --manifest-path crates/looper-client-core/Cargo.toml --check
git diff --check
```

## Complexity bound

- Before: per-mini dedupe scanned all prior survivors and reparsed JSON during each key comparison, so many unique or stale mini keys produced O(n^2) key checks plus repeated JSON parsing.
- After: each input mini is normalized once, keyed once, and looked up in a `HashMap`, so dedupe is expected O(n) with one stored index per distinct key.
- Deterministic output sorting remains O(k log k), where k is the survivor count and k <= n. Sort key extraction is bounded to one cached key per survivor, not repeated JSON parsing inside comparator calls.
- Local-store merge uses the same bounded key index over current survivors plus one key lookup per incoming mini; it no longer scans the survivor vector for each incoming mini.

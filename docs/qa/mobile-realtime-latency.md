# Mobile Realtime Latency QA

- Observed at: 2026-06-25 07:46:37Z
- Measurement layer: DEBUG app-internal selftest, not XCTest tap round-trip latency.
- Artifact directory: `.build/mobile-realtime-latency/runs/20260625T073929Z`
- Device platform: `physical-ios`
- DerivedData mode: `not recorded by this pre-cache run`
- DerivedData cache proof: `.omo/evidence/g010-derived-data-cache-harness-check.txt`
- Classification: `strict-pass`
- Sample count: 20
- Percentile: p95
- UI mode p95: 3 ms (target 30 ms)
- UI prompt p95: 3 ms (target 30 ms)
- Mode ACK p95: 29 ms (target 30 ms)
- Prompt ACK p95: 17 ms (target 30 ms)
- Notification persistence p95: 2 ms (target 30 ms)
- Stream apply p95: 2 ms (target 30 ms)
- Stream resume p95: 2 ms (target 30 ms)
- Snapshot-on-tap count: 0
- Full snapshot calls on action path: 0
- Remote/Tailscale ACK available in this run: False

Samples:

- Sample 1: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 2: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 3: UI mode 2 ms, UI prompt 3 ms, mode ACK 33 ms, prompt ACK 27 ms, notification persist 1 ms, stream apply 17 ms, full snapshots 0
- Sample 4: UI mode 2 ms, UI prompt 2 ms, mode ACK 29 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 5: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 6: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 7: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 8: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 9: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 14 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 10: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 14 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 11: UI mode 3 ms, UI prompt 2 ms, mode ACK 19 ms, prompt ACK 15 ms, notification persist 2 ms, stream apply 2 ms, full snapshots 0
- Sample 12: UI mode 2 ms, UI prompt 4 ms, mode ACK 15 ms, prompt ACK 17 ms, notification persist 1 ms, stream apply 2 ms, full snapshots 0
- Sample 13: UI mode 3 ms, UI prompt 2 ms, mode ACK 15 ms, prompt ACK 14 ms, notification persist 1 ms, stream apply 2 ms, full snapshots 0
- Sample 14: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 2 ms, full snapshots 0
- Sample 15: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 14 ms, notification persist 2 ms, stream apply 1 ms, full snapshots 0
- Sample 16: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 13 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 17: UI mode 2 ms, UI prompt 2 ms, mode ACK 13 ms, prompt ACK 14 ms, notification persist 1 ms, stream apply 1 ms, full snapshots 0
- Sample 18: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 14 ms, notification persist 1 ms, stream apply 2 ms, full snapshots 0
- Sample 19: UI mode 2 ms, UI prompt 2 ms, mode ACK 14 ms, prompt ACK 15 ms, notification persist 2 ms, stream apply 2 ms, full snapshots 0
- Sample 20: UI mode 2 ms, UI prompt 3 ms, mode ACK 14 ms, prompt ACK 15 ms, notification persist 2 ms, stream apply 2 ms, full snapshots 0

Command:

```bash
LOOPER_IOS_DEVICE_ID=D7749DEB-F9A8-5FD6-B33B-BF715B8B2F7C python3 scripts/run-mobile-realtime-latency-check.py --samples 20 --strict-local-first-targets --assert-budget-contract --app-selftest --physical-device
```

## Step 1 ACK-first server proof

Server/control-plane verification commands:

```bash
cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane grpc_prompt_ack_returns_before_codex_resume_delivery_completes -- --nocapture
cargo test --manifest-path crates/agent-control-plane/Cargo.toml --test isolated_control_plane mobile_events:: -- --nocapture
cargo test --manifest-path crates/agent-control-plane/Cargo.toml prompt_delivery -- --nocapture
```

Observed output:

- `grpc_prompt_ack_returns_before_codex_resume_delivery_completes`: `1 passed`; proves prompt ACK returns before the controlled 3 second Codex resume delivery finishes.
- `mobile_events::`: `14 passed`; covers ACK replay, in-flight command recovery, hot mini-cache precondition, notification reply ACK path, and state-mini replay.
- `prompt_delivery`: `16 passed`; covers delivery routing and assistant-surface cache key separation.

Physical-device artifact paths:

- JSON p95 report: `.build/mobile-realtime-latency/runs/20260625T073929Z/mobile-realtime-latency.json`
- iOS build output: `.build/mobile-realtime-latency/runs/20260625T073929Z/ios-physical-build.log`
- install output: `.build/mobile-realtime-latency/runs/20260625T073929Z/ios-physical-install.txt`
- launch/selftest output: `.build/mobile-realtime-latency/runs/20260625T073929Z/ios-physical-launch.txt`
- device info: `.build/mobile-realtime-latency/runs/20260625T073929Z/ios-physical-device-info.txt`

Relevant physical-device output:

```text
** BUILD SUCCEEDED **
App installed: bundleID: dev.looper.app.ios
Launched application with dev.looper.app.ios bundle identifier.
G006_SELFTEST_PASS c007 {"classification":"strict-pass","platform":"physical-ios","sampleCount":20,"promptAckMs":17,"modeAckMs":29,"snapshotOnTapCount":0,"fullSnapshotCallsOnTap":0}
The app terminated with the exit code 0.
```

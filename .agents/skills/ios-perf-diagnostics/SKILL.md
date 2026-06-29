---
name: ios-perf-diagnostics
description: Use Looper's iOS Simulator observability loop with oslog-live, lldb-trap, perf-loop, and ETTrace. Use when debugging iOS latency, switcher taps, hangs, crashes, UI invalidations, or performance regressions.
---

# Looper iOS Performance Diagnostics

Use this skill for Looper iOS simulator diagnosis when timing, OSLog, LLDB traps, xctrace, or ETTrace can prove the failure faster than code inspection alone.

If the bug is assistant switcher command ordering, stale async results, state-mini pending command ownership, or latest-wins behavior, use `$ios-session-sync-debugging` first. Return here only after behavior is fixed or when measured latency/hitches remain the actual problem.

## Truth source

- The iOS app product is `Looper`; bundle id is `dev.looper.app.ios`.
- Use Xcode beta for current Looper iOS proof unless the user asks otherwise:
  `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`.
- Repo wrapper: `bash scripts/ios-diagnostics.sh`.
- Artifacts must stay under `build/ios-diagnostics/`.
- Do not edit global tools in `~/.local/bin` or Homebrew. Treat them as installed tools only.

## Tool loop

1. Check local tool availability:

```bash
bash scripts/ios-diagnostics.sh doctor
```

2. Launch or verify the app in the simulator with the repo's normal iOS flow, then capture OSLog while reproducing the issue:

```bash
bash scripts/ios-diagnostics.sh oslog --timeout 30s --category AssistantSurface
```

3. For browser-visible proof, use the iOS simulator browser skill: start `serve-sim` for the same simulator UDID, open `http://localhost:3200/` in the Codex in-app browser, and capture a browser screenshot showing the live simulator frame.

4. For UI traps, emit LLDB setup first. Attach only when you are ready for an interactive or blocking debugger session:

```bash
bash scripts/ios-diagnostics.sh lldb-trap --preset ui
bash scripts/ios-diagnostics.sh lldb-trap --preset ui --attach
```

5. For invalidation or latency regressions, run repeatable xctrace captures. Always pass the simulator UDID so `perf-loop` cannot attach a host process with the same name:

```bash
bash scripts/ios-diagnostics.sh perf-loop --device <simulator-udid> --launch --iterations 3 --time-limit 15s
```

The wrapper resolves `dev.looper.app.ios` through `simctl appinfo`, launches the simulator app when `--launch` is present, and passes the concrete host PID to `perf-loop`. Use `--pid <pid>` only when you have already captured the exact simulator app PID.

6. For ETTrace, keep instrumentation temporary unless the task explicitly asks for permanent app wiring. Capture dSYMs from the matching build when available:

```bash
bash scripts/ios-diagnostics.sh ettrace --simulator --dsyms path/to/dSYMs --launch --verbose
```

The iOS app emits DEBUG-only ETTrace metric notifications for `assistant_surface_selection.<surface>` in the assistant switcher flow. These are no-ops unless ETTrace is linked for that diagnostic build.

7. For one-shot reproductions, collect OSLog and xctrace in the same artifact directory:

```bash
bash scripts/ios-diagnostics.sh capture --device <simulator-udid> --timeout 30s --iterations 3 --time-limit 15s
```

## Reporting

- Report artifact paths and the exact scenario exercised.
- For switcher bugs, include `AssistantSurface` OSLog events: requested, dispatch, selected, stale, cancelled, failed.
- If ETTrace is used, preserve its `output_*.json` and summarize the top main-thread offenders instead of pasting raw traces.
- If a capture cannot run because the app is not installed or launched, state that blocker and run the fastest build/install path rather than guessing.

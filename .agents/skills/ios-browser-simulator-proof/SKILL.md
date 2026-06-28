---
name: ios-browser-simulator-proof
description: Mirror the Looper iOS Simulator into the Codex in-app Browser and use it as the primary observable iOS app surface. Use when the user asks to launch Looper in Codex Browser, prove iOS UI behavior, show the simulator in-app, reproduce taps, collect browser-visible proof, or avoid static screenshots/standalone Chromium for iOS QA.
---

# Looper iOS Browser Simulator Proof

Use this skill to drive Looper's iOS app through the same surface the user can see in Codex: XcodeBuildMCP launches the simulator app, `serve-sim` mirrors that exact simulator, and the Codex in-app Browser opens the live mirror.

This is the primary iOS proof path for Looper. Physical iPhone install is secondary and used for device-only behavior, continuity, signing, APNs, or when the user explicitly asks for phone install.

## Rules

- Prefer XcodeBuildMCP for simulator build/run/test actions.
- Use the Codex in-app Browser plugin, not standalone Chromium, raw Playwright, or a static image page.
- Do not call a browser page loaded unless it shows a live simulator frame. A static JPEG or "connecting" mirror is not proof.
- Do not rebuild repeatedly. If the app is already installed and no source changed, launch the installed app and mirror it.
- Keep `serve-sim` pinned to one explicit simulator UDID. Never run an unscoped `serve-sim --kill`.
- If Browser CUA serializes taps too slowly for rapid-overlap proof, say so and use deterministic tests for overlap while using Browser taps to prove the installed path.

## Simulator Setup

1. Check XcodeBuildMCP defaults first:

```text
session_show_defaults
```

2. If defaults already point at `ios/LooperCompanion.xcodeproj`, scheme `LooperCompanion`, and the intended simulator, do not rediscover the project.

3. If code changed or the app is not installed, use:

```text
build_run_sim
```

4. If the app is already installed and no build is needed, launch it directly:

```bash
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  xcrun simctl launch <simulator-udid> dev.looper.app.ios
```

## Mirror Setup

Start `serve-sim` in a long-running terminal for the same simulator UDID:

```bash
SIM="<simulator-udid>"
cleanup_serve_sim() {
  npx --yes serve-sim@latest --kill "$SIM" >/dev/null 2>&1 || true
}
trap cleanup_serve_sim EXIT INT TERM HUP
cleanup_serve_sim
npx --yes serve-sim@latest "$SIM"
```

Wait for the printed URL, normally `http://localhost:3200`, before opening Browser.

## Browser Setup

Use the `browser:control-in-app-browser` skill's Node REPL setup. After setup, open the mirror and make Browser visible:

```js
if (globalThis.agent?.browsers == null) {
  const { setupBrowserRuntime } = await import("/Users/ay/.codex/plugins/cache/openai-bundled/browser/26.623.42026/scripts/browser-client.mjs");
  await setupBrowserRuntime({ globals: globalThis });
}
globalThis.browser = await agent.browsers.get("iab");
var browserTab = await browser.tabs.selected();
if (!browserTab) browserTab = await browser.tabs.new();
await (await browser.capabilities.get("visibility")).set(true);
await browserTab.goto("http://localhost:3200/");
```

After navigation, capture a screenshot and inspect the page. Success requires the page to say the selected simulator is `live` and to show a real Looper frame:

```js
await new Promise(resolve => setTimeout(resolve, 1800));
var shot = await browserTab.screenshot({ fullPage: false });
await nodeRepl.emitImage(shot);
var info = await browserTab.playwright.evaluate(() => ({
  url: location.href,
  title: document.title,
  bodyText: document.body.innerText.slice(0, 500),
  canvasCount: document.querySelectorAll("canvas").length,
  videoCount: document.querySelectorAll("video").length,
  imageCount: document.querySelectorAll("img").length
}));
nodeRepl.write(JSON.stringify(info, null, 2));
```

If the page says `connecting` or `No simulator`, wait for `serve-sim` capture output, then reload once. Do not report success until the frame is live.

## Browser Interaction

Use Browser CUA for visible tap proof:

```js
await browserTab.cua.click({ x: 282, y: 350 });
await new Promise(resolve => setTimeout(resolve, 100));
await browserTab.cua.click({ x: 350, y: 350 });
var tapShot = await browserTab.screenshot({ fullPage: false });
await nodeRepl.emitImage(tapShot);
```

Pair interaction proof with OSLog when behavior matters:

```bash
bash scripts/ios-diagnostics.sh oslog --timeout 30s --category AssistantSurface
```

For switcher bugs, preserve lines for requested, coalesced, dispatch, applied, stale, and failed states.

## Diagnostics Pairing

Use the repo diagnostics wrapper first; it is wired to the globally installed tools:

- `~/.local/bin/oslog-live` for bounded Unified Logging captures.
- `~/.local/bin/lldb-trap` for focused debugger traps when the UI crashes or hangs.
- `~/.local/bin/perf-loop` for repeatable xctrace captures when visual proof or OSLog does not explain latency.

For SwiftUI flicker, capture Browser-visible taps plus `AssistantSurface` OSLog before reaching for xctrace. If `perf-loop --attach Looper` cannot find the simulator process, try a short all-processes capture once and record the blocker instead of looping on Instruments.

## Stop Conditions

Report:

- Simulator name and UDID.
- Whether a build happened or an already-installed app was launched.
- `serve-sim` URL.
- Browser-visible proof status: live frame or exact blocker.
- Any OSLog path used for the reproduced action.

Do not claim iOS proof from build/test alone when the user asked for app behavior in the Browser.
